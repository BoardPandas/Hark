//! WH_KEYBOARD_LL hook on a dedicated message-loop thread. I/O glue:
//! run-on-real-HW (the install and live key edges cannot be validated
//! without a real Windows session).
//!
//! Load-bearing rules (spec §12):
//! - The hook delivers callbacks ONLY while its installing thread pumps
//!   messages: this thread's entire body is the GetMessageW loop. It never
//!   sleeps, parks, or does other work.
//! - The callback must be fast (Windows silently removes low-level hooks
//!   that exceed the LowLevelHooksTimeout): map the key, feed the tracker,
//!   send on the channel, done.
//! - LLKHF_INJECTED events feed the tracker as `injected` so our own
//!   synthesized Ctrl+V can never re-trigger PTT.
//! - We always CallNextHookEx: Hark observes keys, it never swallows them.
//!   (Holding Ctrl+Win marks the Win press as "used in a chord", so the
//!   Start menu does not fire on release; no swallowing needed.)

use crate::edges::{CaptureEvent, ChordTracker, PttChord, PttEvent, PttKeyCode};
use crate::{HotkeyError, ListenerHandle};
use std::cell::RefCell;
use std::sync::mpsc::{self, Sender};
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VIRTUAL_KEY, VK_CAPITAL, VK_F1, VK_F24, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_RCONTROL,
    VK_RMENU, VK_RSHIFT, VK_RWIN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PostQuitMessage, PostThreadMessageW,
    SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, KBDLLHOOKSTRUCT, LLKHF_INJECTED,
    LLKHF_UP, MSG, WH_KEYBOARD_LL, WM_QUIT,
};

/// Map a Win32 virtual-key code to a chord-capable key. Pure; unit-tested.
fn vk_to_key(vk: u32) -> Option<PttKeyCode> {
    let vk = VIRTUAL_KEY(vk as u16);
    let f_first = VK_F1.0;
    let f_last = VK_F24.0;
    let key = match vk {
        VK_LCONTROL => PttKeyCode::LCtrl,
        VK_RCONTROL => PttKeyCode::RCtrl,
        VK_LSHIFT => PttKeyCode::LShift,
        VK_RSHIFT => PttKeyCode::RShift,
        VK_LMENU => PttKeyCode::LAlt,
        VK_RMENU => PttKeyCode::RAlt,
        VK_LWIN => PttKeyCode::LWin,
        VK_RWIN => PttKeyCode::RWin,
        VK_CAPITAL => PttKeyCode::CapsLock,
        v if (f_first..=f_last).contains(&v.0) => PttKeyCode::F((v.0 - f_first + 1) as u8),
        _ => return None,
    };
    Some(key)
}

/// Per-hook-thread state. The LL hook callback carries no user pointer, but
/// it always runs on the installing thread, so thread-local state is exact.
/// The same hook serves push-to-talk (resolved chord edges) and the settings
/// "record a shortcut" flow (raw key edges); the mode picks which.
enum HookState {
    /// Push-to-talk: feed a `ChordTracker`, emit engage/disengage edges.
    Ptt {
        tracker: ChordTracker,
        tx: Sender<PttEvent>,
    },
    /// Recording: forward every non-injected chord-capable key edge.
    Capture { tx: Sender<CaptureEvent> },
}

thread_local! {
    static HOOK_STATE: RefCell<Option<HookState>> = const { RefCell::new(None) };
}

unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        // lparam points at the event struct for keyboard LL hooks.
        let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let injected = info.flags.contains(LLKHF_INJECTED);
        let down = !info.flags.contains(LLKHF_UP);
        if let Some(key) = vk_to_key(info.vkCode) {
            HOOK_STATE.with(|state| {
                if let Some(s) = state.borrow_mut().as_mut() {
                    // A send error means the receiver is gone (pipeline stopped
                    // or the record UI closed): shut this hook down rather than
                    // hooking keys forever.
                    let disconnected = match s {
                        HookState::Ptt { tracker, tx } => tracker
                            .on_event(key, down, injected)
                            .is_some_and(|event| tx.send(event).is_err()),
                        HookState::Capture { tx } => {
                            // Injected input (our own synthesized Ctrl+V) must
                            // never land in a recorded shortcut.
                            !injected && tx.send(CaptureEvent { key, down }).is_err()
                        }
                    };
                    if disconnected {
                        unsafe { PostQuitMessage(0) };
                    }
                }
            });
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Install the hook for push-to-talk: resolved chord edges arrive on `tx`.
pub(crate) fn spawn_listener(
    chord: PttChord,
    tx: Sender<PttEvent>,
) -> Result<ListenerHandle, HotkeyError> {
    spawn_hook(
        "hark-hotkey",
        HookState::Ptt {
            tracker: ChordTracker::new(chord),
            tx,
        },
    )
}

/// Install the hook for the record-a-shortcut flow: raw key edges arrive on
/// `tx`. Same install/teardown as `spawn_listener`.
pub(crate) fn spawn_capture(tx: Sender<CaptureEvent>) -> Result<ListenerHandle, HotkeyError> {
    spawn_hook("hark-hotkey-capture", HookState::Capture { tx })
}

/// Install the hook and pump messages until WM_QUIT. Runs as the entire body
/// of the dedicated listener thread.
fn spawn_hook(thread_name: &str, hook_state: HookState) -> Result<ListenerHandle, HotkeyError> {
    let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<u32, HotkeyError>>(1);

    let thread = std::thread::Builder::new()
        .name(thread_name.to_string())
        .spawn(move || {
            HOOK_STATE.with(|state| {
                *state.borrow_mut() = Some(hook_state);
            });

            // A low-level hook needs no module handle: the callback runs in
            // this process via the message loop.
            let hook =
                match unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), None, 0) } {
                    Ok(h) => h,
                    Err(e) => {
                        let _ = ready_tx.send(Err(HotkeyError::Install(e.to_string())));
                        return;
                    }
                };
            let _ = ready_tx.send(Ok(unsafe { GetCurrentThreadId() }));

            // The message pump IS the hook's lifeline (spec §12): callbacks
            // are delivered only while this loop runs.
            let mut msg = MSG::default();
            while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
                unsafe {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }

            unsafe {
                if let Err(e) = UnhookWindowsHookEx(hook) {
                    log::warn!("unhooking keyboard hook failed: {e}");
                }
            }
        })
        .map_err(|e| HotkeyError::Install(format!("cannot spawn hook thread: {e}")))?;

    match ready_rx.recv() {
        Ok(Ok(thread_id)) => Ok(ListenerHandle {
            thread_id,
            thread: Some(thread),
        }),
        Ok(Err(e)) => {
            let _ = thread.join();
            Err(e)
        }
        Err(_) => Err(HotkeyError::Install(
            "hook thread died before reporting readiness".to_string(),
        )),
    }
}

/// Ask the listener thread to exit its message loop.
pub(crate) fn stop_listener(thread_id: u32) {
    // Posting fails only if the thread is already gone; nothing to do then.
    unsafe {
        let _ = PostThreadMessageW(thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vk_mapping_covers_the_chord_keys() {
        assert_eq!(vk_to_key(0xA2), Some(PttKeyCode::LCtrl));
        assert_eq!(vk_to_key(0xA3), Some(PttKeyCode::RCtrl));
        assert_eq!(vk_to_key(0xA0), Some(PttKeyCode::LShift));
        assert_eq!(vk_to_key(0xA1), Some(PttKeyCode::RShift));
        assert_eq!(vk_to_key(0xA4), Some(PttKeyCode::LAlt));
        assert_eq!(vk_to_key(0xA5), Some(PttKeyCode::RAlt));
        assert_eq!(vk_to_key(0x5B), Some(PttKeyCode::LWin));
        assert_eq!(vk_to_key(0x5C), Some(PttKeyCode::RWin));
        assert_eq!(vk_to_key(0x14), Some(PttKeyCode::CapsLock));
        assert_eq!(vk_to_key(0x70), Some(PttKeyCode::F(1)));
        assert_eq!(vk_to_key(0x7C), Some(PttKeyCode::F(13)));
        assert_eq!(vk_to_key(0x87), Some(PttKeyCode::F(24)));
    }

    #[test]
    fn vk_mapping_ignores_typing_keys() {
        assert_eq!(vk_to_key(0x41), None); // 'A'
        assert_eq!(vk_to_key(0x20), None); // Space
        assert_eq!(vk_to_key(0x0D), None); // Enter
        assert_eq!(vk_to_key(0x56), None); // 'V' (the paste key!)
    }
}

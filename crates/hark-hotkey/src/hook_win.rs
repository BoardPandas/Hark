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
//! - A hook does NOT see every release. Releases that happen on another
//!   desktop (lock screen, UAC, Ctrl+Alt+Del), across a sleep/resume, or
//!   after Windows quietly unhooks a callback that ran long simply never
//!   arrive — and one lost release wedges the tracker engaged forever: the
//!   recording never ends, the overlay pill never goes away, and no later
//!   press produces an edge either. So while (and only while) a chord is
//!   held, a thread timer polls the real key state and heals the difference
//!   (`watchdog_tick`).

use crate::capture::{CaptureEvent, HeldScan};
use crate::edges::{ChordTracker, PttChord, PttEvent, PttKeyCode};
use crate::{CaptureTap, HotkeyError, ListenerHandle};
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VIRTUAL_KEY, VK_CAPITAL, VK_F1, VK_F24, VK_LCONTROL, VK_LMENU, VK_LSHIFT,
    VK_LWIN, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, KillTimer, PostQuitMessage, PostThreadMessageW,
    SetTimer, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, KBDLLHOOKSTRUCT,
    LLKHF_INJECTED, LLKHF_UP, MSG, WH_KEYBOARD_LL, WM_QUIT, WM_TIMER,
};

/// How often the watchdog re-checks that the held chord is really still held.
/// Only runs between engage and disengage, so an idle Hark posts no timers at
/// all. Short enough that a lost release costs a fraction of a second of
/// trailing audio, long enough to be free (a handful of key-state reads).
const WATCHDOG_MS: u32 = 250;

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

/// The inverse of [`vk_to_key`], for asking Windows whether a chord key is
/// physically down. Round-trip unit-tested against `vk_to_key`.
fn key_to_vk(key: PttKeyCode) -> VIRTUAL_KEY {
    match key {
        PttKeyCode::LCtrl => VK_LCONTROL,
        PttKeyCode::RCtrl => VK_RCONTROL,
        PttKeyCode::LShift => VK_LSHIFT,
        PttKeyCode::RShift => VK_RSHIFT,
        PttKeyCode::LAlt => VK_LMENU,
        PttKeyCode::RAlt => VK_RMENU,
        PttKeyCode::LWin => VK_LWIN,
        PttKeyCode::RWin => VK_RWIN,
        PttKeyCode::CapsLock => VK_CAPITAL,
        // Chords only ever carry F1..=F24 (parse and capture both enforce it);
        // clamping keeps a bogus index inside the F-key block regardless.
        PttKeyCode::F(n) => VIRTUAL_KEY(VK_F1.0 + u16::from(n.clamp(1, 24)) - 1),
    }
}

/// Is this key physically held right now? `GetAsyncKeyState`'s high bit is the
/// physical state (its low bit is the CapsLock-style toggle, which we ignore).
fn physically_down(key: PttKeyCode) -> bool {
    // SAFETY: a pure getter over a virtual-key code, no out-params.
    let state = unsafe { GetAsyncKeyState(i32::from(key_to_vk(key).0)) };
    (state as u16) & 0x8000 != 0
}

/// Per-hook-thread state. The LL hook callback carries no user pointer, but
/// it always runs on the installing thread, so thread-local state is exact.
/// The same hook serves push-to-talk (resolved chord edges) and the settings
/// "record a shortcut" flow (raw key edges); the mode picks which.
enum HookState {
    /// Push-to-talk: feed a `ChordTracker`, emit engage/disengage edges —
    /// unless the settings recorder has armed the tap, in which case the raw
    /// edge goes there instead and the tracker never sees it.
    Ptt {
        tracker: ChordTracker,
        tx: Sender<PttEvent>,
        /// Published once, immediately after the hook installs. A `OnceLock`
        /// rather than a plain `Arc` only because the tap needs the hook
        /// thread's liveness flag, which does not exist until the thread does.
        tap: Arc<std::sync::OnceLock<Arc<CaptureTap>>>,
    },
    /// Recording: forward every non-injected chord-capable key edge.
    Capture { tx: Sender<CaptureEvent> },
}

/// Clears the handle's liveness flag on the way out of the hook thread,
/// including an unwind, so `ListenerHandle::is_alive` is never optimistic.
struct AliveGuard(Arc<AtomicBool>);

impl Drop for AliveGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

thread_local! {
    static HOOK_STATE: RefCell<Option<HookState>> = const { RefCell::new(None) };
    /// Id of the live watchdog timer, or 0 when the chord is not held.
    static WATCHDOG: Cell<usize> = const { Cell::new(0) };
}

/// Arm the watchdog for the duration of a hold, or disarm it. Idempotent, and
/// called only from the hook thread (which owns the timer and its queue).
fn set_watchdog(armed: bool) {
    let id = WATCHDOG.with(|w| w.get());
    // SAFETY (both arms): thread timers, created and destroyed on this thread.
    // A null window handle posts WM_TIMER to this thread's own queue.
    if armed && id == 0 {
        let id = unsafe { SetTimer(None, 0, WATCHDOG_MS, None) };
        if id == 0 {
            log::warn!("push-to-talk watchdog timer could not be created");
        }
        WATCHDOG.with(|w| w.set(id));
    } else if !armed && id != 0 {
        if let Err(e) = unsafe { KillTimer(None, id) } {
            log::warn!("push-to-talk watchdog timer could not be stopped: {e}");
        }
        WATCHDOG.with(|w| w.set(0));
    }
}

/// One watchdog poll: if the chord the tracker thinks is held is no longer
/// physically down, the release event went missing — emit it ourselves so the
/// recording ends. Cheap by construction: a few key-state reads, and only
/// while a chord is engaged.
fn watchdog_tick() {
    let mut disconnected = false;
    let mut healed = false;
    HOOK_STATE.with(|state| {
        if let Some(HookState::Ptt { tracker, tx, .. }) = state.borrow_mut().as_mut() {
            if let Some(event) = tracker.resync_released(physically_down) {
                log::warn!("push-to-talk release never arrived; ending the recording");
                disconnected = tx.send(event).is_err();
                healed = true;
            }
        }
    });
    if healed {
        set_watchdog(false);
    }
    if disconnected {
        // Same contract as the hook callback: no receiver, no reason to hook.
        unsafe { PostQuitMessage(0) };
    }
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
                        // Recording a shortcut: the raw edge goes to the
                        // settings UI and the tracker is bypassed entirely, so
                        // the chord being recorded cannot also fire a dictation.
                        HookState::Ptt { tap, .. }
                            if !injected && tap.get().is_some_and(|t| t.forward(key, down)) =>
                        {
                            false
                        }
                        HookState::Ptt { tracker, tx, .. } => {
                            match tracker.on_event_verified(key, down, injected, physically_down) {
                                Some(event) => {
                                    // The watchdog exists only for the span of
                                    // a hold: armed on engage, disarmed on the
                                    // release that ends it.
                                    set_watchdog(event == PttEvent::Down);
                                    tx.send(event).is_err()
                                }
                                None => false,
                            }
                        }
                        HookState::Capture { tx } => {
                            // Injected input (our own synthesized Ctrl+V) must
                            // never land in a recorded shortcut.
                            !injected && tx.send(CaptureEvent { key, down }).is_err()
                        }
                    };
                    if disconnected {
                        log::warn!("keyboard hook receiver is gone; stopping the hook");
                        unsafe { PostQuitMessage(0) };
                    }
                }
            });
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// How often the scanner re-reads real key state while a recording is open.
/// Fast enough that a deliberate tap of a shortcut cannot slip between ticks,
/// and cheap by construction: 33 `GetAsyncKeyState` reads, each a user-mode
/// lookup, on a thread that exists only while the settings recorder is up.
const SCAN_MS: u64 = 15;

/// Watch real key state for as long as the tap is armed, feeding the recorder
/// the same edges the hook does.
///
/// This is the recorder's second source, not its only one, and it is
/// deliberately NOT wired to `ChordTracker`: the crate rule is heal releases,
/// never presses, and a press synthesized from a poll would start a dictation
/// nobody asked for. Nothing here runs in the hook callback — `GetAsyncKeyState`
/// is documented as not yet reflecting the key that is currently being
/// delivered, so polling from inside the callback would read stale state.
///
/// Retires on the next tick after the tap disarms. Arming again spawns a fresh
/// one; if that happens inside a tick the two overlap briefly, which is
/// harmless — duplicate edges are a no-op for `CaptureBuffer`.
pub(crate) fn spawn_scanner(tap: std::sync::Arc<crate::CaptureTap>) {
    let spawned = std::thread::Builder::new()
        .name("hark-hotkey-scan".to_string())
        .spawn(move || {
            let mut scan = HeldScan::new(physically_down);
            while tap.armed() {
                std::thread::sleep(std::time::Duration::from_millis(SCAN_MS));
                if !tap.armed() {
                    break;
                }
                scan.tick(physically_down, |event| tap.emit_polled(event));
            }
        });
    if let Err(e) = spawned {
        // The hook tap still works; the recorder just loses its backstop.
        log::warn!("could not start the shortcut scanner: {e}");
    }
}

/// Install the hook for push-to-talk: resolved chord edges arrive on `tx`.
pub(crate) fn spawn_listener(
    chord: PttChord,
    tx: Sender<PttEvent>,
) -> Result<ListenerHandle, HotkeyError> {
    let (capture_tx, capture_rx) = mpsc::channel();
    // The tap needs the hook thread's liveness flag, and the flag is created
    // inside spawn_hook, so the tap is built from the handle afterwards and
    // the hook thread reads it through a shared cell set before it can fire.
    let shared: Arc<std::sync::OnceLock<Arc<CaptureTap>>> = Arc::new(std::sync::OnceLock::new());
    let mut handle = spawn_hook(
        "hark-hotkey",
        HookState::Ptt {
            tracker: ChordTracker::new(chord),
            tx,
            tap: shared.clone(),
        },
    )?;
    let tap = Arc::new(CaptureTap::new(capture_tx, handle.alive.clone()));
    let _ = shared.set(tap.clone());
    handle.tap = Some(tap);
    handle.capture_rx = Some(capture_rx);
    Ok(handle)
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
    let alive = Arc::new(AtomicBool::new(true));

    let name = thread_name.to_string();
    let thread_alive = alive.clone();
    let thread = std::thread::Builder::new()
        .name(thread_name.to_string())
        .spawn(move || {
            // Cleared however this thread leaves — a clean WM_QUIT, a lost
            // receiver, or a panic — so a caller can never mistake a dead hook
            // for a live one.
            let _alive = AliveGuard(thread_alive);

            HOOK_STATE.with(|state| {
                *state.borrow_mut() = Some(hook_state);
            });

            // A low-level hook needs no module handle: the callback runs in
            // this process via the message loop.
            let hook =
                match unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), None, 0) } {
                    Ok(h) => h,
                    Err(e) => {
                        log::error!("{name}: keyboard hook install failed: {e}");
                        let _ = ready_tx.send(Err(HotkeyError::Install(e.to_string())));
                        return;
                    }
                };
            log::info!("{name}: keyboard hook installed");
            let _ = ready_tx.send(Ok(unsafe { GetCurrentThreadId() }));

            // The message pump IS the hook's lifeline (spec §12): callbacks
            // are delivered only while this loop runs.
            let mut msg = MSG::default();
            while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
                // The watchdog runs from the pump itself: a thread timer has
                // no window, so DispatchMessageW would drop WM_TIMER on the
                // floor. It does not violate "the pump IS the hook" — the tick
                // is a handful of key-state reads that never blocks.
                if msg.message == WM_TIMER {
                    watchdog_tick();
                    continue;
                }
                unsafe {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            set_watchdog(false);

            unsafe {
                if let Err(e) = UnhookWindowsHookEx(hook) {
                    log::warn!("{name}: unhooking keyboard hook failed: {e}");
                }
            }
            log::info!("{name}: keyboard hook removed");
        })
        .map_err(|e| HotkeyError::Install(format!("cannot spawn hook thread: {e}")))?;

    match ready_rx.recv() {
        Ok(Ok(thread_id)) => Ok(ListenerHandle {
            thread_id,
            alive,
            tap: None,
            capture_rx: None,
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
    fn vk_mapping_round_trips_for_every_chord_key() {
        // Every key in CHORD_KEYS, not a hand-picked dozen: the scanner reads
        // key_to_vk for all 33 each tick, so a mismatch anywhere would invent a
        // press or miss a release for that key. The watchdog only ever touched
        // the handful in a configured chord, which is why 12 used to be enough.
        for key in crate::capture::CHORD_KEYS {
            assert_eq!(vk_to_key(u32::from(key_to_vk(key).0)), Some(key), "{key}");
        }
    }

    #[test]
    fn vk_mapping_ignores_typing_keys() {
        assert_eq!(vk_to_key(0x41), None); // 'A'
        assert_eq!(vk_to_key(0x20), None); // Space
        assert_eq!(vk_to_key(0x0D), None); // Enter
        assert_eq!(vk_to_key(0x56), None); // 'V' (the paste key!)
    }
}

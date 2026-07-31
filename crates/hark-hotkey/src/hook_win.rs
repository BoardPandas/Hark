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
use crate::edges::{ChordTracker, PttChord, PttEvent};
use crate::keycode::PttKeyCode;
use crate::{CaptureTap, HotkeyError, ListenerHandle};
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VIRTUAL_KEY, VK_0, VK_1, VK_2, VK_3, VK_4, VK_5, VK_6, VK_7, VK_8, VK_9,
    VK_A, VK_ADD, VK_APPS, VK_B, VK_BACK, VK_C, VK_CAPITAL, VK_D, VK_DECIMAL, VK_DELETE, VK_DIVIDE,
    VK_DOWN, VK_E, VK_END, VK_F, VK_F1, VK_F10, VK_F11, VK_F12, VK_F13, VK_F14, VK_F15, VK_F16,
    VK_F17, VK_F18, VK_F19, VK_F2, VK_F20, VK_F21, VK_F22, VK_F23, VK_F24, VK_F3, VK_F4, VK_F5,
    VK_F6, VK_F7, VK_F8, VK_F9, VK_G, VK_H, VK_HOME, VK_I, VK_INSERT, VK_J, VK_K, VK_L,
    VK_LCONTROL, VK_LEFT, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_M, VK_MULTIPLY, VK_N, VK_NEXT,
    VK_NUMLOCK, VK_NUMPAD0, VK_NUMPAD1, VK_NUMPAD2, VK_NUMPAD3, VK_NUMPAD4, VK_NUMPAD5, VK_NUMPAD6,
    VK_NUMPAD7, VK_NUMPAD8, VK_NUMPAD9, VK_O, VK_OEM_1, VK_OEM_102, VK_OEM_2, VK_OEM_3, VK_OEM_4,
    VK_OEM_5, VK_OEM_6, VK_OEM_7, VK_OEM_8, VK_OEM_COMMA, VK_OEM_MINUS, VK_OEM_PERIOD, VK_OEM_PLUS,
    VK_P, VK_PRIOR, VK_Q, VK_R, VK_RCONTROL, VK_RETURN, VK_RIGHT, VK_RMENU, VK_RSHIFT, VK_RWIN,
    VK_S, VK_SCROLL, VK_SPACE, VK_SUBTRACT, VK_T, VK_TAB, VK_U, VK_UP, VK_V, VK_W, VK_X, VK_Y,
    VK_Z,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, KillTimer, PostQuitMessage, PostThreadMessageW,
    SetTimer, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, KBDLLHOOKSTRUCT,
    LLKHF_INJECTED, LLKHF_UP, MSG, WH_KEYBOARD_LL, WM_QUIT, WM_TIMER,
};
use PttKeyCode as K;

/// How often the watchdog re-checks that the held chord is really still held.
/// Only runs between engage and disengage, so an idle Hark posts no timers at
/// all. Short enough that a lost release costs a fraction of a second of
/// trailing audio, long enough to be free (a handful of key-state reads).
const WATCHDOG_MS: u32 = 250;

/// Map a Win32 virtual-key code to a chord-capable key. Pure; round-trip tested
/// against [`key_to_vk`] over every key in `ALL_KEYS`.
fn vk_to_key(vk: u32) -> Option<PttKeyCode> {
    let vk = VIRTUAL_KEY(vk as u16);
    let key = match vk {
        VK_LCONTROL => K::LCtrl,
        VK_RCONTROL => K::RCtrl,
        VK_LSHIFT => K::LShift,
        VK_RSHIFT => K::RShift,
        VK_LMENU => K::LAlt,
        VK_RMENU => K::RAlt,
        VK_LWIN => K::LWin,
        VK_RWIN => K::RWin,
        VK_CAPITAL => K::CapsLock,
        VK_NUMLOCK => K::NumLock,
        VK_SCROLL => K::ScrollLock,
        VK_APPS => K::Apps,
        VK_F1 => K::F1,
        VK_F2 => K::F2,
        VK_F3 => K::F3,
        VK_F4 => K::F4,
        VK_F5 => K::F5,
        VK_F6 => K::F6,
        VK_F7 => K::F7,
        VK_F8 => K::F8,
        VK_F9 => K::F9,
        VK_F10 => K::F10,
        VK_F11 => K::F11,
        VK_F12 => K::F12,
        VK_F13 => K::F13,
        VK_F14 => K::F14,
        VK_F15 => K::F15,
        VK_F16 => K::F16,
        VK_F17 => K::F17,
        VK_F18 => K::F18,
        VK_F19 => K::F19,
        VK_F20 => K::F20,
        VK_F21 => K::F21,
        VK_F22 => K::F22,
        VK_F23 => K::F23,
        VK_F24 => K::F24,
        VK_A => K::A,
        VK_B => K::B,
        VK_C => K::C,
        VK_D => K::D,
        VK_E => K::E,
        VK_F => K::F,
        VK_G => K::G,
        VK_H => K::H,
        VK_I => K::I,
        VK_J => K::J,
        VK_K => K::K,
        VK_L => K::L,
        VK_M => K::M,
        VK_N => K::N,
        VK_O => K::O,
        VK_P => K::P,
        VK_Q => K::Q,
        VK_R => K::R,
        VK_S => K::S,
        VK_T => K::T,
        VK_U => K::U,
        VK_V => K::V,
        VK_W => K::W,
        VK_X => K::X,
        VK_Y => K::Y,
        VK_Z => K::Z,
        VK_0 => K::Digit0,
        VK_1 => K::Digit1,
        VK_2 => K::Digit2,
        VK_3 => K::Digit3,
        VK_4 => K::Digit4,
        VK_5 => K::Digit5,
        VK_6 => K::Digit6,
        VK_7 => K::Digit7,
        VK_8 => K::Digit8,
        VK_9 => K::Digit9,
        VK_LEFT => K::Left,
        VK_RIGHT => K::Right,
        VK_UP => K::Up,
        VK_DOWN => K::Down,
        VK_INSERT => K::Insert,
        VK_DELETE => K::Delete,
        VK_HOME => K::Home,
        VK_END => K::End,
        VK_PRIOR => K::PageUp,
        VK_NEXT => K::PageDown,
        VK_TAB => K::Tab,
        VK_SPACE => K::Space,
        VK_RETURN => K::Enter,
        VK_BACK => K::Backspace,
        VK_NUMPAD0 => K::Numpad0,
        VK_NUMPAD1 => K::Numpad1,
        VK_NUMPAD2 => K::Numpad2,
        VK_NUMPAD3 => K::Numpad3,
        VK_NUMPAD4 => K::Numpad4,
        VK_NUMPAD5 => K::Numpad5,
        VK_NUMPAD6 => K::Numpad6,
        VK_NUMPAD7 => K::Numpad7,
        VK_NUMPAD8 => K::Numpad8,
        VK_NUMPAD9 => K::Numpad9,
        VK_ADD => K::NumpadAdd,
        VK_SUBTRACT => K::NumpadSubtract,
        VK_MULTIPLY => K::NumpadMultiply,
        VK_DIVIDE => K::NumpadDivide,
        VK_DECIMAL => K::NumpadDecimal,
        VK_OEM_PLUS => K::Equals,
        VK_OEM_COMMA => K::Comma,
        VK_OEM_MINUS => K::Minus,
        VK_OEM_PERIOD => K::Period,
        VK_OEM_1 => K::Semicolon,
        VK_OEM_2 => K::Slash,
        VK_OEM_3 => K::Backtick,
        VK_OEM_4 => K::LeftBracket,
        VK_OEM_5 => K::Backslash,
        VK_OEM_6 => K::RightBracket,
        VK_OEM_7 => K::Quote,
        VK_OEM_8 => K::Oem8,
        VK_OEM_102 => K::Oem102,
        _ => return None,
    };
    Some(key)
}

/// The inverse of [`vk_to_key`], for asking Windows whether a key is physically
/// down. Both the 15 ms recording scanner and the push-to-talk watchdog resolve
/// state through this, so a wrong entry here invents a press or loses a release.
fn key_to_vk(key: PttKeyCode) -> VIRTUAL_KEY {
    match key {
        K::LCtrl => VK_LCONTROL,
        K::RCtrl => VK_RCONTROL,
        K::LShift => VK_LSHIFT,
        K::RShift => VK_RSHIFT,
        K::LAlt => VK_LMENU,
        K::RAlt => VK_RMENU,
        K::LWin => VK_LWIN,
        K::RWin => VK_RWIN,
        K::CapsLock => VK_CAPITAL,
        K::NumLock => VK_NUMLOCK,
        K::ScrollLock => VK_SCROLL,
        K::Apps => VK_APPS,
        K::F1 => VK_F1,
        K::F2 => VK_F2,
        K::F3 => VK_F3,
        K::F4 => VK_F4,
        K::F5 => VK_F5,
        K::F6 => VK_F6,
        K::F7 => VK_F7,
        K::F8 => VK_F8,
        K::F9 => VK_F9,
        K::F10 => VK_F10,
        K::F11 => VK_F11,
        K::F12 => VK_F12,
        K::F13 => VK_F13,
        K::F14 => VK_F14,
        K::F15 => VK_F15,
        K::F16 => VK_F16,
        K::F17 => VK_F17,
        K::F18 => VK_F18,
        K::F19 => VK_F19,
        K::F20 => VK_F20,
        K::F21 => VK_F21,
        K::F22 => VK_F22,
        K::F23 => VK_F23,
        K::F24 => VK_F24,
        K::A => VK_A,
        K::B => VK_B,
        K::C => VK_C,
        K::D => VK_D,
        K::E => VK_E,
        K::F => VK_F,
        K::G => VK_G,
        K::H => VK_H,
        K::I => VK_I,
        K::J => VK_J,
        K::K => VK_K,
        K::L => VK_L,
        K::M => VK_M,
        K::N => VK_N,
        K::O => VK_O,
        K::P => VK_P,
        K::Q => VK_Q,
        K::R => VK_R,
        K::S => VK_S,
        K::T => VK_T,
        K::U => VK_U,
        K::V => VK_V,
        K::W => VK_W,
        K::X => VK_X,
        K::Y => VK_Y,
        K::Z => VK_Z,
        K::Digit0 => VK_0,
        K::Digit1 => VK_1,
        K::Digit2 => VK_2,
        K::Digit3 => VK_3,
        K::Digit4 => VK_4,
        K::Digit5 => VK_5,
        K::Digit6 => VK_6,
        K::Digit7 => VK_7,
        K::Digit8 => VK_8,
        K::Digit9 => VK_9,
        K::Left => VK_LEFT,
        K::Right => VK_RIGHT,
        K::Up => VK_UP,
        K::Down => VK_DOWN,
        K::Insert => VK_INSERT,
        K::Delete => VK_DELETE,
        K::Home => VK_HOME,
        K::End => VK_END,
        K::PageUp => VK_PRIOR,
        K::PageDown => VK_NEXT,
        K::Tab => VK_TAB,
        K::Space => VK_SPACE,
        K::Enter => VK_RETURN,
        K::Backspace => VK_BACK,
        K::Numpad0 => VK_NUMPAD0,
        K::Numpad1 => VK_NUMPAD1,
        K::Numpad2 => VK_NUMPAD2,
        K::Numpad3 => VK_NUMPAD3,
        K::Numpad4 => VK_NUMPAD4,
        K::Numpad5 => VK_NUMPAD5,
        K::Numpad6 => VK_NUMPAD6,
        K::Numpad7 => VK_NUMPAD7,
        K::Numpad8 => VK_NUMPAD8,
        K::Numpad9 => VK_NUMPAD9,
        K::NumpadAdd => VK_ADD,
        K::NumpadSubtract => VK_SUBTRACT,
        K::NumpadMultiply => VK_MULTIPLY,
        K::NumpadDivide => VK_DIVIDE,
        K::NumpadDecimal => VK_DECIMAL,
        K::Equals => VK_OEM_PLUS,
        K::Comma => VK_OEM_COMMA,
        K::Minus => VK_OEM_MINUS,
        K::Period => VK_OEM_PERIOD,
        K::Semicolon => VK_OEM_1,
        K::Slash => VK_OEM_2,
        K::Backtick => VK_OEM_3,
        K::LeftBracket => VK_OEM_4,
        K::Backslash => VK_OEM_5,
        K::RightBracket => VK_OEM_6,
        K::Quote => VK_OEM_7,
        K::Oem8 => VK_OEM_8,
        K::Oem102 => VK_OEM_102,
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
    // Stack local, false by default: every path that is not the one exception
    // below -- an unmapped VK, an armed capture tap, a lost receiver -- falls
    // through to CallNextHookEx unchanged.
    let mut swallow = false;
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
                            // Asked AFTER the tracker consumes the event, so
                            // the engage edge itself can be swallowed. Reads
                            // only tracker state; see ChordTracker::swallow.
                            let event =
                                tracker.on_event_verified(key, down, injected, physically_down);
                            swallow = tracker.swallow(key, down, injected);
                            match event {
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
    if swallow {
        // The ONE exception to "observe, never swallow": a lock key pressed as
        // part of the running chord, so the dictation does not also flip Caps
        // Lock or Scroll Lock. Non-zero tells Windows to discard the event.
        return LRESULT(1);
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// How often the scanner re-reads real key state while a recording is open.
/// Fast enough that a deliberate tap of a shortcut cannot slip between ticks.
/// Cost is 114 `GetAsyncKeyState` reads per tick; each is a light win32k
/// transition (~1 us), NOT the user-mode lookup an earlier comment here
/// claimed, so a tick is on the order of 100 us and the thread exists only
/// while the settings recorder is open.
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
    swallow_locks: bool,
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
            tracker: ChordTracker::with_lock_suppression(chord, swallow_locks),
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
        assert_eq!(vk_to_key(0x70), Some(PttKeyCode::F1));
        assert_eq!(vk_to_key(0x7C), Some(PttKeyCode::F13));
        assert_eq!(vk_to_key(0x87), Some(PttKeyCode::F24));
    }

    #[test]
    fn vk_mapping_round_trips_for_every_chord_key() {
        // Every key in CHORD_KEYS, not a hand-picked dozen: the scanner reads
        // key_to_vk for all 33 each tick, so a mismatch anywhere would invent a
        // press or miss a release for that key. The watchdog only ever touched
        // the handful in a configured chord, which is why 12 used to be enough.
        for key in crate::keycode::ALL_KEYS {
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

//! evdev push-to-talk hook for Linux. I/O glue: run-on-real-HW (the device
//! scan and live key edges cannot be validated headless).
//!
//! **Why evdev and not X11.** The X11 grab APIs the rest of the ecosystem
//! reaches for are invisible under Wayland, which is now the default session
//! on GNOME and KDE — an X11 hook would leave push-to-talk silently dead for
//! most of the target audience. Reading `/dev/input/event*` sits below the
//! display server, so one implementation covers X11, Wayland and a bare TTY
//! identically. The cost is a permission: the user must be able to read those
//! nodes (the `input` group; see `packaging/70-hark-uinput.rules`).
//!
//! Load-bearing rules, mirroring `hook_win.rs` where the platforms agree:
//! - **One thread owns every device.** `poll(2)` over all keyboard fds plus a
//!   self-pipe; the tracker, the watchdog and the rescan all run on it, so no
//!   lock guards the tracker and no edge can be observed out of order.
//! - **Our own synthesized Ctrl+V must never re-trigger push-to-talk.** There
//!   is no `LLKHF_INJECTED` here: a uinput device is indistinguishable from
//!   real hardware once its events reach `/dev/input`. So the scan skips
//!   devices whose name starts with `Hark` — which is what `hark-inject` names
//!   the virtual keyboard it pastes through. Both ends key off the app name so
//!   the two cannot drift apart; see `hark_inject::VIRTUAL_KEYBOARD_NAME`.
//! - **Observe, never swallow.** Suppressing a key here would need
//!   `EVIOCGRAB`, which takes the device *exclusively* — every other keystroke
//!   would stop reaching the focused app too. So `swallow_locks` is inert on
//!   Linux: a chord containing Caps/Scroll Lock still toggles the lock. The
//!   Windows hook can be surgical about this; the kernel interface cannot.
//! - **Auto-repeat is dropped** (`value == 2`). The tracker deals in edges,
//!   and a held chord would otherwise manufacture a down every ~30 ms and
//!   inflate the recorder's edge counter into nonsense.
//! - **Hotplug is real here.** A USB keyboard plugged in after launch is a new
//!   `eventN` node that no open fd covers, so the loop rescans on a cadence
//!   rather than trusting the set it opened at startup.

use crate::capture::CaptureEvent;
use crate::edges::{ChordTracker, PttChord, PttEvent};
use crate::keycode::{PttKeyCode, ALL_KEYS};
use crate::{CaptureTap, HotkeyError, ListenerHandle};
use evdev::{Device, EventType, KeyCode};
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};
use PttKeyCode as K;

/// How often the loop re-checks that a held chord is still physically held.
/// Same reasoning as the Windows watchdog: a release that lands while the
/// device is momentarily unreadable (VT switch, suspend/resume, an unplug
/// mid-hold) would otherwise wedge the tracker engaged forever.
const WATCHDOG_MS: i32 = 250;

/// How often the loop looks for keyboards that appeared or vanished. Also the
/// poll timeout when no chord is held, so an idle Hark wakes twice a second to
/// run one `readdir` and nothing else.
const RESCAN_MS: i32 = 2_000;

/// Devices named with this prefix are ours (`hark-inject`'s uinput keyboard)
/// and are never read: doing so would feed our own synthesized Ctrl+V straight
/// back into the chord tracker. Deliberately the bare app name rather than the
/// full device name, so the two crates agree without sharing a constant.
const OWN_DEVICE_PREFIX: &str = "Hark";

/// Map an evdev key code to a chord-capable key. Pure; round-trip tested
/// against [`key_to_evdev`] over every key in `ALL_KEYS`.
fn evdev_to_key(code: KeyCode) -> Option<PttKeyCode> {
    let key = match code {
        KeyCode::KEY_LEFTCTRL => K::LCtrl,
        KeyCode::KEY_RIGHTCTRL => K::RCtrl,
        KeyCode::KEY_LEFTSHIFT => K::LShift,
        KeyCode::KEY_RIGHTSHIFT => K::RShift,
        KeyCode::KEY_LEFTALT => K::LAlt,
        KeyCode::KEY_RIGHTALT => K::RAlt,
        KeyCode::KEY_LEFTMETA => K::LWin,
        KeyCode::KEY_RIGHTMETA => K::RWin,
        KeyCode::KEY_CAPSLOCK => K::CapsLock,
        KeyCode::KEY_NUMLOCK => K::NumLock,
        KeyCode::KEY_SCROLLLOCK => K::ScrollLock,
        // The "menu" key. Windows calls it Apps; evdev calls it Compose.
        KeyCode::KEY_COMPOSE => K::Apps,
        KeyCode::KEY_F1 => K::F1,
        KeyCode::KEY_F2 => K::F2,
        KeyCode::KEY_F3 => K::F3,
        KeyCode::KEY_F4 => K::F4,
        KeyCode::KEY_F5 => K::F5,
        KeyCode::KEY_F6 => K::F6,
        KeyCode::KEY_F7 => K::F7,
        KeyCode::KEY_F8 => K::F8,
        KeyCode::KEY_F9 => K::F9,
        KeyCode::KEY_F10 => K::F10,
        KeyCode::KEY_F11 => K::F11,
        KeyCode::KEY_F12 => K::F12,
        KeyCode::KEY_F13 => K::F13,
        KeyCode::KEY_F14 => K::F14,
        KeyCode::KEY_F15 => K::F15,
        KeyCode::KEY_F16 => K::F16,
        KeyCode::KEY_F17 => K::F17,
        KeyCode::KEY_F18 => K::F18,
        KeyCode::KEY_F19 => K::F19,
        KeyCode::KEY_F20 => K::F20,
        KeyCode::KEY_F21 => K::F21,
        KeyCode::KEY_F22 => K::F22,
        KeyCode::KEY_F23 => K::F23,
        KeyCode::KEY_F24 => K::F24,
        KeyCode::KEY_A => K::A,
        KeyCode::KEY_B => K::B,
        KeyCode::KEY_C => K::C,
        KeyCode::KEY_D => K::D,
        KeyCode::KEY_E => K::E,
        KeyCode::KEY_F => K::F,
        KeyCode::KEY_G => K::G,
        KeyCode::KEY_H => K::H,
        KeyCode::KEY_I => K::I,
        KeyCode::KEY_J => K::J,
        KeyCode::KEY_K => K::K,
        KeyCode::KEY_L => K::L,
        KeyCode::KEY_M => K::M,
        KeyCode::KEY_N => K::N,
        KeyCode::KEY_O => K::O,
        KeyCode::KEY_P => K::P,
        KeyCode::KEY_Q => K::Q,
        KeyCode::KEY_R => K::R,
        KeyCode::KEY_S => K::S,
        KeyCode::KEY_T => K::T,
        KeyCode::KEY_U => K::U,
        KeyCode::KEY_V => K::V,
        KeyCode::KEY_W => K::W,
        KeyCode::KEY_X => K::X,
        KeyCode::KEY_Y => K::Y,
        KeyCode::KEY_Z => K::Z,
        KeyCode::KEY_0 => K::Digit0,
        KeyCode::KEY_1 => K::Digit1,
        KeyCode::KEY_2 => K::Digit2,
        KeyCode::KEY_3 => K::Digit3,
        KeyCode::KEY_4 => K::Digit4,
        KeyCode::KEY_5 => K::Digit5,
        KeyCode::KEY_6 => K::Digit6,
        KeyCode::KEY_7 => K::Digit7,
        KeyCode::KEY_8 => K::Digit8,
        KeyCode::KEY_9 => K::Digit9,
        KeyCode::KEY_LEFT => K::Left,
        KeyCode::KEY_RIGHT => K::Right,
        KeyCode::KEY_UP => K::Up,
        KeyCode::KEY_DOWN => K::Down,
        KeyCode::KEY_INSERT => K::Insert,
        KeyCode::KEY_DELETE => K::Delete,
        KeyCode::KEY_HOME => K::Home,
        KeyCode::KEY_END => K::End,
        KeyCode::KEY_PAGEUP => K::PageUp,
        KeyCode::KEY_PAGEDOWN => K::PageDown,
        KeyCode::KEY_TAB => K::Tab,
        KeyCode::KEY_SPACE => K::Space,
        KeyCode::KEY_ENTER => K::Enter,
        // The enum has one Enter by design ("one variant per virtual key,
        // never finer" -- keycode.rs), so the numpad's Enter folds into it.
        // `key_to_evdev` picks KEY_ENTER as the canonical direction, which is
        // why the round-trip test excludes this code.
        KeyCode::KEY_KPENTER => K::Enter,
        KeyCode::KEY_BACKSPACE => K::Backspace,
        KeyCode::KEY_KP0 => K::Numpad0,
        KeyCode::KEY_KP1 => K::Numpad1,
        KeyCode::KEY_KP2 => K::Numpad2,
        KeyCode::KEY_KP3 => K::Numpad3,
        KeyCode::KEY_KP4 => K::Numpad4,
        KeyCode::KEY_KP5 => K::Numpad5,
        KeyCode::KEY_KP6 => K::Numpad6,
        KeyCode::KEY_KP7 => K::Numpad7,
        KeyCode::KEY_KP8 => K::Numpad8,
        KeyCode::KEY_KP9 => K::Numpad9,
        KeyCode::KEY_KPPLUS => K::NumpadAdd,
        KeyCode::KEY_KPMINUS => K::NumpadSubtract,
        KeyCode::KEY_KPASTERISK => K::NumpadMultiply,
        KeyCode::KEY_KPSLASH => K::NumpadDivide,
        KeyCode::KEY_KPDOT => K::NumpadDecimal,
        KeyCode::KEY_EQUAL => K::Equals,
        KeyCode::KEY_COMMA => K::Comma,
        KeyCode::KEY_MINUS => K::Minus,
        KeyCode::KEY_DOT => K::Period,
        KeyCode::KEY_SEMICOLON => K::Semicolon,
        KeyCode::KEY_SLASH => K::Slash,
        KeyCode::KEY_GRAVE => K::Backtick,
        KeyCode::KEY_LEFTBRACE => K::LeftBracket,
        KeyCode::KEY_BACKSLASH => K::Backslash,
        KeyCode::KEY_RIGHTBRACE => K::RightBracket,
        KeyCode::KEY_APOSTROPHE => K::Quote,
        KeyCode::KEY_102ND => K::Oem102,
        _ => return None,
    };
    Some(key)
}

/// The inverse of [`evdev_to_key`], for asking the kernel whether a key is
/// physically down. `None` for keys with no evdev counterpart, which is a
/// real state and not an oversight: see [`K::Oem8`] below.
fn key_to_evdev(key: PttKeyCode) -> Option<KeyCode> {
    let code = match key {
        K::LCtrl => KeyCode::KEY_LEFTCTRL,
        K::RCtrl => KeyCode::KEY_RIGHTCTRL,
        K::LShift => KeyCode::KEY_LEFTSHIFT,
        K::RShift => KeyCode::KEY_RIGHTSHIFT,
        K::LAlt => KeyCode::KEY_LEFTALT,
        K::RAlt => KeyCode::KEY_RIGHTALT,
        K::LWin => KeyCode::KEY_LEFTMETA,
        K::RWin => KeyCode::KEY_RIGHTMETA,
        K::CapsLock => KeyCode::KEY_CAPSLOCK,
        K::NumLock => KeyCode::KEY_NUMLOCK,
        K::ScrollLock => KeyCode::KEY_SCROLLLOCK,
        K::Apps => KeyCode::KEY_COMPOSE,
        K::F1 => KeyCode::KEY_F1,
        K::F2 => KeyCode::KEY_F2,
        K::F3 => KeyCode::KEY_F3,
        K::F4 => KeyCode::KEY_F4,
        K::F5 => KeyCode::KEY_F5,
        K::F6 => KeyCode::KEY_F6,
        K::F7 => KeyCode::KEY_F7,
        K::F8 => KeyCode::KEY_F8,
        K::F9 => KeyCode::KEY_F9,
        K::F10 => KeyCode::KEY_F10,
        K::F11 => KeyCode::KEY_F11,
        K::F12 => KeyCode::KEY_F12,
        K::F13 => KeyCode::KEY_F13,
        K::F14 => KeyCode::KEY_F14,
        K::F15 => KeyCode::KEY_F15,
        K::F16 => KeyCode::KEY_F16,
        K::F17 => KeyCode::KEY_F17,
        K::F18 => KeyCode::KEY_F18,
        K::F19 => KeyCode::KEY_F19,
        K::F20 => KeyCode::KEY_F20,
        K::F21 => KeyCode::KEY_F21,
        K::F22 => KeyCode::KEY_F22,
        K::F23 => KeyCode::KEY_F23,
        K::F24 => KeyCode::KEY_F24,
        K::A => KeyCode::KEY_A,
        K::B => KeyCode::KEY_B,
        K::C => KeyCode::KEY_C,
        K::D => KeyCode::KEY_D,
        K::E => KeyCode::KEY_E,
        K::F => KeyCode::KEY_F,
        K::G => KeyCode::KEY_G,
        K::H => KeyCode::KEY_H,
        K::I => KeyCode::KEY_I,
        K::J => KeyCode::KEY_J,
        K::K => KeyCode::KEY_K,
        K::L => KeyCode::KEY_L,
        K::M => KeyCode::KEY_M,
        K::N => KeyCode::KEY_N,
        K::O => KeyCode::KEY_O,
        K::P => KeyCode::KEY_P,
        K::Q => KeyCode::KEY_Q,
        K::R => KeyCode::KEY_R,
        K::S => KeyCode::KEY_S,
        K::T => KeyCode::KEY_T,
        K::U => KeyCode::KEY_U,
        K::V => KeyCode::KEY_V,
        K::W => KeyCode::KEY_W,
        K::X => KeyCode::KEY_X,
        K::Y => KeyCode::KEY_Y,
        K::Z => KeyCode::KEY_Z,
        K::Digit0 => KeyCode::KEY_0,
        K::Digit1 => KeyCode::KEY_1,
        K::Digit2 => KeyCode::KEY_2,
        K::Digit3 => KeyCode::KEY_3,
        K::Digit4 => KeyCode::KEY_4,
        K::Digit5 => KeyCode::KEY_5,
        K::Digit6 => KeyCode::KEY_6,
        K::Digit7 => KeyCode::KEY_7,
        K::Digit8 => KeyCode::KEY_8,
        K::Digit9 => KeyCode::KEY_9,
        K::Left => KeyCode::KEY_LEFT,
        K::Right => KeyCode::KEY_RIGHT,
        K::Up => KeyCode::KEY_UP,
        K::Down => KeyCode::KEY_DOWN,
        K::Insert => KeyCode::KEY_INSERT,
        K::Delete => KeyCode::KEY_DELETE,
        K::Home => KeyCode::KEY_HOME,
        K::End => KeyCode::KEY_END,
        K::PageUp => KeyCode::KEY_PAGEUP,
        K::PageDown => KeyCode::KEY_PAGEDOWN,
        K::Tab => KeyCode::KEY_TAB,
        K::Space => KeyCode::KEY_SPACE,
        K::Enter => KeyCode::KEY_ENTER,
        K::Backspace => KeyCode::KEY_BACKSPACE,
        K::Numpad0 => KeyCode::KEY_KP0,
        K::Numpad1 => KeyCode::KEY_KP1,
        K::Numpad2 => KeyCode::KEY_KP2,
        K::Numpad3 => KeyCode::KEY_KP3,
        K::Numpad4 => KeyCode::KEY_KP4,
        K::Numpad5 => KeyCode::KEY_KP5,
        K::Numpad6 => KeyCode::KEY_KP6,
        K::Numpad7 => KeyCode::KEY_KP7,
        K::Numpad8 => KeyCode::KEY_KP8,
        K::Numpad9 => KeyCode::KEY_KP9,
        K::NumpadAdd => KeyCode::KEY_KPPLUS,
        K::NumpadSubtract => KeyCode::KEY_KPMINUS,
        K::NumpadMultiply => KeyCode::KEY_KPASTERISK,
        K::NumpadDivide => KeyCode::KEY_KPSLASH,
        K::NumpadDecimal => KeyCode::KEY_KPDOT,
        K::Equals => KeyCode::KEY_EQUAL,
        K::Comma => KeyCode::KEY_COMMA,
        K::Minus => KeyCode::KEY_MINUS,
        K::Period => KeyCode::KEY_DOT,
        K::Semicolon => KeyCode::KEY_SEMICOLON,
        K::Slash => KeyCode::KEY_SLASH,
        K::Backtick => KeyCode::KEY_GRAVE,
        K::LeftBracket => KeyCode::KEY_LEFTBRACE,
        K::Backslash => KeyCode::KEY_BACKSLASH,
        K::RightBracket => KeyCode::KEY_RIGHTBRACE,
        K::Quote => KeyCode::KEY_APOSTROPHE,
        K::Oem102 => KeyCode::KEY_102ND,
        // Windows defines VK_OEM_8 as "miscellaneous, varies by keyboard" --
        // it is a layout artifact of the Win32 virtual-key space, not a
        // physical key position, so there is nothing in the kernel's key
        // namespace it corresponds to. A chord containing it (only reachable
        // by carrying a config over from Windows) is reported unbindable
        // rather than mapped to some arbitrary neighbour that would fire on
        // the wrong key.
        K::Oem8 => return None,
    };
    Some(code)
}

/// Keys this platform cannot see, from a chord the user configured elsewhere.
/// Logged once at startup so "push-to-talk does nothing" has an answer in the
/// log instead of being a silent dead end.
fn unmappable(chord: &PttChord) -> Vec<PttKeyCode> {
    chord
        .keys()
        .iter()
        .copied()
        .filter(|k| key_to_evdev(*k).is_none())
        .collect()
}

/// Is this device one we should read? Anything exposing at least one key we
/// can map is in — which admits laptop hotkey blocks and media keyboards that
/// a stricter "does it have Q, Escape and Space" test would drop, while still
/// excluding mice, lid switches and power buttons (whose only `EV_KEY` codes
/// are `BTN_*` / `KEY_POWER`, none of them chord-capable).
fn is_chord_capable(device: &Device) -> bool {
    if !device.supported_events().contains(EventType::KEY) {
        return false;
    }
    let Some(keys) = device.supported_keys() else {
        return false;
    };
    ALL_KEYS
        .iter()
        .filter_map(|k| key_to_evdev(*k))
        .any(|code| keys.contains(code))
}

/// Open every readable keyboard, skipping our own virtual device.
///
/// `evdev::enumerate` silently drops nodes it cannot open, so "no devices" is
/// indistinguishable here from "no permission" -- the caller separates the two
/// by looking at whether `/dev/input` has any `event*` nodes at all.
fn scan() -> Vec<(PathBuf, Device)> {
    evdev::enumerate()
        .filter(|(_, device)| {
            !device
                .name()
                .is_some_and(|n| n.starts_with(OWN_DEVICE_PREFIX))
        })
        .filter(|(_, device)| is_chord_capable(device))
        .filter_map(|(path, device)| {
            // Blocking reads would park the loop in one device's fd and
            // starve every other keyboard plus the watchdog.
            match device.set_nonblocking(true) {
                Ok(()) => Some((path, device)),
                Err(e) => {
                    log::warn!(
                        "hark-hotkey: cannot set {} non-blocking: {e}",
                        path.display()
                    );
                    None
                }
            }
        })
        .collect()
}

/// Does `/dev/input` hold event nodes we were not allowed to open? That is the
/// difference between "this machine has no keyboard" (absurd) and "add
/// yourself to the `input` group" (the actual, fixable problem).
fn has_unreadable_nodes() -> bool {
    let Ok(entries) = std::fs::read_dir("/dev/input") else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry
            .file_name()
            .to_str()
            .is_some_and(|n| n.starts_with("event"))
            && Device::open(entry.path()).is_err()
    })
}

/// The permission failure, phrased as the fix. Returned rather than logged so
/// the settings UI can put it in front of the user, who is the only one who
/// can act on it.
fn permission_error() -> HotkeyError {
    HotkeyError::Install(
        "cannot read any keyboard under /dev/input. Hark needs permission to \
         watch for the push-to-talk chord: add yourself to the \"input\" group \
         (sudo usermod -aG input $USER), then log out and back in."
            .to_string(),
    )
}

/// A self-pipe, so `poll` wakes the instant teardown starts instead of after
/// the rescan timeout. The write end lives in the handle, the read end in the
/// loop's pollfd set.
#[derive(Debug)]
pub(crate) struct Stopper {
    write_fd: std::os::fd::OwnedFd,
    stopping: Arc<AtomicBool>,
}

impl Stopper {
    /// Ask the listener thread to leave its loop. Idempotent and safe to call
    /// from any thread; a failed write only costs the loop one poll timeout.
    pub(crate) fn stop(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        // SAFETY: a one-byte write to a pipe fd this struct owns.
        unsafe {
            libc::write(self.write_fd.as_raw_fd(), [0u8].as_ptr().cast(), 1);
        }
    }
}

/// What the listener thread is doing with the edges it reads.
enum Mode {
    /// Push-to-talk: feed a `ChordTracker` and emit engage/disengage edges,
    /// unless the settings recorder has armed the tap.
    Ptt {
        tracker: ChordTracker,
        tx: Sender<PttEvent>,
        tap: Arc<std::sync::OnceLock<Arc<CaptureTap>>>,
    },
    /// Recording a shortcut: forward every chord-capable edge raw.
    Capture { tx: Sender<CaptureEvent> },
}

/// Clears the handle's liveness flag however the thread leaves, including an
/// unwind, so `ListenerHandle::is_alive` is never optimistic.
struct AliveGuard(Arc<AtomicBool>);

impl Drop for AliveGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

pub(crate) fn spawn_listener(
    chord: PttChord,
    swallow_locks: bool,
    tx: Sender<PttEvent>,
) -> Result<ListenerHandle, HotkeyError> {
    if swallow_locks {
        // Not silently ignored: a user who set this and still sees Caps Lock
        // flip deserves to find out why from the log rather than from
        // guessing. (Module docs: EVIOCGRAB is the only lever, and it takes
        // the whole device.)
        log::info!(
            "hark-hotkey: hotkey.swallow_locks has no effect on Linux; the kernel \
             offers no way to suppress one key without grabbing the whole device"
        );
    }
    for key in unmappable(&chord) {
        log::warn!(
            "hark-hotkey: the chord contains {key}, which has no evdev equivalent; \
             push-to-talk cannot see that key on Linux"
        );
    }
    let (capture_tx, capture_rx) = mpsc::channel();
    let shared: Arc<std::sync::OnceLock<Arc<CaptureTap>>> = Arc::new(std::sync::OnceLock::new());
    let mut handle = spawn_loop(
        "hark-hotkey",
        Mode::Ptt {
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

pub(crate) fn spawn_capture(tx: Sender<CaptureEvent>) -> Result<ListenerHandle, HotkeyError> {
    spawn_loop("hark-hotkey-capture", Mode::Capture { tx })
}

/// Open the devices, then read them until asked to stop. Runs as the entire
/// body of the dedicated listener thread.
fn spawn_loop(thread_name: &str, mode: Mode) -> Result<ListenerHandle, HotkeyError> {
    let devices = scan();
    if devices.is_empty() {
        return Err(if has_unreadable_nodes() {
            permission_error()
        } else {
            HotkeyError::Install("no keyboard found under /dev/input".to_string())
        });
    }
    log::info!(
        "{thread_name}: watching {} keyboard device(s) via evdev",
        devices.len()
    );

    let (read_fd, write_fd) = pipe()?;
    let stopping = Arc::new(AtomicBool::new(false));
    let alive = Arc::new(AtomicBool::new(true));

    let name = thread_name.to_string();
    let thread_alive = alive.clone();
    let thread_stopping = stopping.clone();
    let thread = std::thread::Builder::new()
        .name(thread_name.to_string())
        .spawn(move || {
            let _alive = AliveGuard(thread_alive);
            run(&name, mode, devices, read_fd, &thread_stopping);
            log::info!("{name}: evdev listener stopped");
        })
        .map_err(|e| HotkeyError::Install(format!("cannot spawn listener thread: {e}")))?;

    Ok(ListenerHandle {
        thread_id: 0,
        alive,
        stop: Some(Stopper { write_fd, stopping }),
        tap: None,
        capture_rx: None,
        thread: Some(thread),
    })
}

/// `pipe(2)` with both ends owned, for the shutdown wakeup.
fn pipe() -> Result<(std::os::fd::OwnedFd, std::os::fd::OwnedFd), HotkeyError> {
    use std::os::fd::FromRawFd;
    let mut fds = [0 as libc::c_int; 2];
    // SAFETY: `fds` is a fully initialized two-element array, which is exactly
    // what pipe(2) writes into.
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        let e = std::io::Error::last_os_error();
        return Err(HotkeyError::Install(format!(
            "cannot create the shutdown pipe: {e}"
        )));
    }
    // SAFETY: both descriptors were just produced by pipe(2) and are owned by
    // nobody else, so wrapping them transfers ownership exactly once.
    Ok(unsafe {
        (
            std::os::fd::OwnedFd::from_raw_fd(fds[0]),
            std::os::fd::OwnedFd::from_raw_fd(fds[1]),
        )
    })
}

/// The listener loop. Owns every device, so the tracker needs no lock.
fn run(
    name: &str,
    mut mode: Mode,
    mut devices: Vec<(PathBuf, Device)>,
    read_fd: std::os::fd::OwnedFd,
    stopping: &AtomicBool,
) {
    let mut last_scan = Instant::now();
    let mut engaged = false;

    while !stopping.load(Ordering::SeqCst) {
        // The stop pipe is always fd 0 of the set, so index 0 needs no lookup.
        let mut fds = Vec::with_capacity(devices.len() + 1);
        fds.push(libc::pollfd {
            fd: read_fd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        });
        for (_, device) in &devices {
            fds.push(libc::pollfd {
                fd: device.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            });
        }

        // A held chord is the only thing that needs sub-second attention.
        let timeout = if engaged { WATCHDOG_MS } else { RESCAN_MS };
        // SAFETY: `fds` is a live, correctly sized array of pollfd; every fd in
        // it is owned by this thread for the duration of the call.
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout) };
        if ready < 0 {
            let e = std::io::Error::last_os_error();
            if e.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            log::error!("{name}: poll failed ({e}); stopping the listener");
            return;
        }
        if stopping.load(Ordering::SeqCst) {
            return;
        }

        let mut disconnected = false;
        // Skip index 0: that is the stop pipe, and the flag check above is the
        // only thing that reads it.
        for (slot, (path, device)) in fds[1..].iter().zip(devices.iter_mut()) {
            if slot.revents == 0 {
                continue;
            }
            // POLLERR/POLLHUP is an unplugged keyboard. Leave it for the
            // rescan below to drop; reading it would only spin.
            if slot.revents & libc::POLLIN == 0 {
                continue;
            }
            let events = match device.fetch_events() {
                Ok(events) => events,
                // A non-blocking fd that poll reported readable can still come
                // up empty (another reader drained it, a spurious wakeup); that
                // is EAGAIN, not a fault, and logging it would spam.
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                Err(e) => {
                    log::warn!("{name}: {} read failed ({e})", path.display());
                    continue;
                }
            };
            for event in events {
                if event.event_type() != EventType::KEY {
                    continue;
                }
                // 0 = release, 1 = press, 2 = auto-repeat (module docs).
                let down = match event.value() {
                    0 => false,
                    1 => true,
                    _ => continue,
                };
                let Some(key) = evdev_to_key(KeyCode::new(event.code())) else {
                    continue;
                };
                if dispatch(&mut mode, key, down, &mut engaged) {
                    disconnected = true;
                }
            }
        }

        if engaged && watchdog(&mut mode, &devices) {
            engaged = false;
        }

        if disconnected {
            // Same contract as the Windows hook: no receiver, no reason to
            // keep reading keys for the rest of the session.
            log::warn!("{name}: listener receiver is gone; stopping");
            return;
        }

        if last_scan.elapsed() >= Duration::from_millis(RESCAN_MS as u64) {
            last_scan = Instant::now();
            rescan(name, &mut devices);
        }
    }
}

/// Feed one edge to whatever the listener is doing. Returns true when the
/// receiving end has gone away.
fn dispatch(mode: &mut Mode, key: PttKeyCode, down: bool, engaged: &mut bool) -> bool {
    match mode {
        Mode::Ptt { tap, .. } if tap.get().is_some_and(|t| t.forward(key, down)) => {
            // The settings recorder consumed it, so the tracker never sees it
            // and the chord being recorded cannot also fire a dictation.
            false
        }
        Mode::Ptt { tracker, tx, .. } => {
            // evdev reports edges only for keys that really changed, so unlike
            // the Windows hook there is nothing to re-verify against a second
            // source: the kernel IS the source of truth here.
            match tracker.on_event(key, down, false) {
                Some(event) => {
                    *engaged = event == PttEvent::Down;
                    tx.send(event).is_err()
                }
                None => false,
            }
        }
        Mode::Capture { tx } => tx.send(CaptureEvent { key, down }).is_err(),
    }
}

/// One watchdog poll: if the chord the tracker believes is held is no longer
/// physically down, its release never arrived -- emit it so the recording ends
/// instead of running forever. Returns true when it healed one.
fn watchdog(mode: &mut Mode, devices: &[(PathBuf, Device)]) -> bool {
    let Mode::Ptt { tracker, tx, .. } = mode else {
        return false;
    };
    let Some(event) = tracker.resync_released(|key| physically_down(devices, key)) else {
        return false;
    };
    log::warn!("push-to-talk release never arrived; ending the recording");
    let _ = tx.send(event);
    true
}

/// Is `key` physically held on any attached keyboard? Asks the kernel for each
/// device's key state rather than trusting an accumulated view, so a release
/// lost to a VT switch or a suspend heals on the next tick.
///
/// A device that errors is treated as not holding the key: the watchdog's job
/// is to end recordings that should have ended, and an unreadable keyboard is
/// not evidence that a key is still down.
fn physically_down(devices: &[(PathBuf, Device)], key: PttKeyCode) -> bool {
    let Some(code) = key_to_evdev(key) else {
        return false;
    };
    devices
        .iter()
        .any(|(_, device)| device.get_key_state().is_ok_and(|s| s.contains(code)))
}

/// Pick up keyboards plugged in since the last pass and drop the ones that
/// vanished. Compares by device node path; a re-plugged keyboard usually gets
/// a fresh `eventN`, so it arrives as an addition rather than a stale fd.
fn rescan(name: &str, devices: &mut Vec<(PathBuf, Device)>) {
    let found = scan();
    if found.len() == devices.len()
        && found
            .iter()
            .all(|(path, _)| devices.iter().any(|(known, _)| known == path))
    {
        return;
    }
    log::info!(
        "{name}: keyboard set changed ({} -> {})",
        devices.len(),
        found.len()
    );
    *devices = found;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn evdev_mapping_covers_the_default_chord() {
        assert_eq!(
            evdev_to_key(KeyCode::KEY_LEFTCTRL),
            Some(PttKeyCode::LCtrl),
            "the default chord's Ctrl must map"
        );
        assert_eq!(
            evdev_to_key(KeyCode::KEY_LEFTMETA),
            Some(PttKeyCode::LWin),
            "evdev calls the Windows/Super key META"
        );
        assert_eq!(evdev_to_key(KeyCode::KEY_RIGHTMETA), Some(PttKeyCode::RWin));
        assert_eq!(evdev_to_key(KeyCode::KEY_COMPOSE), Some(PttKeyCode::Apps));
        assert_eq!(evdev_to_key(KeyCode::KEY_102ND), Some(PttKeyCode::Oem102));
    }

    #[test]
    fn evdev_mapping_round_trips_for_every_mappable_key() {
        // The watchdog resolves every chord member through key_to_evdev, so a
        // mismatch anywhere invents a press or loses a release for that key.
        for key in ALL_KEYS {
            let Some(code) = key_to_evdev(key) else {
                continue;
            };
            assert_eq!(evdev_to_key(code), Some(key), "{key}");
        }
    }

    #[test]
    fn exactly_one_key_has_no_evdev_counterpart() {
        // Pinned deliberately: if a future edit maps Oem8 to some neighbouring
        // code, or drops another key's mapping by accident, this fails rather
        // than letting push-to-talk go quietly deaf on that key.
        let unmapped: Vec<_> = ALL_KEYS
            .into_iter()
            .filter(|k| key_to_evdev(*k).is_none())
            .collect();
        assert_eq!(unmapped, vec![PttKeyCode::Oem8]);
    }

    #[test]
    fn evdev_codes_are_unique_across_mappable_keys() {
        // Two keys sharing a code would make the watchdog read the wrong key's
        // state, so a chord containing either would heal at the wrong moment.
        let mut seen = HashSet::new();
        for key in ALL_KEYS {
            if let Some(code) = key_to_evdev(key) {
                assert!(seen.insert(code.0), "{key} reuses an evdev code");
            }
        }
    }

    #[test]
    fn numpad_enter_folds_into_enter() {
        // The enum has one Enter by design (keycode.rs: one variant per
        // virtual key, never finer), so both physical keys must reach it --
        // otherwise a chord bound to Enter would ignore the numpad one.
        assert_eq!(evdev_to_key(KeyCode::KEY_KPENTER), Some(PttKeyCode::Enter));
        assert_eq!(evdev_to_key(KeyCode::KEY_ENTER), Some(PttKeyCode::Enter));
        assert_eq!(key_to_evdev(PttKeyCode::Enter), Some(KeyCode::KEY_ENTER));
    }

    #[test]
    fn evdev_mapping_ignores_what_is_not_a_chord_key() {
        // Same exclusions the Windows table makes, for the same reasons.
        assert_eq!(evdev_to_key(KeyCode::KEY_ESC), None); // cancels a recording
        assert_eq!(evdev_to_key(KeyCode::KEY_SYSRQ), None); // PrintScreen
        assert_eq!(evdev_to_key(KeyCode::KEY_PAUSE), None);
        assert_eq!(evdev_to_key(KeyCode::KEY_MUTE), None); // and the media keys
        assert_eq!(evdev_to_key(KeyCode::BTN_LEFT), None); // a mouse button
    }

    #[test]
    fn unmappable_reports_only_the_chord_keys_that_are_missing() {
        let ok = PttChord::from_keys(vec![PttKeyCode::LCtrl, PttKeyCode::LWin]);
        assert!(unmappable(&ok).is_empty());

        let carried_from_windows = PttChord::from_keys(vec![PttKeyCode::LCtrl, PttKeyCode::Oem8]);
        assert_eq!(unmappable(&carried_from_windows), vec![PttKeyCode::Oem8]);
    }

    #[test]
    fn our_own_virtual_keyboard_is_filtered_out() {
        // The paste device hark-inject creates is named "Hark Virtual
        // Keyboard". Reading it back would feed our own synthesized Ctrl+V
        // into the tracker, which on a Ctrl-containing chord is a dictation
        // loop that never stops.
        assert!("Hark Virtual Keyboard".starts_with(OWN_DEVICE_PREFIX));
    }
}

//! A listen-only Quartz event tap on its own Core Foundation run loop.
//! No AppKit work occurs here: the app's main thread owns all UI.
use crate::mac_ffi::*;
use crate::mac_keycode::{from_code, modifier_down, to_code};
use crate::shortcuts::ShortcutTracker;
use crate::{
    CaptureEvent, CaptureTap, HotkeyError, ListenerHandle, PttChord, PttEvent, PttKeyCode,
    ShortcutEvent,
};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Instant;

const KEY_DOWN: u32 = 10;
const KEY_UP: u32 = 11;
const FLAGS_CHANGED: u32 = 12;
const DISABLED_TIMEOUT: u32 = 0xffff_fffe;
const DISABLED_USER: u32 = 0xffff_ffff;
const KEYCODE: u32 = 9;
const SOURCE_PID: u32 = 41;

fn physically_down(key: PttKeyCode) -> bool {
    // HIDSystemState (1) avoids the private state of synthesized input.
    // The two Enter positions share one portable binding.
    unsafe {
        to_code(key).is_some_and(|code| CGEventSourceKeyState(1, code))
            || (key == PttKeyCode::Enter && CGEventSourceKeyState(1, 0x4c))
    }
}

enum Sink {
    Dictation(mpsc::Sender<PttEvent>),
    Shared(mpsc::Sender<ShortcutEvent>),
    Capture(mpsc::Sender<CaptureEvent>),
}

impl Sink {
    fn send(&self, event: ShortcutEvent) -> bool {
        match (self, event) {
            (Self::Dictation(tx), ShortcutEvent::Dictation(event)) => tx.send(event).is_ok(),
            (Self::Shared(tx), event) => tx.send(event).is_ok(),
            _ => true,
        }
    }
}

struct State {
    tracker: Option<ShortcutTracker>,
    sink: Sink,
    tap: Option<Arc<CaptureTap>>,
    capturing: bool,
    disconnected: bool,
    port: Ref,
}

impl State {
    /// A disabled tap or capture-mode transition invalidates previous holds.
    /// Only releases are manufactured; another physical press is required.
    fn release_all(&mut self) {
        if let Some(tracker) = &mut self.tracker {
            for key in crate::ALL_KEYS {
                for event in tracker
                    .on_event(key, false, false, |_| false, Instant::now())
                    .into_iter()
                    .flatten()
                {
                    self.disconnected |= !self.sink.send(event);
                }
            }
        }
    }

    fn sync_capture(&mut self) {
        let capturing = self.tap.as_ref().is_some_and(|tap| tap.armed());
        if capturing != self.capturing {
            self.release_all();
            self.capturing = capturing;
        }
    }

    fn edge(&mut self, key: PttKeyCode, down: bool) {
        self.sync_capture();
        if self.tap.as_ref().is_some_and(|tap| tap.forward(key, down)) {
            return;
        }
        if let Sink::Capture(tx) = &self.sink {
            self.disconnected |= tx.send(CaptureEvent { key, down }).is_err();
        } else if let Some(tracker) = &mut self.tracker {
            for event in tracker
                .on_event(key, down, false, physically_down, Instant::now())
                .into_iter()
                .flatten()
            {
                self.disconnected |= !self.sink.send(event);
            }
        }
    }

    fn watchdog(&mut self) {
        self.sync_capture();
        if let Some(tracker) = &mut self.tracker {
            if tracker.engaged() {
                if let Some(event) = tracker.resync(physically_down, Instant::now()) {
                    self.disconnected |= !self.sink.send(event);
                }
            }
        }
    }
}

unsafe extern "C" fn callback(_proxy: Ref, kind: u32, event: Ref, info: Ref) -> Ref {
    // SAFETY: State is boxed on the installing thread and lives until the
    // source is removed and port invalidated. Only this run loop accesses it.
    let state = unsafe { &mut *info.cast::<State>() };
    if kind == DISABLED_TIMEOUT || kind == DISABLED_USER {
        state.release_all();
        unsafe { CGEventTapEnable(state.port, true) };
        return event;
    }
    if event.is_null() || !matches!(kind, KEY_DOWN | KEY_UP | FLAGS_CHANGED) {
        return event;
    }
    // Quartz tags posted events with their source PID. Ignore all software
    // producers, including enigo's Cmd+V, so dictation cannot trigger itself.
    // Hardware events originate in the kernel (PID 0).
    if unsafe { CGEventGetIntegerValueField(event, SOURCE_PID) } != 0 {
        return event;
    }
    let code = unsafe { CGEventGetIntegerValueField(event, KEYCODE) };
    if let Ok(code) = u16::try_from(code) {
        if let Some(key) = from_code(code) {
            let down = match kind {
                KEY_DOWN => Some(true),
                KEY_UP => Some(false),
                FLAGS_CHANGED => modifier_down(key, unsafe { CGEventGetFlags(event) }),
                _ => None,
            };
            if let Some(down) = down {
                state.edge(key, down);
            }
        }
    }
    event
}

fn validate_chord(chord: &PttChord) -> Result<(), HotkeyError> {
    if let Some(key) = chord.keys().iter().find(|key| to_code(**key).is_none()) {
        return Err(HotkeyError::Install(format!(
            "{} cannot be used as a held shortcut on macOS. Choose Control + Command, or a supported function key (F1–F20). Caps Lock exposes a toggle rather than reliable press/release edges; Windows-only keys have no Quartz mapping.", key.label()
        )));
    }
    Ok(())
}

pub(crate) fn spawn_listener(
    chord: PttChord,
    swallow_locks: bool,
    tx: mpsc::Sender<PttEvent>,
) -> Result<ListenerHandle, HotkeyError> {
    spawn_routed(chord, swallow_locks, None, Sink::Dictation(tx))
}

pub(crate) fn spawn_shared_listener(
    chord: PttChord,
    swallow_locks: bool,
    meeting: Option<PttChord>,
    tx: mpsc::Sender<ShortcutEvent>,
) -> Result<ListenerHandle, HotkeyError> {
    spawn_routed(chord, swallow_locks, meeting, Sink::Shared(tx))
}

fn spawn_routed(
    chord: PttChord,
    swallow_locks: bool,
    meeting: Option<PttChord>,
    sink: Sink,
) -> Result<ListenerHandle, HotkeyError> {
    validate_chord(&chord)?;
    if let Some(meeting) = &meeting {
        validate_chord(meeting)?;
    }
    if swallow_locks {
        log::debug!("macOS observes shortcuts without changing system lock-key behavior");
    }
    let alive = Arc::new(AtomicBool::new(true));
    let (capture_tx, capture_rx) = mpsc::channel();
    let tap = Arc::new(CaptureTap::new(capture_tx, alive.clone()));
    let mut handle = spawn_hook(
        Some(ShortcutTracker::new(chord, false, meeting)),
        sink,
        Some(tap.clone()),
        alive,
    )?;
    handle.tap = Some(tap);
    handle.capture_rx = Some(capture_rx);
    Ok(handle)
}

pub(crate) fn spawn_capture(tx: mpsc::Sender<CaptureEvent>) -> Result<ListenerHandle, HotkeyError> {
    spawn_hook(
        None,
        Sink::Capture(tx),
        None,
        Arc::new(AtomicBool::new(true)),
    )
}

struct AliveGuard(Arc<AtomicBool>);
impl Drop for AliveGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// The run loop and native resources never cross a thread boundary. The only
/// cross-thread shutdown state is an atomic, checked at most 100 ms later.
struct NativeTap {
    port: Ref,
    source: Ref,
    run_loop: Ref,
}
impl Drop for NativeTap {
    fn drop(&mut self) {
        unsafe {
            CFRunLoopRemoveSource(self.run_loop, self.source, kCFRunLoopDefaultMode);
            CFMachPortInvalidate(self.port);
            CFRelease(self.source);
            CFRelease(self.port);
        }
    }
}

fn spawn_hook(
    tracker: Option<ShortcutTracker>,
    sink: Sink,
    tap: Option<Arc<CaptureTap>>,
    alive: Arc<AtomicBool>,
) -> Result<ListenerHandle, HotkeyError> {
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    let thread_alive = alive.clone();
    let thread = std::thread::Builder::new()
        .name("hark-hotkey".into())
        .spawn(move || {
            let _alive = AliveGuard(thread_alive);
            let mut state = Box::new(State {
                tracker,
                sink,
                tap,
                capturing: false,
                disconnected: false,
                port: null_mut(),
            });
            run_hook(&mut state, &thread_stop, ready_tx);
        })
        .map_err(|e| HotkeyError::Install(format!("cannot spawn keyboard listener: {e}")))?;
    match ready_rx.recv() {
        Ok(Ok(())) => Ok(ListenerHandle {
            thread_id: 0,
            alive,
            stop: Some(stop),
            tap: None,
            capture_rx: None,
            thread: Some(thread),
        }),
        Ok(Err(error)) => {
            let _ = thread.join();
            Err(error)
        }
        Err(_) => {
            let _ = thread.join();
            Err(HotkeyError::Install(
                "keyboard listener exited during setup".into(),
            ))
        }
    }
}

fn run_hook(
    state: &mut State,
    stop: &AtomicBool,
    ready: mpsc::SyncSender<Result<(), HotkeyError>>,
) {
    // Session tap (1), head placement (0), listen-only (1): no event
    // suppression, no root requirement, and no interaction with AppKit.
    let port = unsafe {
        CGEventTapCreate(
            1,
            0,
            1,
            (1 << KEY_DOWN) | (1 << KEY_UP) | (1 << FLAGS_CHANGED),
            callback,
            (state as *mut State).cast(),
        )
    };
    if port.is_null() {
        let _ = ready.send(Err(HotkeyError::Install(
            "macOS denied keyboard monitoring. Enable Hark in System Settings → Privacy & Security → Input Monitoring and Accessibility, then quit and reopen Hark.".into(),
        )));
        return;
    }
    state.port = port;
    let source = unsafe { CFMachPortCreateRunLoopSource(null_mut(), port, 0) };
    if source.is_null() {
        unsafe {
            CFMachPortInvalidate(port);
            CFRelease(port);
        }
        let _ = ready.send(Err(HotkeyError::Install(
            "cannot create the macOS keyboard run-loop source".into(),
        )));
        return;
    }
    let native = NativeTap {
        port,
        source,
        run_loop: unsafe { CFRunLoopGetCurrent() },
    };
    unsafe {
        CFRunLoopAddSource(native.run_loop, source, kCFRunLoopDefaultMode);
    }
    if ready.send(Ok(())).is_err() {
        return;
    }
    while !stop.load(Ordering::Acquire) && !state.disconnected {
        // Pump continuously; the bounded interval also runs release recovery
        // during secure input, sleep/resume and lost key-up. A removed source
        // must exit instead of spinning on kCFRunLoopRunFinished.
        let result = unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.1, 0) };
        if matches!(result, 1 | 2) {
            break;
        }
        state.watchdog();
    }
    state.release_all();
    // NativeTap drops before State, so callbacks cannot outlive userInfo.
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_unsupported_chords_before_requesting_permissions() {
        assert!(validate_chord(&PttChord::parse("LCtrl+LWin").unwrap()).is_ok());
        assert!(validate_chord(&PttChord::parse("F20").unwrap()).is_ok());
        assert!(validate_chord(&PttChord::parse("F24").unwrap()).is_err());
        assert!(validate_chord(&PttChord::parse("CapsLock").unwrap()).is_err());
    }
    fn test_state() -> (
        State,
        mpsc::Receiver<ShortcutEvent>,
        mpsc::Receiver<CaptureEvent>,
    ) {
        let (tx, rx) = mpsc::channel();
        let (capture_tx, capture_rx) = mpsc::channel();
        let tap = Arc::new(CaptureTap::new(capture_tx, Arc::new(AtomicBool::new(true))));
        (
            State {
                tracker: Some(ShortcutTracker::new(
                    PttChord::parse("F12").unwrap(),
                    false,
                    Some(PttChord::parse("F13").unwrap()),
                )),
                sink: Sink::Shared(tx),
                tap: Some(tap),
                capturing: false,
                disconnected: false,
                port: null_mut(),
            },
            rx,
            capture_rx,
        )
    }

    #[test]
    fn capture_transition_ends_hold_and_bypasses_both_shortcuts() {
        let (mut state, rx, capture_rx) = test_state();
        state.edge(PttKeyCode::F12, true);
        assert_eq!(
            rx.try_recv().unwrap(),
            ShortcutEvent::Dictation(PttEvent::Down)
        );
        state
            .tap
            .as_ref()
            .unwrap()
            .on
            .store(true, Ordering::Relaxed);
        state.edge(PttKeyCode::F13, true);
        assert_eq!(
            rx.try_recv().unwrap(),
            ShortcutEvent::Dictation(PttEvent::Up)
        );
        assert!(rx.try_recv().is_err());
        assert_eq!(
            capture_rx.try_recv().unwrap(),
            CaptureEvent {
                key: PttKeyCode::F13,
                down: true
            }
        );
        state
            .tap
            .as_ref()
            .unwrap()
            .on
            .store(false, Ordering::Relaxed);
        state.edge(PttKeyCode::F13, false);
        assert!(rx.try_recv().is_err());
        state.edge(PttKeyCode::F13, true);
        assert_eq!(rx.try_recv().unwrap(), ShortcutEvent::MeetingToggle);
    }

    #[test]
    fn clearing_lost_holds_does_not_manufacture_meeting_toggles() {
        let (mut state, rx, _capture_rx) = test_state();
        state.edge(PttKeyCode::F12, true);
        state.edge(PttKeyCode::F13, true);
        assert_eq!(
            rx.try_recv().unwrap(),
            ShortcutEvent::Dictation(PttEvent::Down)
        );
        assert_eq!(rx.try_recv().unwrap(), ShortcutEvent::MeetingToggle);
        state.release_all();
        assert_eq!(
            rx.try_recv().unwrap(),
            ShortcutEvent::Dictation(PttEvent::Up)
        );
        assert!(rx.try_recv().is_err());
        state.edge(PttKeyCode::F13, true);
        assert_eq!(rx.try_recv().unwrap(), ShortcutEvent::MeetingToggle);
    }
}

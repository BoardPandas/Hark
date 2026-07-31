//! Hark push-to-talk source. A low-level key hook on a dedicated
//! message-loop thread emits clean chord edges (default: hold Left Ctrl +
//! Left Win to record, release either to stop).
//!
//! The `spawn_listener` boundary is the platform seam: `hook_win.rs`
//! (WH_KEYBOARD_LL) implements it now; `hook_mac.rs` (CGEventTap) slots in
//! behind the same signature in checkpoint 7 without touching the pipeline.

pub mod capture;
pub mod edges;

#[cfg(windows)]
mod hook_win;

pub use capture::{CaptureBuffer, CaptureEvent, Rejected};
pub use edges::{pretty_chord, ChordParseError, ChordTracker, PttChord, PttEvent, PttKeyCode};

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HotkeyError {
    #[error(transparent)]
    Chord(#[from] ChordParseError),
    #[error("cannot install the keyboard hook: {0}")]
    Install(String),
    #[error("push-to-talk is not implemented for this platform yet")]
    UnsupportedPlatform,
}

/// A tap on the listener's raw key stream, for the settings recorder.
///
/// The recorder used to install a *second* `WH_KEYBOARD_LL` hook of its own.
/// On real hardware that hook installs, reports itself alive, pumps messages,
/// and is never called — while the push-to-talk hook, same code, same thread
/// shape, delivers every key. Rather than keep guessing at why, capture now
/// rides the hook that demonstrably works: one relaxed atomic load per key
/// event decides whether the callback feeds the chord tracker or forwards the
/// raw edge here.
pub struct CaptureTap {
    /// Set while the settings UI is recording. Relaxed on both ends: the
    /// worst a stale read can do is forward one edge either side of the flip.
    on: AtomicBool,
    /// Every edge the hook has forwarded since the tap was armed. The UI shows
    /// it, so "the hook never saw your keys" and "the hook saw them and the UI
    /// lost them" are distinguishable on sight instead of by another release.
    seen: AtomicU64,
    /// Only the platform hook sends on this; off Windows there is no hook yet.
    #[cfg_attr(not(windows), allow(dead_code))]
    tx: Sender<CaptureEvent>,
}

#[cfg_attr(not(windows), allow(dead_code))]
impl CaptureTap {
    pub(crate) fn new(tx: Sender<CaptureEvent>) -> CaptureTap {
        CaptureTap {
            on: AtomicBool::new(false),
            seen: AtomicU64::new(0),
            tx,
        }
    }

    /// Called from the hook callback. Must stay lean: two relaxed atomics and
    /// a channel send, no allocation, no I/O, no locks (Windows silently
    /// removes low-level hooks whose callback overruns `LowLevelHooksTimeout`).
    /// Returns true when the edge was consumed, so the caller skips the tracker
    /// and a chord pressed while recording cannot start a dictation.
    pub(crate) fn forward(&self, key: PttKeyCode, down: bool) -> bool {
        if !self.on.load(Ordering::Relaxed) {
            return false;
        }
        self.seen.fetch_add(1, Ordering::Relaxed);
        let _ = self.tx.send(CaptureEvent { key, down });
        true
    }

    pub fn edges_seen(&self) -> u64 {
        self.seen.load(Ordering::Relaxed)
    }
}

/// A running push-to-talk listener. Dropping it stops the hook thread.
pub struct ListenerHandle {
    #[cfg_attr(not(windows), allow(dead_code))]
    thread_id: u32,
    /// Cleared by the hook thread as its last act. Two things depend on it:
    /// callers can tell a hook that died on its own from one that is still
    /// listening, and teardown skips posting to a thread id the OS may already
    /// have recycled onto somebody else's thread.
    alive: Arc<AtomicBool>,
    /// Shared with the hook thread; `None` for a capture-only hook, which has
    /// nothing to tap.
    tap: Option<Arc<CaptureTap>>,
    /// The tap's receiving end, handed to the recorder on the first arm.
    capture_rx: Option<Receiver<CaptureEvent>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl ListenerHandle {
    /// Is the hook still installed? False once the hook thread has exited on
    /// its own — the receiver went away, or the install was torn down by the
    /// OS. A recording UI that ignores this shows "press your keys..." forever
    /// against a hook that is no longer listening.
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }

    /// Start forwarding raw key edges instead of feeding the chord tracker.
    /// The receiver comes back on the first call and is the caller's from then
    /// on; later calls re-arm the same channel. `None` on a platform with no
    /// hook, or if the receiver was already taken and dropped.
    pub fn arm_capture(&mut self) -> Option<(Arc<CaptureTap>, Receiver<CaptureEvent>)> {
        let tap = self.tap.clone()?;
        let rx = self.capture_rx.take()?;
        // Stale edges from a previous arm would be attributed to this one, and
        // a leftover release would complete a "chord" the user never pressed.
        while rx.try_recv().is_ok() {}
        tap.seen.store(0, Ordering::Relaxed);
        tap.on.store(true, Ordering::Relaxed);
        Some((tap, rx))
    }

    /// Stop forwarding; the chord tracker takes the stream back. Giving the
    /// receiver back keeps the handle re-armable for the next recording.
    pub fn disarm_capture(&mut self, rx: Receiver<CaptureEvent>) {
        if let Some(tap) = &self.tap {
            tap.on.store(false, Ordering::Relaxed);
        }
        self.capture_rx = Some(rx);
    }
}

impl Drop for ListenerHandle {
    fn drop(&mut self) {
        #[cfg(windows)]
        if self.alive.load(Ordering::Acquire) {
            hook_win::stop_listener(self.thread_id);
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Can this platform record a shortcut by watching real key edges? False
/// wherever [`spawn_capture`] would fail, so the settings UI can offer the
/// typed fallback instead of a button that only ever reports an error.
pub fn capture_supported() -> bool {
    cfg!(windows)
}

/// Start listening for the chord; edges arrive on `tx`. One listener per
/// process. Blocks until the hook is installed (or failed).
pub fn spawn_listener(
    chord: PttChord,
    tx: Sender<PttEvent>,
) -> Result<ListenerHandle, HotkeyError> {
    #[cfg(windows)]
    {
        hook_win::spawn_listener(chord, tx)
    }
    #[cfg(not(windows))]
    {
        // CGEventTap arrives in checkpoint 7 (NEEDS MAC).
        let _ = (chord, tx);
        Err(HotkeyError::UnsupportedPlatform)
    }
}

/// Start recording a shortcut: every chord-capable key press/release arrives
/// on `tx` (injected events filtered) so the settings UI can build a chord
/// with `CaptureBuffer`. Same install/teardown contract as `spawn_listener`;
/// dropping the handle stops the hook. Only one hook should run at a time, so
/// the caller pauses the push-to-talk listener while recording.
pub fn spawn_capture(tx: Sender<CaptureEvent>) -> Result<ListenerHandle, HotkeyError> {
    #[cfg(windows)]
    {
        hook_win::spawn_capture(tx)
    }
    #[cfg(not(windows))]
    {
        // Recording rides the same platform hook, so it lands with the
        // CGEventTap in checkpoint 7 (NEEDS MAC).
        let _ = tx;
        Err(HotkeyError::UnsupportedPlatform)
    }
}

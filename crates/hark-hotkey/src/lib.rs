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

pub use capture::{CaptureBuffer, CaptureEvent, HeldScan, Rejected, CHORD_KEYS};
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
    /// Edges the hook callback forwarded, and edges the physical-state scanner
    /// supplied, counted apart. Together they answer the question a single
    /// total cannot: whether the hook is telling us everything the keyboard is
    /// actually doing. A recording that only completes because the scanner
    /// filled gaps is a hook that is dropping events, visible on sight.
    hook_edges: AtomicU64,
    polled_edges: AtomicU64,
    /// The listener thread's liveness, shared so a recorder can tell a hook
    /// that died from one that is merely quiet.
    alive: Arc<AtomicBool>,
    tx: Sender<CaptureEvent>,
}

/// How many key edges each source has contributed to the recording in progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureCounts {
    pub hook: u64,
    pub polled: u64,
}

#[cfg_attr(not(windows), allow(dead_code))]
impl CaptureTap {
    pub(crate) fn new(tx: Sender<CaptureEvent>, alive: Arc<AtomicBool>) -> CaptureTap {
        CaptureTap {
            on: AtomicBool::new(false),
            hook_edges: AtomicU64::new(0),
            polled_edges: AtomicU64::new(0),
            alive,
            tx,
        }
    }

    /// Called from the hook callback. Must stay lean: a relaxed load, a relaxed
    /// add and a channel send — no allocation, no I/O, no locks (Windows
    /// silently removes low-level hooks whose callback overruns
    /// `LowLevelHooksTimeout`).
    ///
    /// Returns true when the edge was consumed, so the caller skips the tracker
    /// and a chord pressed while recording cannot start a dictation. A send
    /// that fails disarms the tap and returns false: the receiver is gone, and
    /// a tap left armed against a dead channel would swallow every chord-capable
    /// key for the rest of the session — push-to-talk silently dead forever.
    pub(crate) fn forward(&self, key: PttKeyCode, down: bool) -> bool {
        if !self.on.load(Ordering::Relaxed) {
            return false;
        }
        self.hook_edges.fetch_add(1, Ordering::Relaxed);
        if self.tx.send(CaptureEvent { key, down }).is_err() {
            self.on.store(false, Ordering::Relaxed);
            return false;
        }
        true
    }

    /// Called from the scanner thread, never from the hook callback.
    pub(crate) fn emit_polled(&self, event: CaptureEvent) {
        self.polled_edges.fetch_add(1, Ordering::Relaxed);
        let _ = self.tx.send(event);
    }

    pub(crate) fn armed(&self) -> bool {
        self.on.load(Ordering::Relaxed)
    }

    pub fn counts(&self) -> CaptureCounts {
        CaptureCounts {
            hook: self.hook_edges.load(Ordering::Relaxed),
            polled: self.polled_edges.load(Ordering::Relaxed),
        }
    }

    /// Total edges seen, for the "N key events seen" readout.
    pub fn edges_seen(&self) -> u64 {
        let c = self.counts();
        c.hook + c.polled
    }

    /// Is the hook that feeds this tap still installed?
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
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

    /// Start forwarding raw key edges to the settings recorder instead of the
    /// chord tracker, and start the physical-state scanner that backs it up.
    ///
    /// The receiver deliberately never leaves this handle. It used to be taken
    /// on arm and handed back on disarm, which meant a pipeline restart
    /// mid-recording stored the old, disconnected receiver into the *new*
    /// listener — after which the edge counter climbed forever and nothing ever
    /// recorded, with `try_recv` unable to tell Empty from Disconnected.
    pub fn arm_capture(&self) -> Option<Arc<CaptureTap>> {
        let tap = self.tap.clone()?;
        let rx = self.capture_rx.as_ref()?;
        // Stale edges from a previous arm would be attributed to this one, and a
        // leftover release would complete a "chord" the user never pressed.
        while rx.try_recv().is_ok() {}
        tap.hook_edges.store(0, Ordering::Relaxed);
        tap.polled_edges.store(0, Ordering::Relaxed);
        tap.on.store(true, Ordering::Relaxed);
        #[cfg(windows)]
        hook_win::spawn_scanner(tap.clone());
        Some(tap)
    }

    /// Stop forwarding; the chord tracker takes the stream back and the scanner
    /// thread retires on its next tick.
    pub fn disarm_capture(&self) {
        if let Some(tap) = &self.tap {
            tap.on.store(false, Ordering::Relaxed);
        }
    }

    /// Drain whatever either source has produced since the last call.
    pub fn drain_capture(&self, mut f: impl FnMut(CaptureEvent)) {
        let Some(rx) = self.capture_rx.as_ref() else {
            return;
        };
        while let Ok(edge) = rx.try_recv() {
            f(edge);
        }
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

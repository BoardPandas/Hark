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

pub use capture::{CaptureBuffer, CaptureEvent};
pub use edges::{pretty_chord, ChordParseError, ChordTracker, PttChord, PttEvent, PttKeyCode};

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
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

/// A running push-to-talk listener. Dropping it stops the hook thread.
pub struct ListenerHandle {
    #[cfg_attr(not(windows), allow(dead_code))]
    thread_id: u32,
    /// Cleared by the hook thread as its last act. Two things depend on it:
    /// callers can tell a hook that died on its own from one that is still
    /// listening, and teardown skips posting to a thread id the OS may already
    /// have recycled onto somebody else's thread.
    alive: Arc<AtomicBool>,
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

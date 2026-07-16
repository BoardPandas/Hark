//! Hark push-to-talk source. A low-level key hook on a dedicated
//! message-loop thread emits clean chord edges (default: hold Left Ctrl +
//! Left Win to record, release either to stop).
//!
//! The `spawn_listener` boundary is the platform seam: `hook_win.rs`
//! (WH_KEYBOARD_LL) implements it now; `hook_mac.rs` (CGEventTap) slots in
//! behind the same signature in checkpoint 7 without touching the pipeline.

pub mod edges;

#[cfg(windows)]
mod hook_win;

pub use edges::{ChordParseError, ChordTracker, PttChord, PttEvent, PttKeyCode};

use std::sync::mpsc::Sender;
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
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for ListenerHandle {
    fn drop(&mut self) {
        #[cfg(windows)]
        hook_win::stop_listener(self.thread_id);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
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

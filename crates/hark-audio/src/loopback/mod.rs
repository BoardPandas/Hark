//! System-audio loopback for meeting mode: the "Them" channel. The contract
//! (types, handle, start) lives here; `win.rs` (WASAPI process loopback),
//! `linux.rs` (PipeWire), and `core_audio_mac` (the macOS process tap)
//! implement it.
//!
//! Windows and Linux deliver 16 kHz mono into the same f32
//! [`ring`](crate::ring) the microphone uses, converted by the audio server
//! itself (Windows: `AUTOCONVERTPCM`; PipeWire: the stream's audioconvert
//! adapter), and both deliver audio continuously through silence, so the
//! channel's timeline is simply its sample count. macOS delivers the tap
//! device's own rate and the recorder resamples. Each implementation is glue
//! on one dedicated thread whose only per-packet work is a stack-buffer copy
//! and `Producer::push`; see the platform module for how targeting differs.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod win;

use crate::ring::Consumer;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use thiserror::Error;

/// The rate the loopback delivers: the audio server resamples to it.
pub const LOOPBACK_RATE: u32 = crate::TARGET_RATE;

/// Whose audio to capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopbackTarget {
    /// Only this process and its descendants: a detected meeting app, by root
    /// PID (the tree covers WebView2 and browser audio child processes).
    IncludeTree(u32),
    /// Everything except this process and its descendants: a manual start,
    /// excluding Hark's own PID so its sounds are never transcribed.
    ExcludeTree(u32),
}

#[derive(Debug, Error)]
pub enum LoopbackError {
    #[error("system-audio capture is not implemented on this platform")]
    UnsupportedPlatform,
    /// Includes Windows builds before 19041, which have no process loopback:
    /// the caller's cue to fall back to endpoint loopback.
    #[error("cannot activate process loopback: {0}")]
    Activate(String),
    #[error("cannot start process loopback: {0}")]
    Start(String),
    #[error("loopback thread exited before reporting a stream")]
    ThreadDied,
}

/// A running loopback capture. The ring `Consumer` is handed out at start and
/// moves to the meeting drain; this handle keeps the stream alive, and
/// dropping it stops the stream and joins its thread.
pub struct LoopbackHandle {
    /// macOS delivers the tap device's rate rather than 16 kHz.
    #[cfg(target_os = "macos")]
    pub(crate) sample_rate: u32,
    pub(crate) shutdown: Arc<AtomicBool>,
    pub(crate) stream_error: Arc<AtomicBool>,
    pub(crate) discontinuities: Arc<AtomicU64>,
    pub(crate) start_qpc_ns: Arc<AtomicU64>,
    pub(crate) thread: Option<std::thread::JoinHandle<()>>,
}

impl LoopbackHandle {
    /// Windows and Linux deliver [`LOOPBACK_RATE`]; macOS the tap device rate.
    pub fn sample_rate(&self) -> u32 {
        #[cfg(target_os = "macos")]
        {
            self.sample_rate
        }
        #[cfg(not(target_os = "macos"))]
        {
            LOOPBACK_RATE
        }
    }

    /// True once capture has stopped for good (the stream reported an error
    /// and its thread exited). Latching.
    pub fn stream_errored(&self) -> bool {
        self.stream_error.load(Ordering::Relaxed)
    }

    /// Packets the platform flagged as following a gap (audio lost before
    /// Hark saw it). One at startup is normal. Monotonic.
    pub fn discontinuities(&self) -> Arc<AtomicU64> {
        self.discontinuities.clone()
    }

    /// When ring sample 0 was captured, in nanoseconds on the platform's
    /// monotonic clock (QPC on Windows, `CLOCK_MONOTONIC` on Linux), or `None`
    /// before the first packet. The drain uses it to place this channel on
    /// the session timeline beside the microphone.
    pub fn start_qpc_ns(&self) -> Option<u64> {
        match self.start_qpc_ns.load(Ordering::Acquire) {
            0 => None,
            ns => Some(ns),
        }
    }
}

impl Drop for LoopbackHandle {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            // The capture thread wakes at least every 100 ms to check
            // `shutdown` (Windows: wait timeout; Linux: loop timer).
            let _ = t.join();
        }
    }
}

/// Start capturing `target` into a ring of `ring_seconds` at
/// [`LOOPBACK_RATE`]. Blocks until the stream is live or has failed.
pub fn start_process_loopback(
    target: LoopbackTarget,
    ring_seconds: u32,
) -> Result<(LoopbackHandle, Consumer), LoopbackError> {
    #[cfg(windows)]
    {
        win::start(target, ring_seconds)
    }
    #[cfg(target_os = "linux")]
    {
        linux::start(target, ring_seconds)
    }
    #[cfg(target_os = "macos")]
    {
        crate::core_audio_mac::start(target, ring_seconds)
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        let _ = (target, ring_seconds);
        Err(LoopbackError::UnsupportedPlatform)
    }
}

/// Whether this OS has a native system-audio capture API. macOS additionally
/// requires 14.2+ (asked at start); permission is requested there at start.
pub fn supported() -> bool {
    #[cfg(target_os = "macos")]
    {
        crate::core_audio_mac::supported()
    }
    #[cfg(not(target_os = "macos"))]
    {
        cfg!(any(windows, target_os = "linux"))
    }
}

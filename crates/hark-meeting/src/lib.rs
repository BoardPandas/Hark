//! Hark meeting mode: the pure logic behind recording a meeting.
//!
//! - [`session`]: one meeting's lifecycle, `Idle -> Recording -> Finalizing ->
//!   Summarizing -> Done | Failed`, as a total `advance(state, event)`.
//! - [`chunker`]: cuts one channel's continuous audio into ~20-30 s chunks at
//!   the quietest point, and drops chunks with nothing in them.
//! - [`merge`]: orders transcribed chunks from both channels on the shared
//!   session timeline.
//! - [`detect`]: when to offer (or start, or stop) meeting notes, from
//!   snapshots of who is using the microphone.
//! - [`storage`]: which recordings' audio to evict to stay under the cap.
//!
//! The exceptions to "pure" are small and fenced: [`storage_fs`] (measure
//! and delete meeting audio, behind a path guard) and [`probe_win`] (read who
//! holds the microphone on Windows).
//!
//! Everything else is arithmetic: no I/O, no threads, no clocks. The
//! session timeline is counted in 16 kHz samples from the session start, the
//! same unit the spool files and the chunker use, so tests assert exact sample
//! counts and never wall-clock time.

pub mod chunker;
pub mod detect;
pub mod merge;
pub mod probe_win;
pub mod session;
pub mod storage;
pub mod storage_fs;

pub use chunker::{Chunk, ChunkParams, Chunker};
pub use detect::{Answer, DetectConfig, DetectMode, Detector, Verdict};
pub use merge::{Segment, Transcript};
pub use session::{advance, Action, Event, Failure, SessionState};
pub use storage::{plan_eviction, EvictionPlan, StoredAudio};

/// The session timeline's rate: every offset in this crate is a count of
/// 16 kHz mono samples since the session started.
pub const SAMPLE_RATE: u32 = hark_audio::TARGET_RATE;

/// Which side of the call a stream carries. Known exactly from the source,
/// never inferred: the microphone is `Me`, system-audio loopback is `Them`.
///
/// Ordered `Me < Them`, which is the tie-break when both channels have a
/// segment starting at the same sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Channel {
    Me,
    Them,
}

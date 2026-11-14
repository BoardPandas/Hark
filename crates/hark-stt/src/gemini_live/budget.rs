//! How long a Gemini Live session waits, and on what terms: the connect,
//! send and finalise bounds, and the [`Finalize`] budget each kind of turn
//! runs under.

// Named only by the doc comments below.
#[cfg(doc)]
use super::TranscribeMode;
#[cfg(doc)]
use crate::error::SttError;

/// How long to wait for the next frame while finalising a replayed clip.
/// Shorter than the REST budget: by this point the audio is already uploaded,
/// so a slow finalise is a stall, not a large transfer still in flight.
pub const FINALIZE_TIMEOUT_MS: u64 = 8_000;

/// The same wait for a turn that was streamed while the key was held.
///
/// Far shorter than a replay's, because there is almost nothing left to do:
/// the server has been transcribing all along and only the tail is
/// outstanding. Across 547 streamed dictations in one user's log, release to
/// injected text never exceeded 967 ms — including an 85-second dictation.
/// A later investigation (2026-10-30) of 1,902 good streamed dictations found
/// only 4 with release to inject above 1.5 s, while stalled turns
/// (`turnComplete` never arrives, so the wait only ends at this bound) hit
/// about half of all sessions in bad windows. A turn silent for 1.5 s is
/// almost certainly stalled, and the one known way into that state (see
/// [`TranscribeMode::Smart`]) never finishes at all. Waiting longer only
/// delays the fallback that rescues the dictation. The wait still surfaces as
/// [`SttError::Stalled`], never as a network timeout. OpenAI live keeps its
/// own, longer value: there is no latency data for it yet.
pub const STREAM_FINALIZE_TIMEOUT_MS: u64 = 1_500;

/// The whole finalise budget, independent of [`FINALIZE_TIMEOUT_MS`].
///
/// A per-frame timeout alone is not a bound: it is reset by every frame that
/// arrives, so a server that keeps talking without ever closing the turn holds
/// the read loop open forever. The pipeline worker thread is inside `block_on`
/// for that whole time, and it is the only consumer of push-to-talk events —
/// so an unbounded finalise is not a slow dictation, it is an app that has
/// stopped responding until it is killed. This is the ceiling that cannot be
/// pushed back, and it matches the batch adapters' total request budget so no
/// dictation can wait longer on the streaming path than it would have on the
/// ordinary one.
pub const FINALIZE_TOTAL_MS: u64 = 15_000;

// The two budgets are only meaningful in relation to each other, and both are
// constants — so the relationship is checked when the crate compiles rather
// than when a test happens to run. A total at or below the per-frame wait
// would cut off a finalise that was still making progress, and one that did
// not match the batch adapters' budget would mean a dictation could wait
// longer on the streaming path than on the ordinary one.
const _: () = assert!(FINALIZE_TOTAL_MS > FINALIZE_TIMEOUT_MS);
const _: () = assert!(FINALIZE_TOTAL_MS == crate::TOTAL_TIMEOUT_MS);
// A streamed turn has less left to transcribe than a replay, never more.
const _: () = assert!(STREAM_FINALIZE_TIMEOUT_MS < FINALIZE_TIMEOUT_MS);

/// A meeting chunk's per-frame finalise wait. Longer than a dictation replay's:
/// a meeting chunk is up to 30 s of audio rather than a few, and nothing is
/// waiting on it (the live transcript lags the call by design), so patience
/// costs nothing while a premature timeout costs a line.
pub const MEETING_FINALIZE_TIMEOUT_MS: u64 = 15_000;
/// A meeting chunk's whole finalise ceiling.
pub const MEETING_FINALIZE_TOTAL_MS: u64 = 45_000;
const _: () = assert!(MEETING_FINALIZE_TOTAL_MS > MEETING_FINALIZE_TIMEOUT_MS);
const _: () = assert!(MEETING_FINALIZE_TIMEOUT_MS > FINALIZE_TIMEOUT_MS);

/// How long, and on what terms, a session waits for the server to finish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Finalize {
    /// How long the server may stay silent between frames.
    pub per_frame_ms: u64,
    /// The ceiling that no stream of frames can push back.
    pub total_ms: u64,
    /// A session in which the server never produced a single word (no
    /// interim, no final) returns an empty transcript instead of a timeout.
    ///
    /// Only for meeting chunks. A chunk can pass the loudness gate on a cough
    /// or a keyboard and still hold no speech, and Gemini Live answers such a
    /// turn with nothing at all: the first real call logged 19 of those as
    /// timeouts. A dictation keeps the timeout: the user pressed the key to
    /// say something, and silence there is a failure worth reporting.
    pub quiet_is_empty: bool,
}

impl Finalize {
    /// A dictation replayed from a finished clip.
    pub const REPLAY: Finalize = Finalize {
        per_frame_ms: FINALIZE_TIMEOUT_MS,
        total_ms: FINALIZE_TOTAL_MS,
        quiet_is_empty: false,
    };
    /// A dictation streamed while the key was held.
    pub const STREAM: Finalize = Finalize {
        per_frame_ms: STREAM_FINALIZE_TIMEOUT_MS,
        total_ms: FINALIZE_TOTAL_MS,
        quiet_is_empty: false,
    };
    /// A meeting's live-transcript chunk.
    pub const MEETING_CHUNK: Finalize = Finalize {
        per_frame_ms: MEETING_FINALIZE_TIMEOUT_MS,
        total_ms: MEETING_FINALIZE_TOTAL_MS,
        quiet_is_empty: true,
    };
}

// Only meeting chunks may read a wordless turn as empty: a dictation's
// silence is a failure worth reporting (see `Finalize::quiet_is_empty`).
const _: () = assert!(!Finalize::REPLAY.quiet_is_empty && !Finalize::STREAM.quiet_is_empty);
const _: () = assert!(Finalize::MEETING_CHUNK.quiet_is_empty);

/// How long any single socket write may take.
///
/// `SinkExt::send` has no timeout of its own, and these writes happen on the
/// pipeline worker thread via `block_on`. A socket that stops accepting bytes
/// — a stalled peer, a dead link the OS has not given up on yet — would wedge
/// that thread with no way out. Generous, because a write that cannot complete
/// in this long is not slow, it is stuck.
pub const SEND_TIMEOUT_MS: u64 = 5_000;

/// Connect + setup budget. A Live session that cannot hand-shake quickly has
/// already lost to the batch adapters.
pub const CONNECT_TIMEOUT_MS: u64 = 4_000;

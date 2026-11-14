//! How long an OpenAI live session waits: the connect, send and finalise
//! bounds, and the [`Finalize`] budget each kind of turn runs under.

/// Connect budget (TCP + TLS + upgrade): the same bound as Gemini Live. A
/// socket that cannot open in this long has already lost to the hold, and the
/// pipeline falls back to replaying the finished clip.
pub const CONNECT_TIMEOUT_MS: u64 = 4_000;

/// How long any single socket write may take. `SinkExt::send` has no timeout
/// of its own and these writes run on the pipeline worker thread inside
/// `block_on`, so an unbounded one can wedge the only consumer of push-to-talk
/// events.
pub const SEND_TIMEOUT_MS: u64 = 5_000;

/// How long the server may stay silent between frames while finalising a
/// replayed clip, which still has the whole utterance to transcribe.
pub const FINALIZE_TIMEOUT_MS: u64 = 8_000;

/// The same wait for a turn streamed during the hold. Only the tail is left:
/// CP0's commit-to-final was 419-731 ms, so a server silent for several times
/// that is not finishing, and the replay that rescues the dictation should not
/// wait behind it.
pub const STREAM_FINALIZE_TIMEOUT_MS: u64 = 3_000;

/// The finalise ceiling no stream of frames can push back. A per-frame bound
/// alone is renewed by every delta, so a server that keeps talking without
/// completing would hold the worker thread forever. Matches the batch
/// adapters' total budget, so no dictation waits longer here than there.
pub const FINALIZE_TOTAL_MS: u64 = 15_000;

const _: () = assert!(FINALIZE_TOTAL_MS > FINALIZE_TIMEOUT_MS);
const _: () = assert!(FINALIZE_TOTAL_MS == crate::TOTAL_TIMEOUT_MS);
const _: () = assert!(STREAM_FINALIZE_TIMEOUT_MS < FINALIZE_TIMEOUT_MS);

/// How long a session waits for the server to finish the turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Finalize {
    /// How long the server may stay silent between frames.
    pub per_frame_ms: u64,
    /// The ceiling that no stream of frames can push back.
    pub total_ms: u64,
}

impl Finalize {
    /// A finished clip replayed over the socket (Test connection).
    pub const REPLAY: Finalize = Finalize {
        per_frame_ms: FINALIZE_TIMEOUT_MS,
        total_ms: FINALIZE_TOTAL_MS,
    };
    /// A dictation streamed while the key was held.
    pub const STREAM: Finalize = Finalize {
        per_frame_ms: STREAM_FINALIZE_TIMEOUT_MS,
        total_ms: FINALIZE_TOTAL_MS,
    };
}

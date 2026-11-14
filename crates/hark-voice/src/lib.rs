//! Hark voice layer: optional BYOK cleanup of the corrected transcript before
//! injection. After STT and spellbook correction, one low-temperature
//! chat-completions call rewrites the transcript in the user's chosen voice;
//! Verbatim never calls, short utterances skip via a word-count gate, and
//! every failure is fail-open (the uncleaned transcript always injects).
//!
//! Same discipline as hark-stt: the adapter is I/O-thin over pure, unit-
//! testable request/response functions; all HTTP is `reqwest::blocking` on
//! the pipeline worker thread (no tokio); no code path ever logs API keys,
//! prompts, or transcript text.

mod error;
pub mod openai_compatible;
mod speaker_names;
mod speaker_refs;
pub mod summary;
mod voices;

pub use error::{error_for_status, error_for_transport, CleanupError};
pub use speaker_names::{
    build_guess_request, guess_speaker_names, parse_guesses, NameLine, RawGuess, GUESS_TIMEOUT_MS,
};
pub use speaker_refs::{speaker_spans, Span, SpeakerRefs, UNKNOWN_SPEAKER};
pub use summary::{
    build_summary_request, parse_notes, summarize, ActionItem, MeetingNotes, SummaryConfig,
    SummaryError, DEFAULT_SUMMARY_TEMPLATE, SUMMARY_TIMEOUT_MS,
};
pub use voices::{
    over_expanded, present_terms, reads_as_reply, skips_cleanup, system_prompt, UnknownVoice,
    Voice, EXPANSION_GRACE_WORDS, LENGTH_DISCIPLINE_CLAUSE, PUNCTUATION_CLAUSE, RETURN_ONLY_CLAUSE,
};

/// Connect timeout enforced by the shared HTTP client (built once per process
/// by `hark_stt::shared_client` and cloned here; `Client` is an `Arc`
/// internally). Mirrored so transport-error mapping can name the right bound.
pub const CONNECT_TIMEOUT_MS: u64 = 3_000;

/// Ceiling of the per-request total timeout for cleanup calls
/// (`RequestBuilder::timeout`), tighter than STT's 15 s: cleanup failure has a
/// graceful fallback (inject the uncleaned text), so it fails fast and never
/// retries. A request gets [`cleanup_timeout_ms`], which only reaches this for
/// a long input.
pub const CLEANUP_TIMEOUT_MS: u64 = 10_000;
/// What a cleanup call is allowed for even a one-word input. LLM latency has a
/// fat tail, so this is not lower.
pub const CLEANUP_TIMEOUT_FLOOR_MS: u64 = 4_000;

/// Per-request timeout for cleaning `input_chars` characters: the floor plus
/// 5 ms per character, capped at [`CLEANUP_TIMEOUT_MS`] (reached at 1,200
/// characters). A short dictation that hangs now falls back to its uncleaned
/// text after seconds rather than ten.
pub fn cleanup_timeout_ms(input_chars: usize) -> u64 {
    (input_chars as u64)
        .saturating_mul(5)
        .saturating_add(CLEANUP_TIMEOUT_FLOOR_MS)
        .clamp(CLEANUP_TIMEOUT_FLOOR_MS, CLEANUP_TIMEOUT_MS)
}

/// One cleanup result. `request_ms` is the full HTTP round trip as seen by
/// the caller (added to release-to-inject latency when cleanup runs).
pub struct Cleaned {
    pub text: String,
    pub request_ms: u128,
}

/// A configured, reusable cleanup adapter (the analog of `SttProvider`; a
/// trait so the pipeline worker tests can script it like `MockProvider`).
pub trait CleanupProvider: Send {
    /// Blocking; called from the pipeline worker thread. Returns the
    /// rewritten text plus wall time. Implementations must never log keys,
    /// prompts, or text.
    fn clean(&self, text: &str) -> Result<Cleaned, CleanupError>;

    /// Short label for logs and errors ("openai", "gemini").
    fn label(&self) -> &str;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_timeout_scales_with_input_between_floor_and_cap() {
        assert_eq!(cleanup_timeout_ms(0), 4_000);
        assert_eq!(cleanup_timeout_ms(1), 4_005);
        assert_eq!(cleanup_timeout_ms(200), 5_000);
        assert_eq!(cleanup_timeout_ms(1_199), 9_995);
        assert_eq!(cleanup_timeout_ms(1_200), CLEANUP_TIMEOUT_MS);
        assert_eq!(cleanup_timeout_ms(1_000_000), CLEANUP_TIMEOUT_MS);
        assert_eq!(cleanup_timeout_ms(usize::MAX), CLEANUP_TIMEOUT_MS);
    }
}

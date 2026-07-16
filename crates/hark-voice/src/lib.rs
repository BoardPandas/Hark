//! Hark voice layer: optional BYOK cleanup of the corrected transcript before
//! injection. After STT and dictionary correction, one low-temperature
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

pub use error::{error_for_status, error_for_transport, CleanupError};

/// Connect timeout enforced by the shared HTTP client (built once per process
/// by `hark_stt::shared_client` and cloned here; `Client` is an `Arc`
/// internally). Mirrored so transport-error mapping can name the right bound.
pub const CONNECT_TIMEOUT_MS: u64 = 3_000;

/// Per-request total timeout for cleanup calls (`RequestBuilder::timeout`),
/// tighter than STT's 15 s: cleanup failure has a graceful fallback (inject
/// the uncleaned text), so it fails fast and never retries.
pub const CLEANUP_TIMEOUT_MS: u64 = 10_000;

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

    /// Short label for logs and errors ("openai", "groq").
    fn label(&self) -> &str;
}

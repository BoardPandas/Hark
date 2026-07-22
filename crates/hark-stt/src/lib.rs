//! Hark BYOK cloud STT: one `SttProvider` trait, per-contract adapters.
//!
//! Adapters are deliberately I/O-thin: they take complete WAV bytes and return
//! text plus wall time. WAV encoding, timing loops, and reporting live in the
//! caller (the spike harness now, the pipeline worker thread later). All HTTP
//! is `reqwest::blocking` on the calling thread — there is no tokio runtime in
//! this crate. Implementations must never log API keys or raw audio.

mod config;
pub mod deepgram;
mod error;
pub mod fixture;
pub mod gemini;
pub mod metrics;
pub mod openai_compatible;
pub mod wav;

pub use config::{ProviderConfig, ProviderKind};
pub use error::{error_for_status, error_for_transport, SttError};

/// Connect timeout for provider requests. Latency is the product: fail fast.
pub const CONNECT_TIMEOUT_MS: u64 = 3_000;
/// Total request timeout (connect + upload + transcription + download).
pub const TOTAL_TIMEOUT_MS: u64 = 15_000;

/// One transcription result. `request_ms` is the full HTTP round trip as seen
/// by the caller (the dominant share of release-to-inject latency).
pub struct Transcript {
    /// The verbatim transcript, always present. Stays the ground truth even
    /// when `cleaned` is populated: dictionary correction and the cleanup
    /// expansion guard both need the un-rewritten text.
    pub text: String,
    /// Populated only by fused adapters (Gemini), which return the cleaned
    /// rewrite from the same round trip. `None` means "this provider did not
    /// do cleanup" — the caller runs its own cleanup pass as before.
    pub cleaned: Option<String>,
    pub request_ms: u128,
}

/// A configured, reusable cloud transcription adapter.
pub trait SttProvider: Send {
    /// Blocking; called from the pipeline worker thread. `wav_bytes` is a
    /// complete 16 kHz mono WAV. Implementations must never log `api_key` or
    /// raw audio.
    fn transcribe(&self, wav_bytes: &[u8]) -> Result<Transcript, SttError>;

    /// Short label for reports and errors ("groq", "openai", "deepgram").
    fn label(&self) -> &str;
}

/// Build the adapter for a config, sharing the process-wide HTTP client.
pub fn build(
    config: &ProviderConfig,
    client: reqwest::blocking::Client,
) -> Result<Box<dyn SttProvider>, SttError> {
    match config.kind {
        ProviderKind::OpenAiCompatible => Ok(Box::new(openai_compatible::OpenAiCompatible::new(
            config, client,
        ))),
        ProviderKind::Deepgram => Ok(Box::new(deepgram::Deepgram::new(config, client)?)),
        ProviderKind::Gemini => Ok(Box::new(gemini::Gemini::new(config, client))),
    }
}

/// The one long-lived HTTP client per process: keep-alive + TLS session
/// resumption are a large share of warm-request latency savings. Build once,
/// clone freely (`Client` is an `Arc` internally).
pub fn shared_client() -> Result<reqwest::blocking::Client, SttError> {
    client_with_timeout(TOTAL_TIMEOUT_MS)
}

/// A client identical to [`shared_client`] but with a different total request
/// budget.
///
/// This exists for the on-device fallback: when a local model is armed and
/// ready, waiting the full [`TOTAL_TIMEOUT_MS`] before failing over would make
/// a rescued dictation take 15 s of cloud plus ~2 s of local decoding. A
/// fallback that slow is worse than none, so the pipeline gives the cloud a
/// shorter budget precisely when it has something to fall back to.
pub fn client_with_timeout(total_ms: u64) -> Result<reqwest::blocking::Client, SttError> {
    reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_millis(
            CONNECT_TIMEOUT_MS.min(total_ms),
        ))
        .timeout(std::time::Duration::from_millis(total_ms))
        .build()
        .map_err(|e| SttError::Http {
            provider: "client".to_string(),
            detail: e.to_string(),
        })
}

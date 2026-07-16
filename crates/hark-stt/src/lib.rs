//! Hark BYOK cloud STT: one `SttProvider` trait, per-contract adapters.
//!
//! Adapters are deliberately I/O-thin: they take complete WAV bytes and return
//! text plus wall time. WAV encoding, timing loops, and reporting live in the
//! caller (the spike harness now, the pipeline worker thread later). All HTTP
//! is `reqwest::blocking` on the calling thread — there is no tokio runtime in
//! this crate. Implementations must never log API keys or raw audio.

mod config;
mod error;
pub mod metrics;
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
    pub text: String,
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

/// The one long-lived HTTP client per process: keep-alive + TLS session
/// resumption are a large share of warm-request latency savings. Build once,
/// clone freely (`Client` is an `Arc` internally).
pub fn shared_client() -> Result<reqwest::blocking::Client, SttError> {
    reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_millis(CONNECT_TIMEOUT_MS))
        .timeout(std::time::Duration::from_millis(TOTAL_TIMEOUT_MS))
        .build()
        .map_err(|e| SttError::Http {
            provider: "client".to_string(),
            detail: e.to_string(),
        })
}

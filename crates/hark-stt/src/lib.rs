//! Hark BYOK cloud STT: one `SttProvider` trait, per-contract adapters.
//!
//! Adapters are deliberately I/O-thin: they take complete WAV bytes and return
//! text plus wall time. WAV encoding, timing loops, and reporting live in the
//! caller (the spike harness now, the pipeline worker thread later). Batch HTTP
//! uses `reqwest::blocking` on the calling thread. Gemini Live is the deliberate
//! exception: its adapter owns a private current-thread Tokio runtime for the
//! WebSocket session; there is no process-wide runtime. Implementations must
//! never log API keys or raw audio.

mod config;
pub mod deepgram;
mod error;
pub mod fixture;
pub mod gemini;
pub mod gemini_live;
pub mod metrics;
pub mod openai_compatible;
pub mod openai_transcribe;
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
    /// when `cleaned` is populated: spellbook correction and the cleanup
    /// expansion guard both need the un-rewritten text.
    pub text: String,
    /// Populated only by fused adapters (Gemini), which return the cleaned
    /// rewrite from the same round trip. `None` means "this provider did not
    /// do cleanup" — the caller runs its own cleanup pass as before.
    pub cleaned: Option<String>,
    pub request_ms: u128,
}

/// A transcription session fed while the speaker is still talking.
///
/// This exists because [`SttProvider::transcribe`] takes a finished clip, and
/// a finished clip is only available after the key is released — at which
/// point every byte still has to be uploaded before any text can come back.
/// A session is opened on key-down instead and fed as audio is captured, so
/// release-to-inject shrinks to whatever is left rather than the whole clip.
///
/// Sessions are single-use and single-threaded: the pump thread owns one for
/// the length of one dictation and drops it afterwards.
pub trait LiveSession: Send {
    /// Push 16 kHz mono samples. Blocking, but only for one socket write.
    fn push(&mut self, samples_16k: &[f32]) -> Result<(), SttError>;

    /// End the turn and wait for the final transcript.
    fn finish(&mut self) -> Result<Transcript, SttError>;
}

/// An adapter that can open a [`LiveSession`].
///
/// Deliberately separate from [`SttProvider`] rather than a method on it: most
/// adapters are a single HTTP POST and have nothing to stream into, and giving
/// them a `start_session` that always fails would put a runtime error where a
/// compile-time absence belongs.
pub trait LiveStt: Send + Sync {
    /// Open a session. Blocking (connect + handshake + setup).
    fn start_session(&self) -> Result<Box<dyn LiveSession>, SttError>;
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
        ProviderKind::OpenAiTranscribe => Ok(Box::new(openai_transcribe::OpenAiTranscribe::new(
            config, client,
        ))),
        ProviderKind::Deepgram => Ok(Box::new(deepgram::Deepgram::new(config, client)?)),
        ProviderKind::GeminiLive => Ok(Box::new(gemini_live::GeminiLive::new(
            config,
            config.live_mode,
        )?)),
        ProviderKind::Gemini => Ok(Box::new(gemini::Gemini::new(config, client))),
    }
}

/// The streaming adapter for a config, when the provider has one.
///
/// Separate from [`build`] rather than folded into it because the two are
/// used at different moments: this one is opened on key-down, the batch
/// adapter only if streaming could not carry the dictation. The pair costs a
/// second adapter instance, which for Gemini Live means a second idle
/// `current_thread` runtime — no extra threads, and the alternative is
/// downcasting a `dyn SttProvider` to find out whether it also streams.
///
/// `None` means "this provider does not stream", which is every batch adapter
/// and any build without the `live` feature.
pub fn build_live(config: &ProviderConfig) -> Option<Box<dyn LiveStt>> {
    match config.kind {
        #[cfg(feature = "live")]
        ProviderKind::GeminiLive => match gemini_live::GeminiLive::new(config, config.live_mode) {
            Ok(adapter) => Some(Box::new(adapter) as Box<dyn LiveStt>),
            Err(e) => {
                log::warn!("no live session available for {}: {e}", config.label);
                None
            }
        },
        _ => None,
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

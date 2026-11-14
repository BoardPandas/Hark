//! Hark BYOK cloud STT: one `SttProvider` trait, per-contract adapters.
//!
//! Adapters are deliberately I/O-thin: they take complete WAV bytes and return
//! text plus wall time. WAV encoding, timing loops, and reporting live in the
//! caller (the spike harness now, the pipeline worker thread later). Batch HTTP
//! uses `reqwest::blocking` on the calling thread. The two live WebSocket
//! adapters, Gemini Live and OpenAI live transcribe, are the deliberate
//! exception: each owns a private current-thread Tokio runtime for its
//! sessions, and there is no process-wide runtime. Implementations must never
//! log API keys or raw audio.

mod config;
pub mod deepgram;
mod error;
pub mod fixture;
pub mod gemini;
pub mod gemini_live;
#[cfg(feature = "live")]
mod live_transport;
pub mod meeting;
pub mod meeting_gemini;
pub mod metrics;
pub mod openai_compatible;
pub mod openai_live;
pub mod openai_transcribe;
pub mod wav;

pub use config::{ProviderConfig, ProviderKind};
pub use error::{error_for_status, error_for_transport, SttError};

/// Connect timeout for provider requests. Latency is the product: fail fast.
pub const CONNECT_TIMEOUT_MS: u64 = 3_000;
/// Total request timeout (connect + upload + transcription + download). The
/// ceiling for every batch request: a dictation gets less for a short clip
/// (see [`dictation_timeout_ms`]), a meeting chunk and the shared client get
/// exactly this.
pub const TOTAL_TIMEOUT_MS: u64 = 15_000;
/// What a dictation batch request is allowed even for a near-empty clip. Not
/// lower: OpenAI-compatible endpoints include slow self-hosted servers, and
/// the first request after idle pays a cold connection.
pub const DICTATION_TIMEOUT_FLOOR_MS: u64 = 6_000;

/// Per-attempt total timeout for a dictation batch request carrying
/// `audio_ms` of audio: the floor plus half the clip length, capped at
/// [`TOTAL_TIMEOUT_MS`] (reached at 18 s of audio). A hung provider used to
/// cost the full 15 s on every attempt, and a retry doubled it.
pub fn dictation_timeout_ms(audio_ms: u64) -> u64 {
    DICTATION_TIMEOUT_FLOOR_MS
        .saturating_add(audio_ms / 2)
        .clamp(DICTATION_TIMEOUT_FLOOR_MS, TOTAL_TIMEOUT_MS)
}

/// Duration of a 16 kHz mono PCM16 WAV from its byte length (44-byte header,
/// 32 000 bytes per second). A file with extra chunks reads slightly long,
/// which only makes the timeout marginally more generous.
fn wav_audio_ms(wav_bytes: &[u8]) -> u64 {
    const HEADER_BYTES: usize = 44;
    const BYTES_PER_SEC: u64 = wav::SAMPLE_RATE as u64 * wav::BITS_PER_SAMPLE as u64 / 8;
    wav_bytes.len().saturating_sub(HEADER_BYTES) as u64 * 1_000 / BYTES_PER_SEC
}

/// The total timeout one batch request runs under. `fixed` is `Some` for a
/// caller that needs an exact bound (meeting chunks, the on-device fallback's
/// shortened leash); `None` is a dictation, scaled to the clip.
///
/// Set on each request rather than on the client because a request-level
/// timeout replaces the client's: the shared client's flat total no longer
/// decides anything for the adapters below.
pub(crate) fn request_total_ms(fixed: Option<u64>, wav_bytes: &[u8]) -> u64 {
    fixed.unwrap_or_else(|| dictation_timeout_ms(wav_audio_ms(wav_bytes)))
}

/// One transcription result. `request_ms` is the full HTTP round trip as seen
/// by the caller (the dominant share of release-to-inject latency). For a
/// [`LiveSession`] it is the wait inside `finish` only: the session is open
/// for the whole hold, and the hold is not latency.
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
    /// Push mono samples at the rate [`LiveStt::input_rate`] declares.
    /// Blocking, but only for the socket writes those samples need.
    fn push(&mut self, samples: &[f32]) -> Result<(), SttError>;

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

    /// The sample rate [`LiveSession::push`] expects. Per provider, not a
    /// pipeline constant: Gemini Live takes the pipeline's 16 kHz, OpenAI's
    /// realtime API accepts only 24 kHz.
    fn input_rate(&self) -> u32 {
        wav::SAMPLE_RATE
    }
}

/// A configured, reusable cloud transcription adapter.
pub trait SttProvider: Send {
    /// Blocking; called from the pipeline worker thread. `wav_bytes` is a
    /// complete 16 kHz mono WAV. Implementations must never log `api_key` or
    /// raw audio.
    fn transcribe(&self, wav_bytes: &[u8]) -> Result<Transcript, SttError>;

    /// Short label for reports and errors ("openai", "deepgram", "gemini").
    fn label(&self) -> &str;
}

/// Build the adapter for a meeting's live-transcript chunks. The same as
/// [`build`] except that Gemini Live is tuned for 20-30 s clips that may hold
/// no speech (see [`gemini_live::Finalize::MEETING_CHUNK`]).
///
/// A batch adapter here keeps the flat [`TOTAL_TIMEOUT_MS`] rather than the
/// dictation timeout scaled to clip length: a chunk is not waited on, and a
/// short final chunk must not get less patience than a long one.
pub fn build_meeting_chunks(
    config: &ProviderConfig,
    client: reqwest::blocking::Client,
) -> Result<Box<dyn SttProvider>, SttError> {
    match config.kind {
        ProviderKind::GeminiLive => Ok(Box::new(
            gemini_live::GeminiLive::new(config, config.live_mode)?.for_meeting_chunks(),
        )),
        _ => build_with(config, client, Some(TOTAL_TIMEOUT_MS)),
    }
}

/// Build the adapter for a dictation, sharing the process-wide HTTP client.
/// Each batch request is bounded by [`dictation_timeout_ms`] for its clip.
pub fn build(
    config: &ProviderConfig,
    client: reqwest::blocking::Client,
) -> Result<Box<dyn SttProvider>, SttError> {
    build_with(config, client, None)
}

/// [`build`] with every batch request held to exactly `total_ms`, however long
/// the clip. For the on-device fallback, whose shortened leash is a fixed
/// budget (see [`client_with_timeout`]); a request-level timeout replaces the
/// client's, so a client built with a short total only keeps it if the
/// adapter is built through here with the same value.
pub fn build_with_total_timeout(
    config: &ProviderConfig,
    client: reqwest::blocking::Client,
    total_ms: u64,
) -> Result<Box<dyn SttProvider>, SttError> {
    build_with(config, client, Some(total_ms))
}

fn build_with(
    config: &ProviderConfig,
    client: reqwest::blocking::Client,
    fixed_total_ms: Option<u64>,
) -> Result<Box<dyn SttProvider>, SttError> {
    match config.kind {
        ProviderKind::OpenAiCompatible => Ok(Box::new(
            openai_compatible::OpenAiCompatible::new(config, client)
                .with_total_timeout(fixed_total_ms),
        )),
        ProviderKind::OpenAiTranscribe => Ok(Box::new(
            openai_transcribe::OpenAiTranscribe::new(config, client)
                .with_total_timeout(fixed_total_ms),
        )),
        ProviderKind::Deepgram => Ok(Box::new(
            deepgram::Deepgram::new(config, client)?.with_total_timeout(fixed_total_ms),
        )),
        ProviderKind::GeminiLive => Ok(Box::new(gemini_live::GeminiLive::new(
            config,
            config.live_mode,
        )?)),
        ProviderKind::Gemini => Ok(Box::new(
            gemini::Gemini::new(config, client).with_total_timeout(fixed_total_ms),
        )),
        // The replay adapter: a finished clip over the realtime socket, so
        // Test connection exercises the live path. A dictation the stream
        // could not carry, and every meeting chunk, go to the batch REST
        // route instead (the pipeline's `batch_stt_route`), so this is never
        // the fallback for its own failure.
        ProviderKind::OpenAiLive => Ok(Box::new(openai_live::OpenAiLive::new(config)?)),
    }
}

/// The streaming adapter for a config, when the provider has one.
///
/// Separate from [`build`] rather than folded into it because the two are
/// used at different moments: this one is opened on key-down, the batch
/// adapter only if streaming could not carry the dictation. The pair costs a
/// second adapter instance, which for either live adapter means a second idle
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
        #[cfg(feature = "live")]
        ProviderKind::OpenAiLive => match openai_live::OpenAiLive::new(config) {
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
/// budget. Adapters override it per request, so pair it with
/// [`build_with_total_timeout`].
///
/// This exists for the on-device fallback: when a local model is armed and
/// ready, waiting the full [`TOTAL_TIMEOUT_MS`] before failing over would make
/// a rescued dictation take 15 s of cloud plus ~2 s of local decoding. A
/// fallback that slow is worse than none, so the pipeline gives the cloud a
/// shorter budget precisely when it has something to fall back to.
pub fn client_with_timeout(total_ms: u64) -> Result<reqwest::blocking::Client, SttError> {
    build_client(CONNECT_TIMEOUT_MS.min(total_ms), total_ms)
}

/// A client on the static roots with the given connect and total bounds.
/// Private so every caller goes through a named budget (dictation or
/// [`meeting::final_pass_client`]).
fn build_client(connect_ms: u64, total_ms: u64) -> Result<reqwest::blocking::Client, SttError> {
    let map_err = |e: reqwest::Error| error_for_transport("client", total_ms, &e);
    reqwest::blocking::Client::builder()
        .tls_certs_only(static_trust_roots().map_err(map_err)?)
        .connect_timeout(std::time::Duration::from_millis(connect_ms))
        .timeout(std::time::Duration::from_millis(total_ms))
        .build()
        .map_err(map_err)
}

/// Mozilla's root set, compiled in, as the client's only trust roots.
///
/// reqwest's `rustls` feature otherwise verifies against the OS certificate
/// store, which drifts per machine and lets a locally installed CA (corporate
/// proxy, malware) intercept requests carrying the user's API key.
/// `tls_certs_only` with this list keeps every Hark client on the same static
/// roots on all three platforms.
fn static_trust_roots() -> Result<Vec<reqwest::Certificate>, reqwest::Error> {
    webpki_root_certs::TLS_SERVER_ROOT_CERTS
        .iter()
        .map(|der| reqwest::Certificate::from_der(der.as_ref()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // An empty list would make tls_certs_only reject every server, so every
    // cloud request would fail at the TLS handshake.
    #[test]
    fn static_trust_roots_are_present_and_build_a_client() {
        assert!(static_trust_roots().unwrap().len() > 100);
        assert!(shared_client().is_ok());
        assert!(meeting::final_pass_client().is_ok());
    }

    #[test]
    fn dictation_timeout_scales_with_the_clip_between_floor_and_cap() {
        assert_eq!(dictation_timeout_ms(0), 6_000);
        assert_eq!(dictation_timeout_ms(1_000), 6_500);
        assert_eq!(dictation_timeout_ms(10_000), 11_000);
        assert_eq!(dictation_timeout_ms(17_999), 14_999);
        assert_eq!(dictation_timeout_ms(18_000), TOTAL_TIMEOUT_MS);
        assert_eq!(dictation_timeout_ms(600_000), TOTAL_TIMEOUT_MS);
        assert_eq!(dictation_timeout_ms(u64::MAX), TOTAL_TIMEOUT_MS);
    }

    #[test]
    fn wav_audio_ms_reads_duration_from_byte_length() {
        let one_second = wav::encode_wav_16k_mono(&vec![0.0; 16_000]);
        assert_eq!(wav_audio_ms(&one_second), 1_000);
        assert_eq!(wav_audio_ms(&wav::encode_wav_16k_mono(&[])), 0);
        // Shorter than a header: no underflow.
        assert_eq!(wav_audio_ms(b"RIFF"), 0);
    }

    #[test]
    fn a_fixed_bound_wins_over_clip_scaling() {
        let one_second = wav::encode_wav_16k_mono(&vec![0.0; 16_000]);
        assert_eq!(request_total_ms(None, &one_second), 6_500);
        assert_eq!(request_total_ms(Some(15_000), &one_second), 15_000);
        assert_eq!(request_total_ms(Some(6_000), &[]), 6_000);
    }

    // A server that accepts and never answers must surface as a Timeout that
    // names the bound in force, not the 15 s cap or the client's.
    #[test]
    fn a_hung_batch_request_times_out_naming_its_own_bound() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let _hold = std::thread::spawn(move || {
            let _conn = listener.accept();
            std::thread::sleep(std::time::Duration::from_secs(3));
        });
        let config = ProviderConfig {
            kind: ProviderKind::OpenAiCompatible,
            label: "test".to_string(),
            base_url,
            model: "m".to_string(),
            api_key: "k".to_string(),
            bias_terms: Vec::new(),
            cleanup_instruction: None,
            live_mode: gemini_live::TranscribeMode::Verbatim,
        };
        let adapter = build_with_total_timeout(&config, shared_client().unwrap(), 300).unwrap();
        let wav = wav::encode_wav_16k_mono(&[0.0; 1_600]);
        match adapter.transcribe(&wav) {
            Err(SttError::Timeout { configured_ms, .. }) => assert_eq!(configured_ms, 300),
            other => panic!("expected a 300 ms timeout, got {:?}", other.err()),
        }
    }
}

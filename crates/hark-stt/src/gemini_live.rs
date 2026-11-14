//! The Gemini Live adapter: a bidirectional WebSocket session against
//! `gemini-3.5-transcribe-live`.
//!
//! Earns its own adapter — and the crate's only async runtime — because the
//! Live API has no REST equivalent that takes inline audio. The batch sibling
//! (`gemini-3.5-transcribe`, [`crate::gemini`]'s neighbour on the Interactions
//! API) requires a Files API upload before transcription can start, which puts
//! a whole extra round trip on the release-to-inject path. The Live API takes
//! raw PCM straight over the socket instead, so it is the only Gemini path that
//! belongs anywhere near the hot path.
//!
//! Three things make it a good fit for Hark specifically:
//!
//! - **The wire format is our capture format.** `audio/pcm;rate=16000`, 16-bit
//!   mono little-endian, is exactly what the ring buffer already holds, so
//!   there is no WAV container to build.
//! - **Finalisation is explicit.** `audioStreamEnd` ends the turn on command
//!   instead of waiting for server VAD to decide the speaker stopped — which
//!   maps directly onto key release, the one moment Hark knows for certain.
//! - **`customVocabulary` is a real biasing slot**, up to 1 000 phrases, so the
//!   spellbook maps over whole rather than being packed into a prompt.
//!
//! # Threading and the runtime
//!
//! The runtime is a private `current_thread` instance owned by this adapter and
//! entered only through `block_on` from the pipeline worker thread. It is never
//! process-wide, no blocking reqwest call from the other adapters can land on
//! it, and the main thread never touches it (LL-G Rust HIGH,
//! `blocking-io-on-tokio`; CLAUDE.md "keep the runtime scoped to that adapter").
//!
//! # Secret handling
//!
//! The Live API takes the API key as a **URL query parameter**, so the socket
//! URL is itself a secret. It is never stored in a loggable field, never
//! formatted into an error, and every error path routes through [`scrub`],
//! which strips any `key=` value before the text can reach a log line.

use crate::error::SttError;
#[cfg(feature = "live")]
use crate::live_transport::current_thread_runtime;
use crate::{ProviderConfig, SttProvider, Transcript};
#[cfg(feature = "live")]
use std::time::Instant;

mod budget;
mod mode;
#[cfg(feature = "live")]
pub(crate) mod session;
#[cfg(all(test, feature = "live"))]
mod session_tests;
#[cfg(test)]
mod tests;
mod wire;

pub use budget::{
    Finalize, CONNECT_TIMEOUT_MS, FINALIZE_TIMEOUT_MS, FINALIZE_TOTAL_MS,
    MEETING_FINALIZE_TIMEOUT_MS, MEETING_FINALIZE_TOTAL_MS, SEND_TIMEOUT_MS,
    STREAM_FINALIZE_TIMEOUT_MS,
};
pub use mode::{checked_text, is_canned_refusal, transcript_for, TranscribeMode};
pub use wire::{
    absorb, activity_end_message, activity_start_message, audio_message, audio_stream_end_message,
    join_segments, parse_server_message, setup_message, top_keys, ServerEvent, CHUNK_SAMPLES,
};

/// Shared with [`crate::openai_live`]; re-exported so existing callers keep
/// their import path.
pub use crate::wav::samples_to_pcm16_le;

/// Wire host for the Live API. The model and key are appended per session.
pub const LIVE_HOST: &str =
    "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent";

/// Strip any `key=...` value out of a string before it can be logged.
///
/// The Live API puts the API key in the socket URL, and tungstenite's error
/// Display echoes the URL it failed on. Every error constructed in this module
/// goes through here; nothing else in the crate needs it because no other
/// adapter carries a secret outside a header.
pub fn scrub(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("key=") {
        out.push_str(&rest[..at]);
        out.push_str("key=<redacted>");
        let after = &rest[at + 4..];
        // The value runs to the next delimiter; everything past it is kept.
        let end = after.find(['&', '"', ' ', '\'']).unwrap_or(after.len());
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

fn fail(detail: String) -> SttError {
    SttError::Provider {
        provider: "gemini-live".to_string(),
        detail: scrub(&detail),
    }
}

/// The socket URL for a session. **Carries the API key** — never log the
/// return value, and never put it in an error without [`scrub`].
pub fn live_url(api_key: &str) -> String {
    format!("{LIVE_HOST}?key={api_key}")
}

/// The Gemini Live adapter.
///
/// Holds no loggable copy of the socket URL: it is rebuilt per session from the
/// key so a `{self:?}`-shaped mistake cannot leak it.
// Without the `live` feature only `label` is read: the session half is gone.
#[cfg_attr(not(feature = "live"), allow(dead_code))]
pub struct GeminiLive {
    label: String,
    model: String,
    api_key: String,
    bias_terms: Vec<String>,
    mode: TranscribeMode,
    /// How a replayed clip finalises: [`Finalize::REPLAY`] for dictation,
    /// [`Finalize::MEETING_CHUNK`] for a meeting's live chunks.
    finalize: Finalize,
    #[cfg(feature = "live")]
    runtime: std::sync::Arc<tokio::runtime::Runtime>,
}

impl GeminiLive {
    pub fn new(config: &ProviderConfig, mode: TranscribeMode) -> Result<Self, SttError> {
        if !config.bias_terms.is_empty() {
            // Counts only: terms are user content and never appear in logs.
            log::info!("custom vocabulary: {} terms", config.bias_terms.len());
        }
        Ok(Self {
            label: config.label.clone(),
            model: config.model.clone(),
            api_key: config.api_key.clone(),
            bias_terms: config.bias_terms.clone(),
            mode,
            finalize: Finalize::REPLAY,
            #[cfg(feature = "live")]
            runtime: std::sync::Arc::new(current_thread_runtime().map_err(|e| {
                fail(format!(
                    "could not start the Live API runtime ({:?})",
                    e.kind()
                ))
            })?),
        })
    }
}

impl GeminiLive {
    /// Tune a replay adapter for meeting chunks (see [`Finalize::MEETING_CHUNK`]).
    pub fn for_meeting_chunks(mut self) -> Self {
        self.finalize = Finalize::MEETING_CHUNK;
        self
    }
}

impl SttProvider for GeminiLive {
    #[cfg(feature = "live")]
    fn transcribe(&self, wav_bytes: &[u8]) -> Result<Transcript, SttError> {
        // The trait hands over a finished WAV, so unwrap it back to samples.
        // A streaming-while-holding pipeline would feed `samples_to_pcm16_le`
        // straight from the ring buffer and skip the container entirely.
        let info = crate::wav::parse_wav_16k_mono(wav_bytes)?;
        let pcm = samples_to_pcm16_le(&info.samples);
        let setup = setup_message(&self.model, &self.bias_terms, self.mode);
        let url = live_url(&self.api_key);

        let started = Instant::now();
        let text =
            self.runtime
                .block_on(session::run(url, setup, pcm, self.mode, self.finalize))?;
        let text = checked_text(self.mode, text)?;
        Ok(transcript_for(
            self.mode,
            text,
            started.elapsed().as_millis(),
        ))
    }

    #[cfg(not(feature = "live"))]
    fn transcribe(&self, _wav_bytes: &[u8]) -> Result<Transcript, SttError> {
        Err(fail(
            "this build has no Gemini Live support (built without the `live` feature)".to_string(),
        ))
    }

    fn label(&self) -> &str {
        &self.label
    }
}

/// One open session, fed incrementally by the pipeline's streaming pump.
#[cfg(feature = "live")]
pub struct GeminiLiveSession {
    runtime: std::sync::Arc<tokio::runtime::Runtime>,
    socket: Option<session::Socket>,
    mode: TranscribeMode,
    /// Samples pushed so far, for the log line only.
    pushed: usize,
    /// Finalised transcript text collected so far. Most of a dictation lands
    /// here during the hold rather than at finalise — see [`session::drain_into`].
    segments: Vec<String>,
    /// The server closed the turn before the key came up. Nothing further will
    /// arrive, so finalising would only wait out the budget.
    turn_complete: bool,
}

#[cfg(feature = "live")]
impl crate::LiveSession for GeminiLiveSession {
    fn push(&mut self, samples_16k: &[f32]) -> Result<(), SttError> {
        let Some(socket) = self.socket.as_mut() else {
            return Err(fail("push after the session was finished".to_string()));
        };
        if samples_16k.is_empty() {
            return Ok(());
        }
        let pcm = samples_to_pcm16_le(samples_16k);
        self.pushed += samples_16k.len();
        let segments = &mut self.segments;
        let complete = self.runtime.block_on(async {
            let mut complete = session::send_audio(socket, &pcm, segments).await?;
            // Transcript and interims both pile up in the receive buffer over a
            // long hold. Reading keeps the socket moving; keeping what is read
            // is what puts the whole utterance in the injected text.
            complete |= session::drain_into(socket, segments).await;
            Ok::<_, SttError>(complete)
        })?;
        self.turn_complete |= complete;
        Ok(())
    }

    fn finish(&mut self) -> Result<Transcript, SttError> {
        let started = Instant::now();
        let Some(mut socket) = self.socket.take() else {
            return Err(fail("the session was already finished".to_string()));
        };
        let mode = self.mode;
        let segments = std::mem::take(&mut self.segments);
        let text = if self.turn_complete {
            session::collected(mode, &segments)?
        } else {
            self.runtime.block_on(async move {
                session::finish(&mut socket, mode, segments, Finalize::STREAM).await
            })?
        };
        let text = checked_text(mode, text)?;
        log::info!(
            "gemini live session: {} ms of audio streamed during the hold",
            self.pushed as u64 * 1_000 / 16_000
        );
        Ok(transcript_for(mode, text, started.elapsed().as_millis()))
    }
}

#[cfg(feature = "live")]
impl crate::LiveStt for GeminiLive {
    fn start_session(&self) -> Result<Box<dyn crate::LiveSession>, SttError> {
        let setup = setup_message(&self.model, &self.bias_terms, self.mode);
        let url = live_url(&self.api_key);
        let socket = self
            .runtime
            .block_on(async move { session::connect(url, setup).await })?;
        Ok(Box::new(GeminiLiveSession {
            runtime: self.runtime.clone(),
            socket: Some(socket),
            mode: self.mode,
            pushed: 0,
            segments: Vec::new(),
            turn_complete: false,
        }))
    }
}

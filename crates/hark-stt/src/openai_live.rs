//! The OpenAI live adapter: a realtime transcription WebSocket against
//! `gpt-live-transcribe`.
//!
//! Earns its own adapter for the same reason Gemini Live did: OpenAI's REST
//! multipart endpoint does not accept `gpt-live-transcribe`, and the realtime
//! socket is the only way to upload while the key is still held. A batch
//! adapter has the whole clip outstanding at release; this one has only the
//! tail and the commit. CP0 measured commit-to-final at about 500 ms (p50) on a
//! 10 s clip, against 667 ms for warm gpt-transcribe batch.
//!
//! Wire facts, confirmed against the real API by
//! `examples/openai_live_probe.rs` (CP0, 2026-10-29):
//!
//! - `wss://…/v1/realtime?intent=transcription` with a Bearer header. No model
//!   in the URL (it selects a conversation session, which rejects the
//!   transcription update) and no `OpenAI-Beta` header (that is the pre-GA
//!   interface).
//! - **24 kHz PCM16 is the only accepted rate.** `rate: 16000` fails the
//!   update with `integer_below_min_value`, so [`INPUT_RATE`] is declared to
//!   the pipeline, which resamples straight from the device rate.
//! - No server VAD. Push-to-talk ends the turn with `input_audio_buffer.commit`,
//!   answered by `input_audio_buffer.committed {item_id}` and later by
//!   `conversation.item.input_audio_transcription.completed` for that item.
//! - A commit needs at least 100 ms buffered (`input_audio_buffer_commit_empty`
//!   below that), so [`commit_padding`] tops a very short hold up with silence.
//! - A bad key passes the handshake (101). The refusal is an `error` event
//!   with `invalid_api_key`, then close code 3000.
//! - Every server frame observed was text. Binary is accepted anyway (LL-G
//!   Rust HIGH, `websocket-binary-frames-not-text`).
//!
//! # Threading and the runtime
//!
//! Exactly as [`crate::gemini_live`]: a private `current_thread` runtime owned
//! by the adapter, shared with its sessions through an `Arc` and entered only
//! through `block_on` from the pipeline worker thread. Nothing blocking runs on
//! it — the replay's resampling happens before `block_on`, never inside it
//! (LL-G Rust HIGH, `blocking-io-on-tokio`).
//!
//! # Secret handling
//!
//! The key travels in an `Authorization` header marked sensitive, never in the
//! URL. OpenAI's auth errors quote a masked key back, so this adapter never
//! reads the server's free-text `message` at all: errors carry only
//! slug-shaped codes, and every error leaving the adapter goes through
//! [`scrub`], which removes the key itself, any `sk-…` fragment, and any
//! Bearer token. Logs carry lengths, counts and milliseconds only.

use crate::error::SttError;
#[cfg(feature = "live")]
use crate::live_transport::current_thread_runtime;
use crate::{ProviderConfig, SttProvider, Transcript};
#[cfg(feature = "live")]
use serde_json::Value;
#[cfg(feature = "live")]
use std::time::Instant;

mod budget;
#[cfg(feature = "live")]
mod resample;
mod server_error;
#[cfg(feature = "live")]
pub(crate) mod session;
#[cfg(test)]
mod tests;
mod turn;
mod wire;

pub use budget::{
    Finalize, CONNECT_TIMEOUT_MS, FINALIZE_TIMEOUT_MS, FINALIZE_TOTAL_MS, SEND_TIMEOUT_MS,
    STREAM_FINALIZE_TIMEOUT_MS,
};
#[cfg(feature = "live")]
pub use resample::upsample_16k_to_24k;
pub use server_error::{close_error, remote_error, RemoteError};
pub use turn::Turn;
pub use wire::{
    append_message, commit_message, commit_padding, parse_server_message, realtime_url,
    sanitize_keywords, session_update, ServerEvent, CHUNK_SAMPLES, DELAY, INPUT_RATE,
    MIN_COMMIT_SAMPLES,
};

/// The provider name every error from this module carries.
const PROVIDER: &str = "openai-live";

/// Remove anything credential-shaped from `text`: the key itself, any
/// `sk-…` fragment (OpenAI quotes a masked key back in auth errors), and any
/// Bearer token. Every error leaving this module goes through here.
pub fn scrub(text: &str, api_key: &str) -> String {
    let text = if api_key.is_empty() {
        text.to_string()
    } else {
        text.replace(api_key, "<redacted>")
    };
    let text = redact_tokens(&text, "Bearer ");
    let text = redact_tokens(&text, "bearer ");
    redact_tokens(&text, "sk-")
}

/// Replace `marker` and the token after it with `<redacted>`. Only where the
/// marker starts a word, so "task-list" is left alone.
fn redact_tokens(text: &str, marker: &str) -> String {
    let token_char = |c: char| {
        c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '*' | '.' | '~' | '+' | '/' | '=')
    };
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(marker) {
        out.push_str(&rest[..at]);
        let after = &rest[at + marker.len()..];
        let starts_word = out.chars().next_back().is_none_or(|c| !c.is_alphanumeric());
        if !starts_word {
            out.push_str(marker);
            rest = after;
            continue;
        }
        out.push_str("<redacted>");
        let end = after.find(|c: char| !token_char(c)).unwrap_or(after.len());
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

fn fail(detail: impl Into<String>) -> SttError {
    SttError::Provider {
        provider: PROVIDER.to_string(),
        detail: scrub(&detail.into(), ""),
    }
}

/// Every error the adapter returns, with the key itself scrubbed out of each
/// string it carries. [`fail`] can only strip key-shaped fragments, and a key
/// for an OpenAI-compatible gateway need not look like `sk-…`.
#[cfg(any(feature = "live", test))]
fn redact(error: SttError, api_key: &str) -> SttError {
    match error {
        SttError::Http { provider, detail } => SttError::Http {
            provider,
            detail: scrub(&detail, api_key),
        },
        SttError::Auth {
            provider,
            status,
            detail,
        } => SttError::Auth {
            provider,
            status,
            detail: scrub(&detail, api_key),
        },
        SttError::Provider { provider, detail } => SttError::Provider {
            provider,
            detail: scrub(&detail, api_key),
        },
        SttError::BadAudio(detail) => SttError::BadAudio(scrub(&detail, api_key)),
        // Numbers and the fixed provider name only.
        error @ (SttError::RateLimited { .. }
        | SttError::Timeout { .. }
        | SttError::Stalled { .. }) => error,
    }
}

/// The OpenAI live adapter.
///
/// Holds the key only to build each session's request; nothing here is
/// `Debug`, so a `{self:?}`-shaped mistake cannot leak it.
// Without the `live` feature only `label` is read: the session half is gone.
#[cfg_attr(not(feature = "live"), allow(dead_code))]
pub struct OpenAiLive {
    label: String,
    url: String,
    model: String,
    api_key: String,
    bias_terms: Vec<String>,
    #[cfg(feature = "live")]
    runtime: std::sync::Arc<tokio::runtime::Runtime>,
}

impl OpenAiLive {
    pub fn new(config: &ProviderConfig) -> Result<Self, SttError> {
        let url = realtime_url(&config.base_url)?;
        if !config.bias_terms.is_empty() {
            // Counts only: terms are user content and never appear in logs.
            log::info!(
                "keyword bias: {} terms ({} after sanitizing)",
                config.bias_terms.len(),
                sanitize_keywords(&config.bias_terms).len()
            );
        }
        Ok(Self {
            label: config.label.clone(),
            url,
            model: config.model.clone(),
            api_key: config.api_key.clone(),
            bias_terms: config.bias_terms.clone(),
            #[cfg(feature = "live")]
            runtime: std::sync::Arc::new(current_thread_runtime().map_err(|e| {
                fail(format!(
                    "could not start the realtime runtime ({:?})",
                    e.kind()
                ))
            })?),
        })
    }

    #[cfg(feature = "live")]
    fn update(&self) -> Value {
        session_update(&self.model, &self.bias_terms)
    }
}

impl SttProvider for OpenAiLive {
    /// The replay path, which Test connection uses. The dictation fallback
    /// and meeting chunks take the batch REST route instead (see
    /// `hark_pipeline::batch_stt_route`). A blank transcript stays `Ok("")`
    /// here: a finished clip has nothing left to fall back to, unlike a
    /// streamed turn (see [`OpenAiLiveSession`]'s `finish`).
    #[cfg(feature = "live")]
    fn transcribe(&self, wav_bytes: &[u8]) -> Result<Transcript, SttError> {
        let info = crate::wav::parse_wav_16k_mono(wav_bytes)?;
        // Before block_on, never inside it: CPU work stays off the runtime.
        let samples = upsample_16k_to_24k(&info.samples)?;
        let started = Instant::now();
        let text = self
            .runtime
            .block_on(session::run(
                &self.url,
                &self.api_key,
                self.update(),
                samples,
                Finalize::REPLAY,
            ))
            .map_err(|e| redact(e, &self.api_key))?;
        Ok(Transcript {
            text,
            cleaned: None,
            request_ms: started.elapsed().as_millis(),
        })
    }

    #[cfg(not(feature = "live"))]
    fn transcribe(&self, _wav_bytes: &[u8]) -> Result<Transcript, SttError> {
        Err(fail(
            "this build has no OpenAI live support (built without the `live` feature)",
        ))
    }

    fn label(&self) -> &str {
        &self.label
    }
}

/// One open session, fed incrementally by the pipeline's streaming pump.
#[cfg(feature = "live")]
pub struct OpenAiLiveSession {
    runtime: std::sync::Arc<tokio::runtime::Runtime>,
    socket: Option<session::Socket>,
    /// For scrubbing errors only.
    api_key: String,
    turn: Turn,
    /// Samples short of a full 100 ms chunk, sent with the next push or at
    /// finish.
    pending: Vec<f32>,
    /// Samples appended so far, for the commit minimum and the log line.
    appended: usize,
}

#[cfg(feature = "live")]
impl crate::LiveSession for OpenAiLiveSession {
    fn push(&mut self, samples: &[f32]) -> Result<(), SttError> {
        let Some(socket) = self.socket.as_mut() else {
            return Err(fail("push after the session was finished"));
        };
        self.pending.extend_from_slice(samples);
        let ready = self.pending.len() / CHUNK_SAMPLES * CHUNK_SAMPLES;
        let chunks: Vec<f32> = self.pending.drain(..ready).collect();
        let turn = &mut self.turn;
        // One drain for the whole push, even when nothing was sent: an
        // `error` must end the session now, while the fallback has time.
        let sent = self
            .runtime
            .block_on(session::append(socket, &chunks, turn));
        if let Err(error) = sent {
            self.socket = None;
            return Err(redact(error, &self.api_key));
        }
        self.appended += chunks.len();
        Ok(())
    }

    fn finish(&mut self) -> Result<Transcript, SttError> {
        let started = Instant::now();
        let Some(mut socket) = self.socket.take() else {
            return Err(fail("the session was already finished"));
        };
        let mut tail = std::mem::take(&mut self.pending);
        tail.resize(tail.len() + commit_padding(self.appended + tail.len()), 0.0);
        let appended = self.appended + tail.len();
        let turn = &mut self.turn;
        let text = self
            .runtime
            .block_on(async {
                // No drain after the tail: the finalise loop reads anyway.
                session::send_audio(&mut socket, &tail, turn).await?;
                session::finish(&mut socket, turn, Finalize::STREAM).await
            })
            .map_err(|e| redact(e, &self.api_key))?;
        let audio_ms = appended as u64 * 1_000 / u64::from(INPUT_RATE);
        log::info!(
            "openai live session: {audio_ms} ms of audio appended, {} server frames, {} deltas",
            self.turn.frames(),
            self.turn.deltas()
        );
        if text.trim().is_empty() {
            // The clip passed the speech gate before this session was
            // finished, so blank text is a miss, not silence. Streamed audio
            // goes up at capture level, while the fallback replays the clip
            // after gain normalization, which is exactly what a quiet mic
            // needs. An `Ok("")` here would end the dictation as "empty
            // transcript" and the fallback would never run.
            return Err(fail(format!(
                "the stream returned an empty transcript for {audio_ms} ms of audio"
            )));
        }
        Ok(Transcript {
            text,
            cleaned: None,
            request_ms: started.elapsed().as_millis(),
        })
    }
}

#[cfg(feature = "live")]
impl crate::LiveStt for OpenAiLive {
    fn start_session(&self) -> Result<Box<dyn crate::LiveSession>, SttError> {
        let socket = self
            .runtime
            .block_on(session::connect(&self.url, &self.api_key, self.update()))
            .map_err(|e| redact(e, &self.api_key))?;
        Ok(Box::new(OpenAiLiveSession {
            runtime: self.runtime.clone(),
            socket: Some(socket),
            api_key: self.api_key.clone(),
            turn: Turn::default(),
            pending: Vec::new(),
            appended: 0,
        }))
    }

    fn input_rate(&self) -> u32 {
        INPUT_RATE
    }
}

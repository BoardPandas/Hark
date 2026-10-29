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

use crate::error::{json_error_detail, SttError};
#[cfg(feature = "live")]
use crate::wav::samples_to_pcm16_le;
use crate::{ProviderConfig, SttProvider, Transcript};
use serde_json::{json, Value};
#[cfg(feature = "live")]
use std::time::{Duration, Instant};

/// The only PCM rate the realtime API accepts for this model.
pub const INPUT_RATE: u32 = 24_000;

/// 100 ms at [`INPUT_RATE`]: one `input_audio_buffer.append` per chunk.
pub const CHUNK_SAMPLES: usize = 2_400;

/// The shortest buffer a commit accepts: 100 ms. Below it the server answers
/// `input_audio_buffer_commit_empty` instead of a transcript (CP0).
pub const MIN_COMMIT_SAMPLES: usize = 2_400;

/// The accuracy/latency trade-off. CP0, n=1 per value: `medium`, `high` and
/// `xhigh` reached the same 4% WER as batch gpt-transcribe; omitted, `minimal`
/// and `low` reached 9%. Commit-to-final showed no trend by delay, and Hark
/// injects only the final text, so earlier partials buy nothing.
pub const DELAY: &str = "xhigh";

/// The provider name every error from this module carries.
const PROVIDER: &str = "openai-live";

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
const _: () = assert!(CHUNK_SAMPLES == INPUT_RATE as usize / 10);
const _: () = assert!(MIN_COMMIT_SAMPLES == INPUT_RATE as usize / 10);

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

/// The realtime socket URL for an OpenAI(-compatible) HTTPS base URL:
/// `https://` becomes `wss://` (and `http://` becomes `ws://`), a trailing `/`
/// is dropped, and `/realtime?intent=transcription` is appended. No secret is
/// in it: the key rides a header.
pub fn realtime_url(base_url: &str) -> Result<String, SttError> {
    let base = base_url.trim().trim_end_matches('/');
    let scheme_end = base.find("://").map_or(0, |at| at + 3);
    let (scheme, rest) = base.split_at(scheme_end);
    let socket_scheme = match scheme.to_ascii_lowercase().as_str() {
        "https://" | "wss://" => "wss://",
        "http://" | "ws://" => "ws://",
        _ => {
            return Err(fail(
                "the OpenAI base URL must start with https:// or http://",
            ))
        }
    };
    if rest.is_empty() {
        return Err(fail("the OpenAI base URL has no host"));
    }
    Ok(format!(
        "{socket_scheme}{rest}/realtime?intent=transcription"
    ))
}

/// Make spellbook terms safe to send as `keywords`.
///
/// The API rejects the **whole** `session.update` over a single keyword that
/// spans lines or contains `<` or `>`, which would fail every dictation
/// rather than one term. So `<` and `>` are dropped, line breaks and other
/// control characters become spaces (a term split over two lines stays two
/// words), whitespace runs collapse, empty terms are dropped, and duplicates
/// keep their first position.
pub fn sanitize_keywords(terms: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(terms.len());
    for term in terms {
        let cleaned: String = term
            .chars()
            .filter(|c| !matches!(c, '<' | '>'))
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        let cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
        if !cleaned.is_empty() && !out.contains(&cleaned) {
            out.push(cleaned);
        }
    }
    out
}

/// The `session.update` that configures a transcription session.
///
/// `keywords` is omitted entirely when there are none, rather than sent empty.
/// Terms are sanitized here, not by the caller, so no path can send one the
/// API would reject the whole session over. `turn_detection` is an explicit
/// `null`: the model has no VAD, and push-to-talk decides the turn anyway.
pub fn session_update(model: &str, bias_terms: &[String]) -> Value {
    let mut transcription = json!({
        "model": model,
        "languages": ["en"],
        "delay": DELAY,
    });
    let keywords = sanitize_keywords(bias_terms);
    if !keywords.is_empty() {
        transcription["keywords"] = json!(keywords);
    }
    json!({
        "type": "session.update",
        "session": {
            "type": "transcription",
            "audio": {
                "input": {
                    "format": { "type": "audio/pcm", "rate": INPUT_RATE },
                    "transcription": transcription,
                    "turn_detection": null,
                }
            }
        }
    })
}

/// One `input_audio_buffer.append`: base64 PCM16 LE mono at [`INPUT_RATE`].
pub fn append_message(pcm_le_bytes: &[u8]) -> Value {
    use base64::Engine;
    json!({
        "type": "input_audio_buffer.append",
        "audio": base64::engine::general_purpose::STANDARD.encode(pcm_le_bytes),
    })
}

/// End the turn: the key came up.
pub fn commit_message() -> Value {
    json!({ "type": "input_audio_buffer.commit" })
}

/// Zero samples to append before the commit so it carries at least 100 ms.
///
/// A hold under 100 ms after pre-roll and tail would otherwise fail the commit
/// with `input_audio_buffer_commit_empty`. Padded, the server answers `""`,
/// which injects nothing.
pub fn commit_padding(appended: usize) -> usize {
    MIN_COMMIT_SAMPLES.saturating_sub(appended)
}

/// A server error, reduced to the fields that are safe to keep.
///
/// The free-text `message` is never read: OpenAI quotes a masked key in it.
/// `code`, `type` and `param` are kept only when they are slug-shaped
/// (lowercase ASCII, digits, `_`, `.`, `[`, `]`), which every documented value
/// is and no key or transcript text would be.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoteError {
    pub code: Option<String>,
    pub kind: Option<String>,
    pub param: Option<String>,
}

impl RemoteError {
    fn from_value(error: Option<&Value>) -> Self {
        let field = |name: &str| {
            error
                .and_then(|e| e.get(name))
                .and_then(Value::as_str)
                .and_then(safe_slug)
        };
        RemoteError {
            code: field("code"),
            kind: field("type"),
            param: field("param"),
        }
    }

    fn describe(&self) -> String {
        let mut out = self
            .code
            .clone()
            .or_else(|| self.kind.clone())
            .unwrap_or_else(|| "no error code".to_string());
        if self.code.is_some() {
            if let Some(kind) = &self.kind {
                out.push_str(&format!(" ({kind})"));
            }
        }
        if let Some(param) = &self.param {
            out.push_str(&format!(" at {param}"));
        }
        out
    }
}

/// `value`, if it is a protocol identifier rather than free text.
fn safe_slug(value: &str) -> Option<String> {
    let value = value.trim();
    let ok = (1..=96).contains(&value.len())
        && value.chars().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '[' | ']')
        });
    ok.then(|| value.to_string())
}

/// Map a server error onto the taxonomy the HTTP adapters use.
///
/// | code / type | error |
/// |---|---|
/// | `invalid_api_key`, type `authentication_error` | `Auth`, 401 |
/// | `insufficient_quota`, `account_deactivated`, `organization_deactivated`, `permission_denied`, `access_denied`, type `permission_error` | `Auth`, 403 |
/// | `rate_limit_exceeded`, type `rate_limit_error` | `RateLimited` |
/// | anything else | `Provider`, with the slug-shaped code, type and param |
///
/// A socket refusal has no HTTP status, so `Auth` carries the status OpenAI's
/// REST API uses for the same refusal (close code 3000 is the WebSocket
/// registry's "Unauthorized", 3003 its "Forbidden"). 403 keeps the meaning
/// `SttError::Auth` documents — quota, model access or org policy, not a bad
/// key — so a quota problem is not reported as one that waiting fixes.
pub fn remote_error(context: &str, error: &RemoteError) -> SttError {
    let code = error.code.as_deref();
    let kind = error.kind.as_deref();
    let auth = |status: u16, slug: &str| SttError::Auth {
        provider: PROVIDER.to_string(),
        status,
        detail: slug.to_string(),
    };
    match (code, kind) {
        (Some(slug @ "invalid_api_key"), _) => auth(401, slug),
        (
            Some(
                slug @ ("insufficient_quota"
                | "account_deactivated"
                | "organization_deactivated"
                | "permission_denied"
                | "access_denied"),
            ),
            _,
        ) => auth(403, slug),
        (Some("rate_limit_exceeded"), _) | (None, Some("rate_limit_error")) => {
            SttError::RateLimited {
                provider: PROVIDER.to_string(),
                retry_after_s: None,
            }
        }
        (None, Some(slug @ "authentication_error")) => auth(401, slug),
        (None, Some(slug @ "permission_error")) => auth(403, slug),
        _ => fail(format!("{context}: {}", error.describe())),
    }
}

/// The error for a close frame that arrived before the transcript.
///
/// 3000 and 3003 are how OpenAI closes over a refused key or forbidden
/// access; their reason (`invalid_request_error.invalid_api_key`) is kept only
/// as its slug-shaped last segment. Any other reason is free text and dropped.
pub fn close_error(code: u16, reason: &str, phase: &str) -> SttError {
    let slug = || {
        reason
            .rsplit('.')
            .next()
            .and_then(safe_slug)
            .unwrap_or_default()
    };
    match code {
        3000 => SttError::Auth {
            provider: PROVIDER.to_string(),
            status: 401,
            detail: slug(),
        },
        3003 => SttError::Auth {
            provider: PROVIDER.to_string(),
            status: 403,
            detail: slug(),
        },
        _ => fail(format!(
            "the session closed {phase} (WebSocket close code {code})"
        )),
    }
}

/// What one server frame said.
#[derive(PartialEq)]
pub enum ServerEvent {
    /// Sent unprompted on connect.
    SessionCreated,
    /// The `session.update` was accepted. Never waited for (D10): audio goes
    /// out straight after the update, and an `error` fails the session instead.
    SessionUpdated,
    /// The commit created this turn's item.
    Committed { item_id: String },
    /// A partial transcript. Hark injects once, on release, so it is counted
    /// and discarded.
    Delta,
    /// A final transcript, for the named item.
    Completed { item_id: String, transcript: String },
    /// Transcription of the named item failed.
    Failed { item_id: String, error: RemoteError },
    /// A session-level error. Every one fails the session.
    Error(RemoteError),
    /// A known event Hark has no use for, or an unknown one (logged by type).
    Other,
}

// Hand-written so a reflexive `{event:?}` cannot put transcript text in a log.
impl std::fmt::Debug for ServerEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServerEvent::SessionCreated => f.write_str("SessionCreated"),
            ServerEvent::SessionUpdated => f.write_str("SessionUpdated"),
            ServerEvent::Committed { item_id } => f
                .debug_struct("Committed")
                .field("item_id", item_id)
                .finish(),
            ServerEvent::Delta => f.write_str("Delta"),
            ServerEvent::Completed {
                item_id,
                transcript,
            } => f
                .debug_struct("Completed")
                .field("item_id", item_id)
                .field("transcript_chars", &transcript.chars().count())
                .finish(),
            ServerEvent::Failed { item_id, error } => f
                .debug_struct("Failed")
                .field("item_id", item_id)
                .field("error", error)
                .finish(),
            ServerEvent::Error(error) => f.debug_tuple("Error").field(error).finish(),
            ServerEvent::Other => f.write_str("Other"),
        }
    }
}

/// Events Hark receives and deliberately does nothing with.
const IGNORED_EVENTS: &[&str] = &[
    "conversation.item.added",
    "conversation.item.created",
    "conversation.item.done",
    "conversation.item.input_audio_transcription.segment",
    "input_audio_buffer.cleared",
    "input_audio_buffer.speech_started",
    "input_audio_buffer.speech_stopped",
    "rate_limits.updated",
];

/// Classify one server frame. Pure, so the protocol is testable offline.
pub fn parse_server_message(body: &str) -> Result<ServerEvent, SttError> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| fail(json_error_detail("server frame was not JSON", &e)))?;
    let Some(kind) = v.get("type").and_then(Value::as_str) else {
        log::debug!("openai live: a server frame with no event type");
        return Ok(ServerEvent::Other);
    };
    let text = |field: &str| {
        v.get(field)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| fail(format!("{kind} event had no {field}")))
    };
    Ok(match kind {
        "session.created" => ServerEvent::SessionCreated,
        "session.updated" => ServerEvent::SessionUpdated,
        "input_audio_buffer.committed" => ServerEvent::Committed {
            item_id: text("item_id")?,
        },
        "conversation.item.input_audio_transcription.delta" => ServerEvent::Delta,
        "conversation.item.input_audio_transcription.completed" => ServerEvent::Completed {
            item_id: text("item_id")?,
            transcript: text("transcript")?,
        },
        "conversation.item.input_audio_transcription.failed" => ServerEvent::Failed {
            item_id: text("item_id")?,
            error: RemoteError::from_value(v.get("error")),
        },
        "error" => ServerEvent::Error(RemoteError::from_value(v.get("error"))),
        ignored if IGNORED_EVENTS.contains(&ignored) => ServerEvent::Other,
        unknown => {
            // Never a silent skip: this is how a protocol change turns into an
            // unexplained timeout. The type only, and only if it looks like
            // one; the frame itself may carry transcript text.
            let shown = safe_slug(unknown).unwrap_or_else(|| "<not an event name>".into());
            log::debug!("openai live: unhandled event type {shown}");
            ServerEvent::Other
        }
    })
}

/// How many pre-commit outcomes [`Turn`] keeps. A single-turn session with no
/// VAD never has another item, so this only bounds a misbehaving server.
const EARLY_OUTCOMES_MAX: usize = 8;

/// One turn's progress, fed every frame the server sends.
///
/// The single place a transcript is collected: the drain that runs during the
/// hold and the loop that finalises the turn both go through [`Turn::absorb`],
/// because Gemini Live once lost every word before the last pause to one read
/// path keeping frames and the other dropping them.
#[derive(Default)]
pub struct Turn {
    /// Whether our own `input_audio_buffer.commit` has gone out. A
    /// `committed` before it answers something else (a server-side commit),
    /// and taking that item as ours would return its phrase as the dictation.
    commit_sent: bool,
    /// This turn's item, from the first `input_audio_buffer.committed` after
    /// our commit.
    item_id: Option<String>,
    /// Outcomes that named an item before `committed` said which one is ours.
    /// The server sends `committed` first (CP0), but transcription is
    /// asynchronous to the other events, so this does not rely on it.
    early: Vec<(String, Result<String, RemoteError>)>,
    transcript: Option<String>,
    configured: bool,
    frames: u32,
    deltas: u32,
}

impl Turn {
    /// Record one frame. `Ok(true)` once this turn's transcript has arrived;
    /// an `error` event, or a failure for this turn's item, is an `Err`.
    pub fn absorb(&mut self, body: &str) -> Result<bool, SttError> {
        self.frames += 1;
        match parse_server_message(body)? {
            ServerEvent::SessionCreated | ServerEvent::Other => {}
            ServerEvent::SessionUpdated => self.configured = true,
            ServerEvent::Delta => self.deltas += 1,
            ServerEvent::Committed { item_id } => {
                if !self.commit_sent {
                    // `turn_detection` is null, so the server should never
                    // commit on its own. If it ever does, that item holds only
                    // the audio before it: accepting it would inject the first
                    // phrase as if it were the whole dictation.
                    log::warn!(
                        "openai live: the server committed audio before our commit; \
                         ignoring that item"
                    );
                } else if self.item_id.is_some() {
                    log::debug!("openai live: a second commit acknowledgement; keeping the first");
                } else {
                    if let Some(at) = self.early.iter().position(|(id, _)| *id == item_id) {
                        let (_, outcome) = self.early.swap_remove(at);
                        self.settle(outcome)?;
                    }
                    self.item_id = Some(item_id);
                }
            }
            ServerEvent::Completed {
                item_id,
                transcript,
            } => self.outcome(item_id, Ok(transcript))?,
            ServerEvent::Failed { item_id, error } => self.outcome(item_id, Err(error))?,
            ServerEvent::Error(error) => {
                let context = if self.configured {
                    "server error"
                } else {
                    "server error before the session was configured"
                };
                return Err(remote_error(context, &error));
            }
        }
        Ok(self.transcript.is_some())
    }

    fn outcome(
        &mut self,
        item_id: String,
        outcome: Result<String, RemoteError>,
    ) -> Result<(), SttError> {
        match &self.item_id {
            Some(ours) if *ours == item_id => self.settle(outcome),
            Some(_) => {
                log::debug!("openai live: ignoring an outcome for another item");
                Ok(())
            }
            None => {
                if self.early.len() == EARLY_OUTCOMES_MAX {
                    drop(self.early.remove(0));
                }
                self.early.push((item_id, outcome));
                Ok(())
            }
        }
    }

    fn settle(&mut self, outcome: Result<String, RemoteError>) -> Result<(), SttError> {
        match outcome {
            Ok(text) => {
                self.transcript = Some(text);
                Ok(())
            }
            Err(error) => Err(remote_error("transcription failed", &error)),
        }
    }

    /// Record that our commit has been sent: only a `committed` from here on
    /// can name this turn's item.
    pub fn sent_commit(&mut self) {
        self.commit_sent = true;
    }

    /// This turn's transcript, once it has arrived.
    pub fn take_transcript(&mut self) -> Option<String> {
        self.transcript.take()
    }

    /// Whether `committed` has named this turn's item yet.
    pub fn committed(&self) -> bool {
        self.item_id.is_some()
    }

    /// Whether `session.updated` has arrived.
    pub fn configured(&self) -> bool {
        self.configured
    }

    /// Server frames seen so far, for diagnostics.
    pub fn frames(&self) -> u32 {
        self.frames
    }

    /// Partial transcripts seen so far, for diagnostics.
    pub fn deltas(&self) -> u32 {
        self.deltas
    }
}

/// Upsample a 16 kHz clip to [`INPUT_RATE`] for a replay.
///
/// One whole-clip `process_all`, which trims the FFT startup delay once and
/// returns exactly `ceil(len * 1.5)` samples (LL-G Rust HIGH,
/// `rubato-4-whole-clip-process-all`). Same configuration as hark-audio's
/// resampler, which this crate does not depend on.
#[cfg(feature = "live")]
pub fn upsample_16k_to_24k(samples: &[f32]) -> Result<Vec<f32>, SttError> {
    use rubato::audioadapter_buffers::direct::InterleavedSlice;
    use rubato::{Fft, FixedSync, Resampler};

    if samples.is_empty() {
        return Ok(Vec::new());
    }
    let bad = |what: &str, e: &dyn std::fmt::Display| {
        SttError::BadAudio(format!(
            "could not resample the clip to 24 kHz: {what} ({e})"
        ))
    };
    let mut resampler = Fft::<f32>::new(
        crate::wav::SAMPLE_RATE as usize,
        INPUT_RATE as usize,
        1024,
        1,
        FixedSync::Both,
    )
    .map_err(|e| bad("resampler", &e))?;
    let input = InterleavedSlice::new(samples, 1, samples.len()).map_err(|e| bad("input", &e))?;
    let output = resampler
        .process_all(&input, samples.len(), None)
        .map_err(|e| bad("process", &e))?;
    Ok(output.take_data())
}

#[cfg(feature = "live")]
pub(crate) mod session {
    use super::*;
    use crate::gemini_live::session::frame_json;
    use futures_util::{FutureExt, SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::http::{header, HeaderValue, Request};
    use tokio_tungstenite::tungstenite::{Error as WsError, Message};
    use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

    pub type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

    // WebSocket errors can carry frame contents, URLs or header values. Keep
    // the category and the status, never their Display or Debug.
    fn socket_error(operation: &str, error: &WsError) -> SttError {
        let category = match error {
            WsError::ConnectionClosed | WsError::AlreadyClosed => "connection closed",
            WsError::Io(error) => return fail(format!("{operation}: I/O {:?}", error.kind())),
            WsError::Tls(_) => "TLS failure",
            WsError::Capacity(_) | WsError::WriteBufferFull(_) => "capacity exceeded",
            WsError::Protocol(_) | WsError::AttackAttempt => "protocol error",
            WsError::Utf8(_) => "invalid UTF-8",
            WsError::Url(_) => "invalid URL",
            // A refused upgrade: the status, plus only the whitelisted reason
            // slugs `error_for_status` takes out of OpenAI's JSON body.
            WsError::Http(response) => {
                let body = response
                    .body()
                    .as_deref()
                    .map(String::from_utf8_lossy)
                    .unwrap_or_default();
                return crate::error::error_for_status(
                    PROVIDER,
                    response.status().as_u16(),
                    crate::openai_compatible::retry_after_secs(response.headers()),
                    &body,
                );
            }
            WsError::HttpFormat(_) => "invalid HTTP format",
        };
        fail(format!("{operation}: {category}"))
    }

    /// The upgrade request, with the key in a header marked sensitive and no
    /// `OpenAI-Beta` header (that selects the pre-GA interface).
    pub fn authorized_request(url: &str, api_key: &str) -> Result<Request<()>, SttError> {
        let mut request = url
            .into_client_request()
            .map_err(|e| socket_error("invalid socket URL", &e))?;
        let mut auth = HeaderValue::from_str(&format!("Bearer {api_key}"))
            .map_err(|_| fail("the API key is not a valid HTTP header value"))?;
        auth.set_sensitive(true);
        request.headers_mut().insert(header::AUTHORIZATION, auth);
        Ok(request)
    }

    /// Send one JSON message, bounded by [`SEND_TIMEOUT_MS`].
    pub async fn send_json(socket: &mut Socket, value: Value, what: &str) -> Result<(), SttError> {
        tokio::time::timeout(
            Duration::from_millis(SEND_TIMEOUT_MS),
            socket.send(Message::Text(value.to_string().into())),
        )
        .await
        .map_err(|_| SttError::Timeout {
            provider: PROVIDER.to_string(),
            configured_ms: SEND_TIMEOUT_MS,
        })?
        .map_err(|e| socket_error(&format!("{what} send failed"), &e))
    }

    /// Open the socket and send `session.update`.
    ///
    /// Does **not** wait for `session.updated` (D10). Client events on one
    /// connection are processed in order, and CP0 measured the ack at about
    /// 420 ms, which a short hold would otherwise pay at release. An `error`
    /// that arrives instead fails the session from the next read, and the
    /// pipeline replays the finished clip.
    pub async fn connect(url: &str, api_key: &str, update: Value) -> Result<Socket, SttError> {
        let request = authorized_request(url, api_key)?;
        let (mut socket, _) = tokio::time::timeout(
            Duration::from_millis(CONNECT_TIMEOUT_MS),
            tokio_tungstenite::connect_async(request),
        )
        .await
        .map_err(|_| SttError::Timeout {
            provider: PROVIDER.to_string(),
            configured_ms: CONNECT_TIMEOUT_MS,
        })?
        .map_err(|e| socket_error("websocket connect failed", &e))?;
        send_json(&mut socket, update, "session.update").await?;
        Ok(socket)
    }

    /// Feed one frame to the turn. `Ok(true)` once the transcript is in.
    fn read_frame(frame: Message, turn: &mut Turn, phase: &str) -> Result<bool, SttError> {
        if let Some(body) = frame_json(&frame) {
            return turn.absorb(&body);
        }
        match frame {
            Message::Close(Some(frame)) => Err(close_error(
                u16::from(frame.code),
                frame.reason.as_str(),
                phase,
            )),
            Message::Close(None) => Err(fail(format!("the session closed {phase}"))),
            // tungstenite answers pings itself.
            Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => Ok(false),
            // `frame_json` declines only a binary frame that is not UTF-8.
            Message::Text(_) | Message::Binary(_) => {
                Err(fail("the server sent a binary frame that is not UTF-8"))
            }
        }
    }

    /// The most frames one drain takes, so a chatty server cannot turn a
    /// drain into a stall.
    const DRAIN_MAX_FRAMES: usize = 64;

    /// Let the runtime's driver look at the socket once, without a timer.
    ///
    /// `yield_now` hands the waker to the current_thread scheduler's defer
    /// list, so `block_on` takes its `park_yield` path: one I/O driver turn
    /// with a zero timeout, then the deferred waker fires and this future is
    /// polled again (tokio 1.53, `runtime/scheduler/current_thread`). That
    /// turn is what moves readiness the kernel already has into tokio, so the
    /// `now_or_never` reads after it can see frames that landed while the
    /// worker was outside the runtime.
    async fn turn_driver() {
        tokio::task::yield_now().await;
    }

    /// Take whatever the server has already sent, without waiting for more.
    ///
    /// Reading while writing keeps the receive side moving: deltas stream
    /// throughout the hold (CP0, about 30 per turn), and a client that only
    /// writes lets them back up. It is also where an `error` sent in answer to
    /// `session.update` surfaces, so a refused session ends mid-hold rather
    /// than at release.
    ///
    /// # Why not `timeout(Duration::ZERO, socket.next())`
    ///
    /// That was the first version, and on Windows it cost about 15 ms a call.
    /// A zero deadline is still a timer: tokio rounds every deadline up to the
    /// next 1 ms tick (`deadline_to_tick`), so whenever nothing is buffered —
    /// the usual case — the runtime parks until that tick, and Windows' ~15.6
    /// ms default timer resolution stretches the 1 ms to a whole scheduler
    /// quantum. At one drain per 100 ms chunk a 10 s replay lost up to ~1.5 s,
    /// and every release paid 50-80 ms. [`turn_driver`] plus `now_or_never`
    /// never arms a timer at all; `an_empty_drain_never_waits_on_a_timer` and
    /// `a_drain_sees_frames_that_landed_outside_the_runtime` pin both halves.
    pub async fn drain(socket: &mut Socket, turn: &mut Turn) -> Result<(), SttError> {
        turn_driver().await;
        for _ in 0..DRAIN_MAX_FRAMES {
            let frame = match socket.next().now_or_never() {
                None => return Ok(()),
                Some(None) => return Err(fail("the session ended mid-stream")),
                Some(Some(Err(e))) => return Err(socket_error("socket read failed", &e)),
                Some(Some(Ok(frame))) => frame,
            };
            read_frame(frame, turn, "mid-stream")?;
        }
        Ok(())
    }

    /// The server's own account of a session a send just failed on, if it
    /// gave one: an `error` event, a failed item, or a close frame.
    ///
    /// D10 appends without waiting for `session.updated`, so a refused key's
    /// `error` event and close 3000 race the appends. When the server hangs
    /// up first, the next write fails with a bare I/O error or "connection
    /// closed" while the real reason sits unread in the receive buffer, and a
    /// bad key would read as a network fault. A read error or the end of the
    /// stream says nothing the send error did not, so those leave it standing.
    async fn server_reason(socket: &mut Socket, turn: &mut Turn) -> Option<SttError> {
        turn_driver().await;
        for _ in 0..DRAIN_MAX_FRAMES {
            let Some(Some(Ok(frame))) = socket.next().now_or_never() else {
                return None;
            };
            if let Err(reason) = read_frame(frame, turn, "mid-stream") {
                return Some(reason);
            }
        }
        None
    }

    /// `send_error`, unless the server has already said why the session
    /// ended, in which case that (an `Auth 401` for a refused key, say).
    pub async fn explain_send_failure(
        socket: &mut Socket,
        turn: &mut Turn,
        send_error: SttError,
    ) -> SttError {
        server_reason(socket, turn).await.unwrap_or(send_error)
    }

    /// [`send_json`], with a failure explained by the server where it can be.
    async fn send_or_explain(
        socket: &mut Socket,
        turn: &mut Turn,
        value: Value,
        what: &str,
    ) -> Result<(), SttError> {
        match send_json(socket, value, what).await {
            Ok(()) => Ok(()),
            Err(error) => Err(explain_send_failure(socket, turn, error).await),
        }
    }

    /// Send samples at [`INPUT_RATE`] as 100 ms appends, without reading.
    pub async fn send_audio(
        socket: &mut Socket,
        samples: &[f32],
        turn: &mut Turn,
    ) -> Result<(), SttError> {
        for chunk in samples.chunks(CHUNK_SAMPLES) {
            let message = append_message(&samples_to_pcm16_le(chunk));
            send_or_explain(socket, turn, message, "append").await?;
        }
        Ok(())
    }

    /// [`send_audio`], then one [`drain`]: once per call, not once per chunk.
    /// The drain runs even when there was nothing to send, so an `error`
    /// ends the session while the fallback still has time.
    pub async fn append(
        socket: &mut Socket,
        samples: &[f32],
        turn: &mut Turn,
    ) -> Result<(), SttError> {
        send_audio(socket, samples, turn).await?;
        drain(socket, turn).await
    }

    /// Commit the turn and read until its transcript arrives.
    ///
    /// The caller has already appended at least [`MIN_COMMIT_SAMPLES`].
    pub async fn finish(
        socket: &mut Socket,
        turn: &mut Turn,
        budget: Finalize,
    ) -> Result<String, SttError> {
        send_or_explain(socket, turn, commit_message(), "commit").await?;
        // Marked after the send: nothing is read in between, so a
        // `committed` absorbed from here on was read once the server had our
        // commit, or was already on the wire. The pump drains every 20 ms
        // and the replay after every chunk, so that window is a frame or two
        // wide, and only a server-side commit could land in it.
        turn.sent_commit();
        let started = Instant::now();
        let total = Duration::from_millis(budget.total_ms);
        let per_frame = Duration::from_millis(budget.per_frame_ms);
        let stalled = |turn: &Turn, waited_ms: u64| {
            // A bare timeout cannot tell "the commit never landed" from "still
            // transcribing"; the counts can. Counts only, never text.
            log::warn!(
                "openai live finalise stalled after {waited_ms} ms: {} frames, {} deltas, \
                 committed: {}, configured: {}",
                turn.frames(),
                turn.deltas(),
                turn.committed(),
                turn.configured()
            );
            SttError::Stalled {
                provider: PROVIDER.to_string(),
                waited_ms,
            }
        };
        let text = loop {
            if let Some(text) = turn.take_transcript() {
                break text;
            }
            let remaining = match total.checked_sub(started.elapsed()) {
                Some(left) if !left.is_zero() => left.min(per_frame),
                _ => return Err(stalled(turn, budget.total_ms)),
            };
            let Ok(frame) = tokio::time::timeout(remaining, socket.next()).await else {
                let waited = if remaining < per_frame {
                    budget.total_ms
                } else {
                    budget.per_frame_ms
                };
                return Err(stalled(turn, waited));
            };
            let Some(frame) = frame else {
                return Err(fail(format!(
                    "the session ended before the transcript arrived ({} frames, committed: {})",
                    turn.frames(),
                    turn.committed()
                )));
            };
            let frame = frame.map_err(|e| socket_error("socket read failed", &e))?;
            read_frame(frame, turn, "before the transcript arrived")?;
        };
        if !turn.configured() {
            // In order on one connection, so the ack precedes the commit's
            // answer. Its absence is worth knowing about, not worth failing.
            log::warn!("openai live: transcript arrived without session.updated");
        }
        // Bounded like every other write: a close to a peer that has stopped
        // reading must not outlive the dictation it belongs to.
        let _ =
            tokio::time::timeout(Duration::from_millis(SEND_TIMEOUT_MS), socket.close(None)).await;
        Ok(text)
    }

    /// One complete session from a finished clip at [`INPUT_RATE`].
    pub async fn run(
        url: &str,
        api_key: &str,
        update: Value,
        mut samples: Vec<f32>,
        budget: Finalize,
    ) -> Result<String, SttError> {
        let mut socket = connect(url, api_key, update).await?;
        let mut turn = Turn::default();
        samples.resize(samples.len() + commit_padding(samples.len()), 0.0);
        // A replay is a burst, not a hold, so it drains after every chunk: a
        // write-only burst can back the server's output up (LL-G Rust HIGH,
        // `websocket-binary-frames-not-text`). A drain is one timer-free
        // driver turn, so this costs microseconds per chunk.
        for chunk in samples.chunks(CHUNK_SAMPLES) {
            append(&mut socket, chunk, &mut turn).await?;
        }
        finish(&mut socket, &mut turn, budget).await
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use base64::Engine;
        use std::sync::{Arc, Mutex};
        use tokio_tungstenite::tungstenite::handshake::server::{
            Request as ServerRequest, Response,
        };
        use tokio_tungstenite::tungstenite::protocol::{
            frame::coding::CloseCode, CloseFrame, Role,
        };

        const KEY: &str = "sk-proj-hark-test-SECRET";
        const PRIVATE: &str = "PRIVATE_transcript_and_key";

        fn rendered(error: &SttError) -> String {
            format!("{error} {error:?}")
        }

        fn assert_private(error: &SttError) {
            let text = rendered(error);
            for secret in [KEY, PRIVATE, "Bearer", "sk-"] {
                assert!(!text.contains(secret), "{secret} leaked: {text}");
            }
        }

        fn event(value: Value) -> Message {
            Message::Text(value.to_string().into())
        }

        fn committed(item_id: &str) -> Value {
            json!({"type": "input_audio_buffer.committed", "item_id": item_id, "previous_item_id": null})
        }

        fn completed(item_id: &str, transcript: &str) -> Value {
            json!({
                "type": "conversation.item.input_audio_transcription.completed",
                "item_id": item_id,
                "content_index": 0,
                "transcript": transcript,
                "usage": {"type": "duration", "seconds": 1}
            })
        }

        fn runtime() -> tokio::runtime::Runtime {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
        }

        /// Exercise the real finalizer against a local WebSocket peer. No TLS,
        /// key or external service is involved. The peer reads the commit,
        /// sends `frames`, then reads until the client goes away and returns
        /// whether a close frame arrived.
        fn finish_from_peer(
            frames: Vec<Message>,
            budget: Finalize,
        ) -> (Result<String, SttError>, bool) {
            finish_against(frames, budget, false)
        }

        /// As [`finish_from_peer`]; with `hang_up` the peer drops the TCP
        /// connection straight after its frames instead.
        fn finish_against(
            frames: Vec<Message>,
            budget: Finalize,
            hang_up: bool,
        ) -> (Result<String, SttError>, bool) {
            runtime().block_on(async {
                tokio::time::timeout(Duration::from_secs(5), async {
                    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                    let address = listener.local_addr().unwrap();
                    let peer = tokio::spawn(async move {
                        let (stream, _) = listener.accept().await.unwrap();
                        let mut socket =
                            WebSocketStream::from_raw_socket(stream, Role::Server, None).await;
                        let frame = socket.next().await.unwrap().unwrap();
                        let actual: Value =
                            serde_json::from_str(&frame_json(&frame).unwrap()).unwrap();
                        assert_eq!(actual, commit_message());
                        for frame in frames {
                            // The client may already have given up (a stall
                            // test); that is its answer, not a peer failure.
                            if socket.send(frame).await.is_err() {
                                return false;
                            }
                        }
                        if hang_up {
                            return false;
                        }
                        let mut closed = false;
                        while let Some(Ok(frame)) = socket.next().await {
                            closed |= matches!(frame, Message::Close(_));
                        }
                        closed
                    });
                    let stream = tokio::net::TcpStream::connect(address).await.unwrap();
                    let mut socket = WebSocketStream::from_raw_socket(
                        MaybeTlsStream::Plain(stream),
                        Role::Client,
                        None,
                    )
                    .await;
                    let mut turn = Turn::default();
                    let result = finish(&mut socket, &mut turn, budget).await;
                    drop(socket);
                    (result, peer.await.unwrap())
                })
                .await
                .expect("local WebSocket finalization must terminate")
            })
        }

        #[test]
        fn the_transcript_for_the_committed_item_finishes_the_turn() {
            let (result, closed) = finish_from_peer(
                vec![
                    event(json!({"type": "session.updated", "session": {}})),
                    event(
                        json!({"type": "conversation.item.input_audio_transcription.delta", "item_id": "item_1", "delta": "hel"}),
                    ),
                    event(committed("item_1")),
                    event(json!({"type": "conversation.item.added", "item": {}})),
                    event(json!({"type": "conversation.item.done", "item": {}})),
                    event(completed("item_1", "Hello there.")),
                ],
                Finalize::STREAM,
            );
            assert_eq!(result.unwrap(), "Hello there.");
            assert!(closed, "a finished session closes its socket");
        }

        #[test]
        fn another_items_completion_is_ignored() {
            let (result, _) = finish_from_peer(
                vec![
                    event(committed("item_ours")),
                    event(completed("item_other", "not what was said")),
                    event(completed("item_ours", "What was said.")),
                ],
                Finalize::STREAM,
            );
            assert_eq!(result.unwrap(), "What was said.");
        }

        #[test]
        fn binary_frames_carry_the_protocol_too() {
            let binary = |value: Value| Message::Binary(value.to_string().into_bytes().into());
            let (result, _) = finish_from_peer(
                vec![
                    binary(committed("item_1")),
                    binary(completed("item_1", "Binary works.")),
                ],
                Finalize::STREAM,
            );
            assert_eq!(result.unwrap(), "Binary works.");
        }

        #[test]
        fn a_close_before_the_transcript_fails_without_its_reason() {
            for code in [CloseCode::Normal, CloseCode::Error, CloseCode::Policy] {
                let (result, _) = finish_from_peer(
                    vec![
                        event(committed("item_1")),
                        Message::Close(Some(CloseFrame {
                            code,
                            reason: PRIVATE.into(),
                        })),
                    ],
                    Finalize::STREAM,
                );
                let error = result.unwrap_err();
                assert!(error.to_string().contains(&u16::from(code).to_string()));
                assert_private(&error);
            }
        }

        #[test]
        fn close_code_3000_is_an_auth_refusal() {
            let (result, _) = finish_from_peer(
                vec![Message::Close(Some(CloseFrame {
                    code: CloseCode::from(3000),
                    reason: "invalid_request_error.invalid_api_key".into(),
                }))],
                Finalize::STREAM,
            );
            match result.unwrap_err() {
                SttError::Auth { status, detail, .. } => {
                    assert_eq!(status, 401);
                    assert_eq!(detail, "invalid_api_key");
                }
                other => panic!("expected Auth, got {other}"),
            }
        }

        #[test]
        fn an_empty_close_or_tcp_eof_cannot_finish_the_turn() {
            let (result, _) = finish_from_peer(vec![Message::Close(None)], Finalize::STREAM);
            assert!(result.is_err());
            let (result, _) =
                finish_against(vec![event(committed("item_1"))], Finalize::STREAM, true);
            // The end of the stream, reported as such rather than waited out.
            assert!(
                matches!(result, Err(SttError::Provider { .. })),
                "{result:?}"
            );
        }

        #[test]
        fn a_failed_transcription_for_this_item_is_an_error() {
            let (result, _) = finish_from_peer(
                vec![
                    event(committed("item_1")),
                    event(json!({
                        "type": "conversation.item.input_audio_transcription.failed",
                        "item_id": "item_1",
                        "error": {"code": "audio_unintelligible", "message": PRIVATE}
                    })),
                ],
                Finalize::STREAM,
            );
            let error = result.unwrap_err();
            assert!(
                error.to_string().contains("audio_unintelligible"),
                "{error}"
            );
            assert_private(&error);
        }

        #[test]
        fn a_silent_server_is_a_stall_not_a_hang() {
            let budget = Finalize {
                per_frame_ms: 30,
                total_ms: 200,
            };
            let (result, _) = finish_from_peer(vec![event(committed("item_1"))], budget);
            assert!(matches!(
                result,
                Err(SttError::Stalled { waited_ms: 30, .. })
            ));

            // The ceiling holds even where the per-frame wait would allow more.
            let deltas = (0..40)
                .map(|_| event(json!({"type": "conversation.item.input_audio_transcription.delta", "item_id": "item_1", "delta": "x"})))
                .collect::<Vec<_>>();
            let budget = Finalize {
                per_frame_ms: 1_000,
                total_ms: 0,
            };
            let (result, _) = finish_from_peer(deltas, budget);
            assert!(matches!(
                result,
                Err(SttError::Stalled { waited_ms: 0, .. })
            ));
        }

        #[test]
        fn socket_errors_never_expose_remote_content() {
            let errors = [
                WsError::Io(std::io::Error::other(PRIVATE)),
                WsError::Utf8(PRIVATE.to_string()),
                WsError::WriteBufferFull(Box::new(Message::Text(PRIVATE.into()))),
            ];
            for error in errors {
                assert_private(&socket_error("socket read failed", &error));
            }
        }

        #[test]
        fn a_refused_upgrade_maps_by_status_without_echoing_the_body() {
            let body = format!(
                r#"{{"error":{{"message":"Incorrect API key provided: sk-proj-****CRET. {PRIVATE}","code":"invalid_api_key","type":"invalid_request_error"}}}}"#
            );
            let refused = |status: u16| {
                WsError::Http(Box::new(
                    tokio_tungstenite::tungstenite::http::Response::builder()
                        .status(status)
                        .header("retry-after", "7")
                        .body(Some(body.clone().into_bytes()))
                        .unwrap(),
                ))
            };
            match socket_error("connect", &refused(401)) {
                SttError::Auth {
                    status, ref detail, ..
                } => {
                    assert_eq!(status, 401);
                    assert_eq!(detail, "invalid_api_key");
                }
                other => panic!("expected Auth, got {other}"),
            }
            assert!(matches!(
                socket_error("connect", &refused(429)),
                SttError::RateLimited {
                    retry_after_s: Some(7),
                    ..
                }
            ));
            for status in [401, 403, 429, 500] {
                assert_private(&socket_error("connect", &refused(status)));
            }
        }

        #[test]
        fn the_request_carries_a_sensitive_bearer_header_and_no_beta_header() {
            let request =
                authorized_request("wss://api.openai.com/v1/realtime?intent=transcription", KEY)
                    .unwrap();
            let auth = request.headers().get(header::AUTHORIZATION).unwrap();
            assert_eq!(auth.to_str().unwrap(), format!("Bearer {KEY}"));
            assert!(auth.is_sensitive());
            assert!(!format!("{:?}", request.headers()).contains(KEY));
            assert!(request.headers().get("openai-beta").is_none());
            assert_eq!(request.uri().query(), Some("intent=transcription"));
        }

        #[test]
        fn a_key_that_cannot_be_a_header_fails_without_echoing_it() {
            let error = authorized_request("wss://api.openai.com/v1/realtime", "sk-bad\nSECRET")
                .unwrap_err();
            assert!(!rendered(&error).contains("SECRET"));
        }

        /// What the local peer saw of one replayed session.
        #[derive(Default)]
        struct Seen {
            authorization: Option<String>,
            beta_header: bool,
            path_and_query: String,
            first: Option<Value>,
            appended_samples: usize,
            appends_before_ack: usize,
        }

        /// The whole replay path — connect, update, append, commit, finish —
        /// against a local peer that refuses to acknowledge the update until
        /// audio has arrived. A client that waited for `session.updated`
        /// before appending would hang here (D10).
        // The handshake callback's `Result<Response, ErrorResponse>` is
        // tungstenite's signature, not ours to shrink.
        #[allow(clippy::result_large_err)]
        fn replay_against_peer(
            samples: Vec<f32>,
            on_update: Vec<Message>,
            on_commit: Vec<Message>,
        ) -> (Result<String, SttError>, Seen) {
            runtime().block_on(async {
                tokio::time::timeout(Duration::from_secs(5), async {
                    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                    let address = listener.local_addr().unwrap();
                    let seen = Arc::new(Mutex::new(Seen::default()));
                    let peer_seen = seen.clone();
                    let peer = tokio::spawn(async move {
                        let (stream, _) = listener.accept().await.unwrap();
                        let headers_seen = peer_seen.clone();
                        let mut socket = tokio_tungstenite::accept_hdr_async(
                            stream,
                            move |request: &ServerRequest, response: Response| {
                                let mut seen = headers_seen.lock().unwrap();
                                seen.authorization = request
                                    .headers()
                                    .get(header::AUTHORIZATION)
                                    .map(|v| v.to_str().unwrap().to_string());
                                seen.beta_header = request.headers().contains_key("openai-beta");
                                seen.path_and_query = request.uri().to_string();
                                Ok(response)
                            },
                        )
                        .await
                        .unwrap();
                        socket
                            .send(event(json!({"type": "session.created", "session": {}})))
                            .await
                            .unwrap();
                        let mut acked = false;
                        let mut on_update = Some(on_update);
                        let mut on_commit = Some(on_commit);
                        while let Some(Ok(frame)) = socket.next().await {
                            let Some(body) = frame_json(&frame) else {
                                continue;
                            };
                            let value: Value = serde_json::from_str(&body).unwrap();
                            // Decide under the lock, send after it: a guard
                            // held across an await makes the task non-Send.
                            let replies = {
                                let mut seen = peer_seen.lock().unwrap();
                                if seen.first.is_none() {
                                    seen.first = Some(value);
                                    let replies = on_update.take().unwrap_or_default();
                                    acked |= !replies.is_empty();
                                    replies
                                } else {
                                    match value["type"].as_str().unwrap() {
                                        "input_audio_buffer.append" => {
                                            let bytes = base64::engine::general_purpose::STANDARD
                                                .decode(value["audio"].as_str().unwrap())
                                                .unwrap();
                                            seen.appended_samples += bytes.len() / 2;
                                            if acked {
                                                vec![]
                                            } else {
                                                seen.appends_before_ack += 1;
                                                acked = true;
                                                vec![event(json!({"type": "session.updated", "session": {}}))]
                                            }
                                        }
                                        "input_audio_buffer.commit" => {
                                            on_commit.take().unwrap_or_default()
                                        }
                                        other => panic!("unexpected client event {other}"),
                                    }
                                }
                            };
                            for reply in replies {
                                // A client that has already failed the session
                                // may be gone; its result says so, not this.
                                if socket.send(reply).await.is_err() {
                                    return;
                                }
                            }
                        }
                    });
                    let url = realtime_url(&format!("http://{address}/v1/")).unwrap();
                    let update = session_update("gpt-live-transcribe", &["Hark".to_string()]);
                    let result = run(&url, KEY, update, samples, Finalize::REPLAY).await;
                    peer.await.unwrap();
                    let seen = std::mem::take(&mut *seen.lock().unwrap());
                    (result, seen)
                })
                .await
                .expect("local WebSocket replay must terminate")
            })
        }

        #[test]
        fn a_replay_speaks_the_whole_protocol_without_waiting_for_the_ack() {
            let samples = vec![0.25f32; CHUNK_SAMPLES * 2 + 100];
            let (result, seen) = replay_against_peer(
                samples,
                vec![],
                vec![
                    event(committed("item_1")),
                    event(completed("item_1", "Replayed.")),
                ],
            );
            assert_eq!(result.unwrap(), "Replayed.");
            assert_eq!(
                seen.authorization.as_deref(),
                Some(format!("Bearer {KEY}").as_str())
            );
            assert!(!seen.beta_header);
            assert_eq!(seen.path_and_query, "/v1/realtime?intent=transcription");
            assert_eq!(
                seen.first,
                Some(session_update("gpt-live-transcribe", &["Hark".to_string()]))
            );
            assert_eq!(seen.appended_samples, CHUNK_SAMPLES * 2 + 100);
            assert!(
                seen.appends_before_ack >= 1,
                "audio must not wait for session.updated"
            );
        }

        #[test]
        fn a_short_replay_is_padded_to_the_minimum_commit() {
            let (result, seen) = replay_against_peer(
                vec![0.25f32; 10],
                vec![],
                vec![event(committed("item_1")), event(completed("item_1", ""))],
            );
            assert_eq!(result.unwrap(), "");
            assert_eq!(seen.appended_samples, MIN_COMMIT_SAMPLES);
        }

        #[test]
        fn a_refused_key_is_an_auth_error_that_never_quotes_it() {
            // CP0's shape: the handshake succeeds, the update is answered by
            // an error event quoting a masked key, then close code 3000.
            let (result, _) = replay_against_peer(
                vec![0.25f32; CHUNK_SAMPLES],
                vec![event(json!({
                    "type": "error",
                    "error": {
                        "type": "invalid_request_error",
                        "code": "invalid_api_key",
                        "message": format!("Incorrect API key provided: sk-proj-****CRET. {PRIVATE}"),
                        "param": null,
                    }
                }))],
                // The close is held back to the commit, the client's last
                // write: a peer that has sent Close rejects any data after it.
                vec![Message::Close(Some(CloseFrame {
                    code: CloseCode::from(3000),
                    reason: "invalid_request_error.invalid_api_key".into(),
                }))],
            );
            let error = result.unwrap_err();
            match &error {
                SttError::Auth { status, detail, .. } => {
                    assert_eq!(*status, 401);
                    assert_eq!(detail, "invalid_api_key");
                }
                other => panic!("expected Auth, got {other}"),
            }
            assert_private(&error);
        }

        #[test]
        fn an_error_in_answer_to_the_update_fails_the_session() {
            // D10: audio is appended before the ack, so a rejected update
            // surfaces from a later read rather than from connect.
            let (result, _) = replay_against_peer(
                vec![0.25f32; CHUNK_SAMPLES * 3],
                vec![event(json!({
                    "type": "error",
                    "error": {
                        "type": "invalid_request_error",
                        "code": "integer_below_min_value",
                        "param": "session.audio.input.format.rate",
                        "message": PRIVATE,
                    }
                }))],
                vec![
                    event(committed("item_1")),
                    event(completed("item_1", "too late")),
                ],
            );
            let error = result.unwrap_err();
            let text = error.to_string();
            assert!(text.contains("before the session was configured"), "{text}");
            assert!(text.contains("integer_below_min_value"), "{text}");
            assert_private(&error);
        }

        /// What a local peer on its own thread does with one streaming session.
        #[derive(Default)]
        struct Script {
            /// Sent as soon as the socket is up, before anything is read.
            on_connect: Vec<Message>,
            /// Sent in answer to the client's commit.
            on_commit: Vec<Message>,
        }

        /// A streaming session against a local peer with its own thread and
        /// runtime, so the session's own `block_on` drives the client exactly
        /// as the pipeline worker does: in bursts, with the runtime idle in
        /// between. Returns once `on_connect` has been sent and has had time
        /// to land in the client's receive buffer. Drop the session before
        /// joining the peer.
        fn streamed_session(script: Script) -> (OpenAiLiveSession, std::thread::JoinHandle<()>) {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let (opened, sent) = std::sync::mpsc::channel();
            let peer = std::thread::spawn(move || {
                let (stream, _) = listener.accept().unwrap();
                stream.set_nonblocking(true).unwrap();
                runtime().block_on(async move {
                    let stream = tokio::net::TcpStream::from_std(stream).unwrap();
                    let mut socket =
                        WebSocketStream::from_raw_socket(stream, Role::Server, None).await;
                    for frame in script.on_connect {
                        socket.send(frame).await.unwrap();
                    }
                    opened.send(()).unwrap();
                    let mut on_commit = Some(script.on_commit);
                    while let Some(Ok(frame)) = socket.next().await {
                        let commit = frame_json(&frame)
                            .and_then(|body| serde_json::from_str::<Value>(&body).ok())
                            .is_some_and(|v| v["type"] == "input_audio_buffer.commit");
                        if commit {
                            for reply in on_commit.take().unwrap_or_default() {
                                if socket.send(reply).await.is_err() {
                                    return;
                                }
                            }
                        }
                    }
                })
            });
            let runtime = Arc::new(runtime());
            let socket = runtime.block_on(async {
                let stream = tokio::net::TcpStream::connect(address).await.unwrap();
                WebSocketStream::from_raw_socket(MaybeTlsStream::Plain(stream), Role::Client, None)
                    .await
            });
            sent.recv_timeout(Duration::from_secs(5))
                .expect("the peer sends its opening frames");
            // Sent is not yet received; loopback still takes a moment.
            std::thread::sleep(Duration::from_millis(50));
            let session = OpenAiLiveSession {
                runtime,
                socket: Some(socket),
                api_key: KEY.into(),
                turn: Turn::default(),
                pending: Vec::new(),
                appended: 0,
                started: Instant::now(),
            };
            (session, peer)
        }

        fn delta() -> Message {
            event(
                json!({"type": "conversation.item.input_audio_transcription.delta", "item_id": "item_1", "delta": "x"}),
            )
        }

        #[test]
        fn an_empty_streamed_transcript_hands_the_dictation_to_the_fallback() {
            // Streamed audio is not gain-normalized; the fallback's is. On a
            // quiet mic a blank stream reported as success lost the dictation
            // as "empty transcript" with the fallback never asked.
            use crate::LiveSession;
            for blank in ["", "  \n"] {
                let (mut session, peer) = streamed_session(Script {
                    on_commit: vec![
                        event(committed("item_1")),
                        event(completed("item_1", blank)),
                    ],
                    ..Default::default()
                });
                session.push(&vec![0.01; CHUNK_SAMPLES]).unwrap();
                let error = session
                    .finish()
                    .err()
                    .expect("a blank stream must not count as a transcript");
                assert!(matches!(error, SttError::Provider { .. }), "{error}");
                assert!(error.to_string().contains("empty transcript"), "{error}");
                drop(session);
                peer.join().unwrap();
            }

            let (mut session, peer) = streamed_session(Script {
                on_commit: vec![
                    event(committed("item_1")),
                    event(completed("item_1", "Said.")),
                ],
                ..Default::default()
            });
            session.push(&vec![0.01; CHUNK_SAMPLES]).unwrap();
            assert_eq!(session.finish().unwrap().text, "Said.");
            drop(session);
            peer.join().unwrap();
        }

        #[test]
        fn a_drain_sees_frames_that_landed_outside_the_runtime() {
            // The worker enters the adapter's runtime only inside push, so
            // frames reach the kernel while nothing polls the driver. Without
            // the driver turn before `now_or_never`, tokio's readiness is
            // still empty and the drain reads nothing.
            use crate::LiveSession;
            let (mut session, peer) = streamed_session(Script {
                on_connect: vec![
                    event(json!({"type": "session.updated", "session": {}})),
                    delta(),
                    delta(),
                ],
                ..Default::default()
            });
            session.push(&[]).unwrap();
            assert_eq!(session.turn.frames(), 3);
            assert_eq!(session.turn.deltas(), 2);
            assert!(session.turn.configured());
            drop(session);
            peer.join().unwrap();
        }

        #[test]
        fn an_empty_drain_never_waits_on_a_timer() {
            // `timeout(Duration::ZERO, ..)` parked until the next 1 ms timer
            // tick on every empty drain, which Windows stretches to ~15.6 ms:
            // these 100 drains took about 1.5 s. Without a timer they take
            // microseconds; the bound is loose so a loaded CI box cannot trip it.
            use crate::LiveSession;
            let (mut session, peer) = streamed_session(Script::default());
            let started = Instant::now();
            for _ in 0..100 {
                session.push(&[]).unwrap();
            }
            let elapsed = started.elapsed();
            assert!(
                elapsed < Duration::from_millis(500),
                "100 empty drains took {elapsed:?}"
            );
            drop(session);
            peer.join().unwrap();
        }

        #[test]
        fn a_failed_send_reports_the_servers_refusal_instead() {
            // D10's race, decided for the server: the refusal is already in
            // the receive buffer when a write fails because it hung up.
            let (mut session, peer) = streamed_session(Script {
                on_connect: vec![
                    event(json!({
                        "type": "error",
                        "error": {
                            "type": "invalid_request_error",
                            "code": "invalid_api_key",
                            "message": format!("Incorrect API key provided: sk-proj-****CRET. {PRIVATE}"),
                        }
                    })),
                    Message::Close(Some(CloseFrame {
                        code: CloseCode::from(3000),
                        reason: "invalid_request_error.invalid_api_key".into(),
                    })),
                ],
                ..Default::default()
            });
            let send_error = fail("append send failed: I/O ConnectionReset");
            let socket = session.socket.as_mut().unwrap();
            let reported = session.runtime.block_on(explain_send_failure(
                socket,
                &mut session.turn,
                send_error,
            ));
            match &reported {
                SttError::Auth { status, detail, .. } => {
                    assert_eq!(*status, 401);
                    assert_eq!(detail, "invalid_api_key");
                }
                other => panic!("expected Auth, got {other}"),
            }
            assert_private(&reported);
            drop(session);
            peer.join().unwrap();
        }

        #[test]
        fn a_failed_send_with_nothing_from_the_server_keeps_its_own_error() {
            let (mut session, peer) = streamed_session(Script {
                on_connect: vec![delta()],
                ..Default::default()
            });
            let socket = session.socket.as_mut().unwrap();
            let reported = session.runtime.block_on(explain_send_failure(
                socket,
                &mut session.turn,
                fail("append send failed: connection closed"),
            ));
            assert!(
                reported.to_string().contains("append send failed"),
                "{reported}"
            );
            // What it did read on the way was still recorded.
            assert_eq!(session.turn.deltas(), 1);
            drop(session);
            peer.join().unwrap();
        }
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
            runtime: std::sync::Arc::new(
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| {
                        fail(format!(
                            "could not start the realtime runtime ({:?})",
                            e.kind()
                        ))
                    })?,
            ),
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
    /// Wall clock from the session opening, so `request_ms` stays comparable
    /// with the batch adapters' full round trip.
    started: Instant,
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
            request_ms: self.started.elapsed().as_millis(),
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
            started: Instant::now(),
        }))
    }

    fn input_rate(&self) -> u32 {
        INPUT_RATE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wav::samples_to_pcm16_le;

    const KEY: &str = "sk-proj-hark-test-SECRET";

    fn terms(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn rendered(error: &SttError) -> String {
        format!("{error} {error:?}")
    }

    #[test]
    fn the_session_update_is_exactly_the_probed_shape() {
        assert_eq!(
            session_update("gpt-live-transcribe", &terms(&["Hark", "Levenshtein"])),
            json!({
                "type": "session.update",
                "session": {
                    "type": "transcription",
                    "audio": {
                        "input": {
                            "format": {"type": "audio/pcm", "rate": 24000},
                            "transcription": {
                                "model": "gpt-live-transcribe",
                                "keywords": ["Hark", "Levenshtein"],
                                "languages": ["en"],
                                "delay": "xhigh"
                            },
                            "turn_detection": null
                        }
                    }
                }
            })
        );
    }

    #[test]
    fn turn_detection_is_sent_as_an_explicit_null() {
        let wire = session_update("m", &[]).to_string();
        assert!(wire.contains(r#""turn_detection":null"#), "{wire}");
    }

    #[test]
    fn no_terms_omits_the_keywords_key_entirely() {
        let update = session_update("m", &terms(&["", " <> "]));
        assert!(update["session"]["audio"]["input"]["transcription"]
            .get("keywords")
            .is_none());
    }

    #[test]
    fn the_session_update_sanitizes_keywords_itself() {
        let update = session_update("m", &terms(&["<Hark>"]));
        assert_eq!(
            update["session"]["audio"]["input"]["transcription"]["keywords"],
            json!(["Hark"])
        );
    }

    #[test]
    fn the_socket_url_is_derived_from_the_https_base() {
        assert_eq!(
            realtime_url("https://api.openai.com/v1").unwrap(),
            "wss://api.openai.com/v1/realtime?intent=transcription"
        );
        assert_eq!(
            realtime_url("https://api.openai.com/v1/").unwrap(),
            "wss://api.openai.com/v1/realtime?intent=transcription"
        );
        assert_eq!(
            realtime_url("http://localhost:8080/v1").unwrap(),
            "ws://localhost:8080/v1/realtime?intent=transcription"
        );
        assert_eq!(
            realtime_url(" HTTPS://gateway.example.com/openai/v1// ").unwrap(),
            "wss://gateway.example.com/openai/v1/realtime?intent=transcription"
        );
    }

    #[test]
    fn a_base_url_without_a_web_scheme_is_refused() {
        for base in [
            "",
            "api.openai.com/v1",
            "ftp://api.openai.com/v1",
            "https://",
        ] {
            assert!(realtime_url(base).is_err(), "{base:?}");
        }
    }

    #[test]
    fn keywords_are_one_clean_line_each_and_unique() {
        assert_eq!(
            sanitize_keywords(&terms(&[
                "Hark",
                "<Levenshtein>",
                "New\r\nYork",
                "  spaced   out  ",
                "tab\there",
                "",
                "   ",
                "<>",
                "Hark",
                "New York",
                "hark",
            ])),
            terms(&[
                "Hark",
                "Levenshtein",
                "New York",
                "spaced out",
                "tab here",
                "hark"
            ])
        );
    }

    #[test]
    fn appends_carry_base64_pcm16() {
        let message = append_message(&samples_to_pcm16_le(&[1.0, 0.0, -1.0]));
        assert_eq!(message["type"], json!("input_audio_buffer.append"));
        // 0xFF7F 0x0000 0x0180, base64.
        assert_eq!(message["audio"], json!("/38AAAGA"));
        assert_eq!(
            commit_message(),
            json!({"type": "input_audio_buffer.commit"})
        );
    }

    #[test]
    fn chunking_matches_one_hundred_milliseconds_of_audio() {
        assert_eq!(vec![0.0f32; 24_000].chunks(CHUNK_SAMPLES).count(), 10);
    }

    #[test]
    fn a_short_turn_is_padded_to_one_hundred_milliseconds() {
        assert_eq!(commit_padding(0), 2_400);
        assert_eq!(commit_padding(10), 2_390);
        assert_eq!(commit_padding(2_400), 0);
        assert_eq!(commit_padding(100_000), 0);
    }

    #[test]
    fn every_event_type_is_classified() {
        let parse = |v: Value| parse_server_message(&v.to_string()).unwrap();
        assert_eq!(
            parse(json!({"type": "session.created", "session": {}})),
            ServerEvent::SessionCreated
        );
        assert_eq!(
            parse(json!({"type": "session.updated", "session": {}})),
            ServerEvent::SessionUpdated
        );
        assert_eq!(
            parse(
                json!({"type": "input_audio_buffer.committed", "item_id": "item_1", "previous_item_id": null})
            ),
            ServerEvent::Committed {
                item_id: "item_1".into()
            }
        );
        assert_eq!(
            parse(
                json!({"type": "conversation.item.input_audio_transcription.delta", "item_id": "item_1", "delta": "he"})
            ),
            ServerEvent::Delta
        );
        assert_eq!(
            parse(
                json!({"type": "conversation.item.input_audio_transcription.completed", "item_id": "item_1", "transcript": "hello"})
            ),
            ServerEvent::Completed {
                item_id: "item_1".into(),
                transcript: "hello".into()
            }
        );
        assert_eq!(
            parse(json!({
                "type": "conversation.item.input_audio_transcription.failed",
                "item_id": "item_1",
                "error": {"code": "audio_unintelligible", "type": "invalid_request_error", "message": "free text"}
            })),
            ServerEvent::Failed {
                item_id: "item_1".into(),
                error: RemoteError {
                    code: Some("audio_unintelligible".into()),
                    kind: Some("invalid_request_error".into()),
                    param: None,
                }
            }
        );
        assert_eq!(
            parse(json!({
                "type": "error",
                "error": {"code": "integer_below_min_value", "param": "session.audio.input.format.rate", "message": "Expected a value >= 24000"}
            })),
            ServerEvent::Error(RemoteError {
                code: Some("integer_below_min_value".into()),
                kind: None,
                param: Some("session.audio.input.format.rate".into()),
            })
        );
        for ignored in IGNORED_EVENTS {
            assert_eq!(
                parse(json!({"type": ignored})),
                ServerEvent::Other,
                "{ignored}"
            );
        }
        assert_eq!(
            parse(json!({"type": "some.future_event"})),
            ServerEvent::Other
        );
        assert_eq!(parse(json!({"no_type": true})), ServerEvent::Other);
    }

    #[test]
    fn a_non_json_frame_is_an_error_not_a_silent_skip() {
        assert!(parse_server_message("<html>502</html>").is_err());
    }

    #[test]
    fn a_completion_missing_its_transcript_is_an_error_not_an_empty_dictation() {
        let body = json!({"type": "conversation.item.input_audio_transcription.completed", "item_id": "item_1"});
        assert!(parse_server_message(&body.to_string()).is_err());
    }

    #[test]
    fn debug_output_never_carries_the_transcript() {
        let event = ServerEvent::Completed {
            item_id: "item_1".into(),
            transcript: "PRIVATE words".into(),
        };
        assert!(!format!("{event:?}").contains("PRIVATE"));
    }

    fn absorb_all(turn: &mut Turn, events: &[Value]) -> Result<bool, SttError> {
        let mut done = false;
        for event in events {
            done = turn.absorb(&event.to_string())?;
        }
        Ok(done)
    }

    /// A turn whose commit has gone out, as every finalise sees it.
    fn committed_turn() -> Turn {
        let mut turn = Turn::default();
        turn.sent_commit();
        turn
    }

    #[test]
    fn the_turn_waits_for_its_own_item() {
        let mut turn = committed_turn();
        let done = absorb_all(
            &mut turn,
            &[
                json!({"type": "session.created"}),
                json!({"type": "session.updated"}),
                json!({"type": "conversation.item.input_audio_transcription.delta", "item_id": "item_1", "delta": "he"}),
                json!({"type": "input_audio_buffer.committed", "item_id": "item_1"}),
                json!({"type": "conversation.item.input_audio_transcription.completed", "item_id": "item_9", "transcript": "someone else"}),
            ],
        )
        .unwrap();
        assert!(!done, "another item's completion must not finish this turn");
        assert!(turn.committed() && turn.configured());
        assert_eq!(turn.deltas(), 1);
        assert!(turn
            .absorb(&json!({"type": "conversation.item.input_audio_transcription.completed", "item_id": "item_1", "transcript": "mine"}).to_string())
            .unwrap());
        assert_eq!(turn.take_transcript().as_deref(), Some("mine"));
        assert_eq!(turn.frames(), 6);
    }

    #[test]
    fn a_completion_before_the_commit_ack_is_kept_for_its_item() {
        let mut turn = committed_turn();
        assert!(!absorb_all(
            &mut turn,
            &[
                json!({"type": "conversation.item.input_audio_transcription.completed", "item_id": "item_9", "transcript": "someone else"}),
                json!({"type": "conversation.item.input_audio_transcription.completed", "item_id": "item_1", "transcript": "early"}),
            ],
        )
        .unwrap());
        assert!(turn
            .absorb(
                &json!({"type": "input_audio_buffer.committed", "item_id": "item_1"}).to_string()
            )
            .unwrap());
        assert_eq!(turn.take_transcript().as_deref(), Some("early"));
    }

    #[test]
    fn another_items_failure_is_ignored_but_ours_is_not() {
        let failed = |id: &str| json!({"type": "conversation.item.input_audio_transcription.failed", "item_id": id, "error": {"code": "audio_unintelligible"}});
        let mut turn = committed_turn();
        absorb_all(
            &mut turn,
            &[
                json!({"type": "input_audio_buffer.committed", "item_id": "item_1"}),
                failed("item_9"),
            ],
        )
        .unwrap();
        assert!(turn.absorb(&failed("item_1").to_string()).is_err());
    }

    #[test]
    fn a_commit_acknowledgement_before_our_commit_is_not_ours() {
        // A server-side commit mid-hold: its item holds only the first
        // phrase. Taking it as ours injected that phrase as the dictation.
        let mut turn = Turn::default();
        assert!(!absorb_all(
            &mut turn,
            &[
                json!({"type": "input_audio_buffer.committed", "item_id": "item_early"}),
                json!({"type": "conversation.item.input_audio_transcription.completed", "item_id": "item_early", "transcript": "first phrase"}),
            ],
        )
        .unwrap());
        assert!(!turn.committed(), "an unprompted commit names no item");
        assert_eq!(turn.take_transcript(), None);

        turn.sent_commit();
        assert!(!turn
            .absorb(
                &json!({"type": "input_audio_buffer.committed", "item_id": "item_ours"})
                    .to_string()
            )
            .unwrap());
        assert!(turn
            .absorb(&json!({"type": "conversation.item.input_audio_transcription.completed", "item_id": "item_ours", "transcript": "the whole dictation"}).to_string())
            .unwrap());
        assert_eq!(
            turn.take_transcript().as_deref(),
            Some("the whole dictation")
        );
    }

    #[test]
    fn any_error_event_fails_the_session() {
        let mut turn = Turn::default();
        let error = turn
            .absorb(&json!({"type": "error", "error": {"code": "integer_below_min_value", "param": "session.audio.input.format.rate"}}).to_string())
            .unwrap_err();
        let text = error.to_string();
        assert!(text.contains("before the session was configured"), "{text}");
        assert!(
            text.contains("integer_below_min_value at session.audio.input.format.rate"),
            "{text}"
        );
    }

    #[test]
    fn server_errors_map_onto_the_http_taxonomy() {
        let error = |code: Option<&str>, kind: Option<&str>| {
            remote_error(
                "server error",
                &RemoteError {
                    code: code.map(Into::into),
                    kind: kind.map(Into::into),
                    param: None,
                },
            )
        };
        let auth = |e: SttError| match e {
            SttError::Auth { status, detail, .. } => Some((status, detail)),
            _ => None,
        };
        assert_eq!(
            auth(error(
                Some("invalid_api_key"),
                Some("invalid_request_error")
            )),
            Some((401, "invalid_api_key".into()))
        );
        for code in [
            "insufficient_quota",
            "account_deactivated",
            "organization_deactivated",
            "permission_denied",
            "access_denied",
        ] {
            assert_eq!(
                auth(error(Some(code), None)),
                Some((403, code.into())),
                "{code}"
            );
        }
        assert_eq!(
            auth(error(None, Some("authentication_error"))),
            Some((401, "authentication_error".into()))
        );
        assert_eq!(
            auth(error(None, Some("permission_error"))),
            Some((403, "permission_error".into()))
        );
        assert!(matches!(
            error(Some("rate_limit_exceeded"), None),
            SttError::RateLimited {
                retry_after_s: None,
                ..
            }
        ));
        assert!(matches!(
            error(None, Some("rate_limit_error")),
            SttError::RateLimited { .. }
        ));
        let other = error(Some("model_not_found"), Some("invalid_request_error"));
        assert!(matches!(other, SttError::Provider { .. }));
        assert!(other
            .to_string()
            .contains("model_not_found (invalid_request_error)"));
        assert!(error(None, None).to_string().contains("no error code"));
    }

    #[test]
    fn close_codes_3000_and_3003_are_auth_refusals() {
        assert!(matches!(
            close_error(3000, "invalid_request_error.invalid_api_key", "mid-stream"),
            SttError::Auth { status: 401, ref detail, .. } if detail == "invalid_api_key"
        ));
        assert!(matches!(
            close_error(3003, "", "x"),
            SttError::Auth { status: 403, .. }
        ));
        let other = close_error(1011, "PRIVATE free text", "mid-stream");
        assert!(other.to_string().contains("1011"));
        assert!(!rendered(&other).contains("PRIVATE"));
        // A reason that is not a slug is dropped, not echoed.
        assert!(matches!(
            close_error(3000, "Key sk-proj-****CRET rejected", "x"),
            SttError::Auth { ref detail, .. } if detail.is_empty()
        ));
    }

    #[test]
    fn error_fields_that_are_free_text_are_dropped() {
        let parsed = parse_server_message(
            &json!({"type": "error", "error": {
                "code": "Incorrect API key sk-proj-abc",
                "type": "PRIVATE_TRANSCRIPT",
                "param": "a param with spaces",
                "message": "sk-proj-abc"
            }})
            .to_string(),
        )
        .unwrap();
        assert_eq!(parsed, ServerEvent::Error(RemoteError::default()));
    }

    #[test]
    fn scrubbing_removes_the_key_sk_fragments_and_bearer_tokens() {
        let leaked = format!(
            "Incorrect API key provided: sk-proj-****CRET. Header was Bearer {KEY}; gateway key gw_live_123 too"
        );
        let safe = scrub(&leaked, "gw_live_123");
        for secret in ["sk-", "CRET", KEY, "Bearer", "gw_live_123"] {
            assert!(!safe.contains(secret), "{secret} survived: {safe}");
        }
        assert!(
            safe.starts_with("Incorrect API key provided: <redacted>"),
            "{safe}"
        );
    }

    #[test]
    fn scrubbing_leaves_words_that_merely_contain_the_marker() {
        assert_eq!(
            scrub("task-list and disk-full", ""),
            "task-list and disk-full"
        );
        assert_eq!(scrub("plain error text", KEY), "plain error text");
    }

    #[test]
    fn every_error_this_module_builds_is_scrubbed() {
        let error = fail(format!("connect failed: Bearer {KEY}"));
        assert!(!rendered(&error).contains("SECRET"), "{error}");
        let error = redact(
            SttError::Http {
                provider: PROVIDER.into(),
                detail: "proxy said gw_live_123".into(),
            },
            "gw_live_123",
        );
        assert!(!rendered(&error).contains("gw_live_123"));
    }

    #[test]
    fn the_adapter_reports_its_label_and_never_its_key() {
        let config = ProviderConfig {
            kind: crate::ProviderKind::OpenAiLive,
            label: "openai".into(),
            base_url: "https://api.openai.com/v1".into(),
            model: "gpt-live-transcribe".into(),
            api_key: KEY.into(),
            bias_terms: terms(&["Hark"]),
            cleanup_instruction: None,
            live_mode: crate::gemini_live::TranscribeMode::Verbatim,
        };
        let adapter = OpenAiLive::new(&config).unwrap();
        assert_eq!(adapter.label(), "openai");
        assert!(!format!("{config:?}").contains(KEY));
        // An unusable base URL is refused when the adapter is built.
        let bad = ProviderConfig {
            base_url: "api.openai.com".into(),
            ..config
        };
        assert!(OpenAiLive::new(&bad).is_err());
    }

    #[cfg(not(feature = "live"))]
    #[test]
    fn without_the_feature_a_replay_says_why() {
        let config = ProviderConfig {
            kind: crate::ProviderKind::OpenAiLive,
            label: "openai".into(),
            base_url: "https://api.openai.com/v1".into(),
            model: "gpt-live-transcribe".into(),
            api_key: KEY.into(),
            bias_terms: vec![],
            cleanup_instruction: None,
            live_mode: crate::gemini_live::TranscribeMode::Verbatim,
        };
        let error = OpenAiLive::new(&config)
            .unwrap()
            .transcribe(crate::fixture::SPIKE_WAV)
            .err()
            .unwrap();
        assert!(error.to_string().contains("`live` feature"), "{error}");
    }

    #[cfg(feature = "live")]
    #[test]
    fn the_replay_upsample_has_the_exact_24k_length() {
        let clip = vec![0.1f32; 16_000];
        assert_eq!(upsample_16k_to_24k(&clip).unwrap().len(), 24_000);
        assert_eq!(upsample_16k_to_24k(&clip[..1_001]).unwrap().len(), 1_502);
        assert!(upsample_16k_to_24k(&[]).unwrap().is_empty());
    }

    #[cfg(feature = "live")]
    #[test]
    fn the_adapter_declares_24k_to_the_pipeline() {
        use crate::LiveStt;
        let config = ProviderConfig {
            kind: crate::ProviderKind::OpenAiLive,
            label: "openai".into(),
            base_url: "https://api.openai.com/v1".into(),
            model: "gpt-live-transcribe".into(),
            api_key: KEY.into(),
            bias_terms: vec![],
            cleanup_instruction: None,
            live_mode: crate::gemini_live::TranscribeMode::Verbatim,
        };
        assert_eq!(OpenAiLive::new(&config).unwrap().input_rate(), 24_000);
    }
}

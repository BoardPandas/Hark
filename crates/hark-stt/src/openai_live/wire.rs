//! The realtime API's messages: the audio format, the client events Hark
//! sends, and how a server frame is classified. Pure, so none of it needs a
//! socket.

use super::fail;
use super::server_error::{safe_slug, RemoteError};
use crate::error::{json_error_detail, SttError};
use serde_json::{json, Value};

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

const _: () = assert!(CHUNK_SAMPLES == INPUT_RATE as usize / 10);
const _: () = assert!(MIN_COMMIT_SAMPLES == INPUT_RATE as usize / 10);

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
pub(super) const IGNORED_EVENTS: &[&str] = &[
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

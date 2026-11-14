//! The Live API's messages: the frames Hark sends, and how a server frame is
//! classified and its transcript collected. Pure, so none of it needs a
//! socket.

use super::{fail, TranscribeMode};
use crate::error::SttError;
use serde_json::{json, Value};

/// 100 ms of 16 kHz mono audio, the chunk size the Live API documents.
pub const CHUNK_SAMPLES: usize = 1_600;

/// The opening `setup` message.
///
/// `customVocabulary` is omitted entirely when there are no bias terms rather
/// than sent empty, and capped at the documented 1 000 phrases. Google notes
/// best results at ~100 terms, but silently dropping a user's spellbook is
/// worse than diminishing returns, so only the hard limit is enforced.
pub fn setup_message(model: &str, bias_terms: &[String], mode: TranscribeMode) -> Value {
    /// The Live API's documented ceiling on `customVocabulary`.
    const VOCAB_MAX: usize = 1_000;

    let mut transcription = json!({
        "languageCodes": ["en-US"],
        "mode": mode.wire(),
    });
    if !bias_terms.is_empty() {
        let vocab: Vec<&String> = bias_terms.iter().take(VOCAB_MAX).collect();
        transcription["customVocabulary"] = json!(vocab);
    }
    json!({
        "setup": {
            // The Live API wants the fully-qualified resource name, not the
            // bare model id every other Hark adapter passes.
            "model": format!("models/{model}"),
            "generationConfig": { "responseModalities": ["TEXT"] },
            "inputAudioTranscription": transcription,
            // Turn off the server's own voice-activity detection and mark the
            // turn explicitly instead. Push-to-talk *knows* when speech starts
            // and ends -- the key went down and came up -- so letting the
            // server infer it from audio timing is guessing at something we
            // already have exactly. It also makes the session insensitive to
            // how fast audio arrives: a finished clip replayed at hundreds of
            // times real time is not a realtime stream, and left to its own
            // VAD the server can keep a turn open forever and never emit a
            // final transcript.
            "realtimeInputConfig": {
                "automaticActivityDetection": { "disabled": true }
            },
        }
    })
}

/// One `realtimeInput` audio frame: base64 PCM16 with the rate in the MIME type.
pub fn audio_message(pcm_le_bytes: &[u8]) -> Value {
    use base64::Engine;
    json!({
        "realtimeInput": {
            "audio": {
                "data": base64::engine::general_purpose::STANDARD.encode(pcm_le_bytes),
                "mimeType": "audio/pcm;rate=16000",
            }
        }
    })
}

/// End the turn explicitly. This is the whole reason the Live API suits
/// push-to-talk: release is a fact, not something a VAD has to infer.
pub fn audio_stream_end_message() -> Value {
    json!({ "realtimeInput": { "audioStreamEnd": true } })
}

/// Mark the start of speech. Required once automatic activity detection is
/// disabled: without it the server has been told not to guess and has not
/// been told the answer, so no turn ever opens.
pub fn activity_start_message() -> Value {
    json!({ "realtimeInput": { "activityStart": {} } })
}

/// Mark the end of speech: the key came up.
pub fn activity_end_message() -> Value {
    json!({ "realtimeInput": { "activityEnd": {} } })
}

/// What one server frame contributed.
#[derive(Debug, PartialEq)]
pub enum ServerEvent {
    /// A finalised transcript segment. Segments accumulate across a turn.
    Final(String),
    /// A speculative partial. Hark discards these — it injects once, on
    /// release, so a hypothesis that may be revised is not useful.
    Interim,
    /// The turn is complete; stop reading.
    TurnComplete,
    /// The server has accepted `setup` and the session is usable. Nothing may
    /// be sent before this arrives.
    SetupComplete,
    /// Anything else (keepalive, usage metadata). Carries the frame's
    /// top-level keys so an unrecognised frame can be diagnosed without
    /// logging its contents.
    Other,
}

/// Classify one server frame. Pure, so the protocol is testable without a
/// socket — which matters because the wire path itself cannot be exercised
/// offline.
pub fn parse_server_message(body: &str) -> Result<ServerEvent, SttError> {
    parse_server_frame(body).map(|(event, _)| event)
}

// A frame may carry both its final transcript and the completion marker.
// Keep those facts independent so collecting the text cannot hide completion.
fn parse_server_frame(body: &str) -> Result<(ServerEvent, bool), SttError> {
    let v: Value = serde_json::from_str(body).map_err(|e| {
        fail(crate::error::json_error_detail(
            "server frame was not JSON",
            &e,
        ))
    })?;
    if v.get("setupComplete").is_some() {
        return Ok((ServerEvent::SetupComplete, false));
    }
    let content = match v.get("serverContent") {
        Some(c) => c,
        None => {
            // Never silently swallow an unrecognised frame: this is exactly
            // how a protocol mistake turns into an unexplained timeout. Keys
            // only -- a frame may carry transcript text, which belongs in
            // history, not in a log line.
            log::debug!("gemini live: unhandled frame with keys {:?}", top_keys(&v));
            return Ok((ServerEvent::Other, false));
        }
    };
    // Observed against the real API: a transcription turn ends with
    // `generationComplete`. Accept the documented `turnComplete` too.
    let complete = ["generationComplete", "turnComplete"]
        .into_iter()
        .any(|done| content.get(done).and_then(Value::as_bool) == Some(true));
    if let Some(text) = content
        .get("inputTranscription")
        .and_then(|t| t.get("text"))
        .and_then(|t| t.as_str())
    {
        return Ok((ServerEvent::Final(text.to_string()), complete));
    }
    if content.get("interimInputTranscription").is_some() {
        return Ok((ServerEvent::Interim, complete));
    }
    if complete {
        return Ok((ServerEvent::TurnComplete, true));
    }
    Ok((ServerEvent::Other, false))
}

/// Classify one server frame and keep anything it contributed.
///
/// The single place a transcript is collected. Both read paths — the drain
/// that runs throughout the hold and the loop that finalises the turn — go
/// through here, because the bug this replaced was one path keeping frames and
/// the other silently dropping them, which cost users everything they said
/// before the last pause. Returns the event so callers can still act on it.
pub fn absorb(body: &str, segments: &mut Vec<String>) -> Result<ServerEvent, SttError> {
    let (event, complete) = parse_server_frame(body)?;
    if let ServerEvent::Final(text) = &event {
        segments.push(text.clone());
    }
    Ok(if complete {
        ServerEvent::TurnComplete
    } else {
        event
    })
}

/// Known protocol keys only: an unknown JSON key can itself be user content.
pub fn top_keys(v: &Value) -> Vec<String> {
    v.as_object()
        .map(|o| {
            o.keys()
                .map(|key| match key.as_str() {
                    "setupComplete"
                    | "serverContent"
                    | "usageMetadata"
                    | "goAway"
                    | "sessionResumptionUpdate"
                    | "error"
                    | "inputTranscription"
                    | "interimInputTranscription"
                    | "generationComplete"
                    | "turnComplete"
                    | "modelTurn"
                    | "parts"
                    | "text" => key.clone(),
                    _ => "<unknown>".to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Join finalised segments into one utterance, collapsing the seams.
///
/// The Live API emits a final transcript per pause, so a dictation with a
/// mid-sentence breath arrives as two segments that must not be run together
/// into "wordword".
pub fn join_segments(segments: &[String]) -> String {
    let mut out = String::new();
    for segment in segments {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(segment);
    }
    out
}

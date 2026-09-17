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
use crate::{ProviderConfig, SttProvider, Transcript};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

/// Wire host for the Live API. The model and key are appended per session.
pub const LIVE_HOST: &str =
    "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent";

/// 100 ms of 16 kHz mono audio, the chunk size the Live API documents.
pub const CHUNK_SAMPLES: usize = 1_600;

/// How long to wait for the final transcript after `audioStreamEnd`. Shorter
/// than the REST budget: by this point the audio is already uploaded, so a
/// slow finalise is a stall, not a large transfer still in flight.
pub const FINALIZE_TIMEOUT_MS: u64 = 8_000;

/// Connect + setup budget. A Live session that cannot hand-shake quickly has
/// already lost to the batch adapters.
pub const CONNECT_TIMEOUT_MS: u64 = 4_000;

/// How the model is asked to render the transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscribeMode {
    /// Literal transcript, fillers and self-corrections intact. Hark's default:
    /// it keeps `Transcript::text` a true verbatim record, which the spellbook
    /// corrector, the invocation matcher and the cleanup expansion guard all
    /// compare against.
    Verbatim,
    /// The provider removes disfluencies and formats the text in the same
    /// round trip. Fills `Transcript::cleaned`, and the pipeline then skips its
    /// own cleanup call.
    Smart,
}

impl TranscribeMode {
    /// The wire value for `inputAudioTranscription.mode`.
    pub fn wire(self) -> &'static str {
        match self {
            TranscribeMode::Verbatim => "VERBATIM",
            TranscribeMode::Smart => "SMART",
        }
    }
}

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

/// Convert f32 samples in [-1.0, 1.0] to little-endian PCM16 bytes.
pub fn samples_to_pcm16_le(samples: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 2);
    for &s in samples {
        let clamped = s.clamp(-1.0, 1.0);
        out.extend_from_slice(&((clamped * 32767.0) as i16).to_le_bytes());
    }
    out
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
    /// Anything else (setup ack, keepalive, usage metadata).
    Other,
}

/// Classify one server frame. Pure, so the protocol is testable without a
/// socket — which matters because the wire path itself cannot be exercised
/// offline.
pub fn parse_server_message(body: &str) -> Result<ServerEvent, SttError> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| fail(format!("server frame was not JSON ({e}): {body:.200}")))?;
    let content = match v.get("serverContent") {
        Some(c) => c,
        None => return Ok(ServerEvent::Other),
    };
    if let Some(text) = content
        .get("inputTranscription")
        .and_then(|t| t.get("text"))
        .and_then(|t| t.as_str())
    {
        return Ok(ServerEvent::Final(text.to_string()));
    }
    if content.get("interimInputTranscription").is_some() {
        return Ok(ServerEvent::Interim);
    }
    if content
        .get("turnComplete")
        .and_then(|t| t.as_bool())
        .unwrap_or(false)
    {
        return Ok(ServerEvent::TurnComplete);
    }
    Ok(ServerEvent::Other)
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

/// Build the [`Transcript`] for a finished session.
///
/// In SMART mode the provider already removed disfluencies and formatted the
/// text, so the one string it returned is the cleaned one. It is reported in
/// **both** fields deliberately: `text` because the pipeline injects and
/// records something either way, and `cleaned` because that is the flag the
/// worker reads to skip its own cleanup call. Callers that need a guaranteed
/// verbatim record must use [`TranscribeMode::Verbatim`], which is the default.
pub fn transcript_for(mode: TranscribeMode, text: String, request_ms: u128) -> Transcript {
    match mode {
        TranscribeMode::Verbatim => Transcript {
            text,
            cleaned: None,
            request_ms,
        },
        TranscribeMode::Smart => Transcript {
            cleaned: Some(text.clone()),
            text,
            request_ms,
        },
    }
}

#[cfg(feature = "live")]
mod session {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    /// Run one complete session: connect, configure, stream every chunk, end
    /// the turn, and collect the finalised transcript.
    pub async fn run(
        url: String,
        setup: Value,
        pcm: Vec<u8>,
        mode: TranscribeMode,
    ) -> Result<String, SttError> {
        let connect = tokio_tungstenite::connect_async(&url);
        let (mut socket, _) =
            tokio::time::timeout(Duration::from_millis(CONNECT_TIMEOUT_MS), connect)
                .await
                .map_err(|_| SttError::Timeout {
                    provider: "gemini-live".to_string(),
                    configured_ms: CONNECT_TIMEOUT_MS,
                })?
                // tungstenite's Display echoes the URL, which carries the key.
                .map_err(|e| fail(format!("websocket connect failed: {e}")))?;

        socket
            .send(Message::Text(setup.to_string().into()))
            .await
            .map_err(|e| fail(format!("setup send failed: {e}")))?;

        // 100 ms frames. Sent back to back here because this adapter is handed
        // a finished clip; a future streaming-while-holding path feeds the same
        // socket from the ring buffer instead and changes nothing below.
        for chunk in pcm.chunks(CHUNK_SAMPLES * 2) {
            socket
                .send(Message::Text(audio_message(chunk).to_string().into()))
                .await
                .map_err(|e| fail(format!("audio send failed: {e}")))?;
        }

        socket
            .send(Message::Text(audio_stream_end_message().to_string().into()))
            .await
            .map_err(|e| fail(format!("audioStreamEnd send failed: {e}")))?;

        let mut segments: Vec<String> = Vec::new();
        let deadline = Duration::from_millis(FINALIZE_TIMEOUT_MS);
        loop {
            let frame = tokio::time::timeout(deadline, socket.next())
                .await
                .map_err(|_| SttError::Timeout {
                    provider: "gemini-live".to_string(),
                    configured_ms: FINALIZE_TIMEOUT_MS,
                })?;
            let Some(frame) = frame else { break };
            let frame = frame.map_err(|e| fail(format!("socket read failed: {e}")))?;
            match frame {
                Message::Text(body) => match parse_server_message(&body)? {
                    ServerEvent::Final(text) => segments.push(text),
                    ServerEvent::TurnComplete => break,
                    ServerEvent::Interim | ServerEvent::Other => {}
                },
                Message::Close(_) => break,
                _ => {}
            }
        }
        let _ = socket.close(None).await;

        let text = join_segments(&segments);
        if text.is_empty() {
            return Err(fail(format!(
                "session ended with no finalised transcript ({} mode)",
                mode.wire()
            )));
        }
        Ok(text)
    }
}

/// The Gemini Live adapter.
///
/// Holds no loggable copy of the socket URL: it is rebuilt per session from the
/// key so a `{self:?}`-shaped mistake cannot leak it.
pub struct GeminiLive {
    label: String,
    model: String,
    api_key: String,
    bias_terms: Vec<String>,
    mode: TranscribeMode,
    #[cfg(feature = "live")]
    runtime: tokio::runtime::Runtime,
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
            #[cfg(feature = "live")]
            runtime: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| fail(format!("could not start the Live API runtime: {e}")))?,
        })
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
        let text = self
            .runtime
            .block_on(session::run(url, setup, pcm, self.mode))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn terms(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn setup_names_the_model_as_a_resource_path() {
        let m = setup_message("gemini-3.5-transcribe-live", &[], TranscribeMode::Verbatim);
        assert_eq!(
            m["setup"]["model"],
            json!("models/gemini-3.5-transcribe-live")
        );
    }

    #[test]
    fn verbatim_is_the_wire_default_hark_sends() {
        let m = setup_message("m", &[], TranscribeMode::Verbatim);
        assert_eq!(
            m["setup"]["inputAudioTranscription"]["mode"],
            json!("VERBATIM")
        );
        let m = setup_message("m", &[], TranscribeMode::Smart);
        assert_eq!(
            m["setup"]["inputAudioTranscription"]["mode"],
            json!("SMART")
        );
    }

    #[test]
    fn bias_terms_become_custom_vocabulary() {
        let m = setup_message(
            "m",
            &terms(&["Hark", "Levenshtein"]),
            TranscribeMode::Verbatim,
        );
        assert_eq!(
            m["setup"]["inputAudioTranscription"]["customVocabulary"],
            json!(["Hark", "Levenshtein"])
        );
    }

    #[test]
    fn no_terms_omits_the_vocabulary_key_entirely() {
        let m = setup_message("m", &[], TranscribeMode::Verbatim);
        assert!(m["setup"]["inputAudioTranscription"]
            .get("customVocabulary")
            .is_none());
    }

    #[test]
    fn custom_vocabulary_is_capped_at_the_documented_limit() {
        let many: Vec<String> = (0..1_500).map(|i| format!("term{i}")).collect();
        let m = setup_message("m", &many, TranscribeMode::Verbatim);
        let vocab = m["setup"]["inputAudioTranscription"]["customVocabulary"]
            .as_array()
            .expect("vocabulary present");
        assert_eq!(vocab.len(), 1_000);
    }

    #[test]
    fn audio_frames_declare_the_sample_rate_in_the_mime_type() {
        let m = audio_message(&[0u8, 1, 2, 3]);
        assert_eq!(
            m["realtimeInput"]["audio"]["mimeType"],
            json!("audio/pcm;rate=16000")
        );
        assert!(m["realtimeInput"]["audio"]["data"].is_string());
    }

    #[test]
    fn the_end_signal_is_explicit_not_a_silence_heuristic() {
        assert_eq!(
            audio_stream_end_message(),
            json!({ "realtimeInput": { "audioStreamEnd": true } })
        );
    }

    #[test]
    fn samples_encode_as_little_endian_pcm16() {
        // Full scale, silence, negative full scale.
        let pcm = samples_to_pcm16_le(&[1.0, 0.0, -1.0]);
        assert_eq!(pcm, vec![0xFF, 0x7F, 0x00, 0x00, 0x01, 0x80]);
    }

    #[test]
    fn out_of_range_samples_clamp_rather_than_wrapping() {
        // Without the clamp these would wrap to large negative values and the
        // transcript would be garbage rather than merely clipped.
        let pcm = samples_to_pcm16_le(&[2.0, -2.0]);
        assert_eq!(pcm, vec![0xFF, 0x7F, 0x01, 0x80]);
    }

    #[test]
    fn final_transcripts_are_picked_out_of_server_content() {
        let event = parse_server_message(
            r#"{"serverContent":{"inputTranscription":{"text":"hello world"}}}"#,
        )
        .unwrap();
        assert_eq!(event, ServerEvent::Final("hello world".to_string()));
    }

    #[test]
    fn interim_hypotheses_are_recognised_and_not_mistaken_for_finals() {
        let event = parse_server_message(
            r#"{"serverContent":{"interimInputTranscription":{"text":"hel"}}}"#,
        )
        .unwrap();
        assert_eq!(event, ServerEvent::Interim);
    }

    #[test]
    fn turn_complete_ends_the_read_loop() {
        let event = parse_server_message(r#"{"serverContent":{"turnComplete":true}}"#).unwrap();
        assert_eq!(event, ServerEvent::TurnComplete);
    }

    #[test]
    fn setup_acks_and_unknown_frames_are_ignored_not_errors() {
        assert_eq!(
            parse_server_message(r#"{"setupComplete":{}}"#).unwrap(),
            ServerEvent::Other
        );
        assert_eq!(
            parse_server_message(r#"{"serverContent":{"generationComplete":true}}"#).unwrap(),
            ServerEvent::Other
        );
    }

    #[test]
    fn a_non_json_frame_is_an_error_not_a_silent_skip() {
        assert!(parse_server_message("<html>502</html>").is_err());
    }

    #[test]
    fn segments_join_with_a_space_so_a_breath_does_not_fuse_words() {
        let joined = join_segments(&terms(&["hello there", "world"]));
        assert_eq!(joined, "hello there world");
    }

    #[test]
    fn empty_and_whitespace_segments_do_not_produce_double_spaces() {
        let joined = join_segments(&terms(&["hello", "   ", "", "world"]));
        assert_eq!(joined, "hello world");
    }

    #[test]
    fn verbatim_leaves_cleaned_empty_so_the_pipeline_still_runs_cleanup() {
        let t = transcript_for(TranscribeMode::Verbatim, "um hello".to_string(), 42);
        assert_eq!(t.text, "um hello");
        assert!(t.cleaned.is_none());
    }

    #[test]
    fn smart_reports_the_cleaned_text_so_the_pipeline_skips_its_own_call() {
        let t = transcript_for(TranscribeMode::Smart, "Hello.".to_string(), 42);
        assert_eq!(t.text, "Hello.");
        assert_eq!(t.cleaned.as_deref(), Some("Hello."));
    }

    #[test]
    fn the_api_key_never_survives_scrubbing() {
        let leaked = format!(
            "websocket connect failed: io error on {}: refused",
            live_url("sk-secret-value")
        );
        let safe = scrub(&leaked);
        assert!(!safe.contains("sk-secret-value"), "{safe}");
        assert!(safe.contains("key=<redacted>"));
        // The useful part of the message survives.
        assert!(safe.contains("refused"));
    }

    #[test]
    fn scrubbing_handles_a_key_mid_query_and_trailing_params() {
        let safe = scrub("wss://host/path?key=abc123&alt=json then more text");
        assert_eq!(
            safe,
            "wss://host/path?key=<redacted>&alt=json then more text"
        );
    }

    #[test]
    fn scrubbing_is_a_no_op_when_there_is_no_key() {
        assert_eq!(scrub("plain error text"), "plain error text");
    }

    #[test]
    fn every_error_this_module_builds_is_scrubbed() {
        let err = fail(format!("connect to {} failed", live_url("sk-leak")));
        assert!(!format!("{err}").contains("sk-leak"), "{err}");
    }

    #[test]
    fn chunking_matches_one_hundred_milliseconds_of_audio() {
        // 1 s of 16 kHz mono PCM16 = 32 000 bytes = ten 100 ms frames.
        let pcm = samples_to_pcm16_le(&vec![0.0; 16_000]);
        assert_eq!(pcm.chunks(CHUNK_SAMPLES * 2).count(), 10);
    }
}

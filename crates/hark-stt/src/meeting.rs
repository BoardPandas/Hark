//! The Deepgram final pass for meetings: one `multichannel=true&diarize=true`
//! request over a whole stereo recording (left = Me/microphone, right =
//! Them/system audio), replacing the live Me/Them transcript with diarized
//! "Speaker N" utterances within Them. Deepgram diarizes channel 0 too, so
//! `speaker` is forced to `None` there (CP0: it split "Me" into two).
//!
//! The body is streamed rather than buffered (an hour of stereo 16 kHz PCM16
//! is ~115 MB), which reopens the multipart-masks-transport-errors problem
//! documented in LL-G `kb/rust/reqwest-multipart-masks-transport-errors.md`
//! even without the `multipart` feature: a streamed body still hands off to
//! reqwest's internal channel, so a connect/timeout failure during upload can
//! surface as an opaque body-sender error with `is_connect()`/`is_timeout()`
//! both false. [`classify_final_pass_error`] falls back to walking the
//! `source()` chain for the underlying `io::Error`, which keeps its real
//! `ErrorKind`, before giving up and reporting a generic transport error.

use crate::error::{error_for_status, error_for_transport, json_error_detail, SttError};
use crate::openai_compatible::retry_after_secs;
use reqwest::blocking::{Body, Client};
use std::io::Read;
use std::time::Duration;

/// Deepgram's own processing limit is 10 min; this budget also has to cover
/// uploading ~230 MB/h of stereo 16 kHz PCM16, so it is well past that.
pub const FINAL_PASS_TIMEOUT_MS: u64 = 900_000;

/// Container format for a complete stereo meeting, never a mono share export.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeetingEncoding {
    Wav,
    Mp3,
}

impl MeetingEncoding {
    pub fn content_type(self) -> &'static str {
        match self {
            Self::Wav => "audio/wav",
            Self::Mp3 => "audio/mpeg",
        }
    }
}

/// One diarized utterance from the final pass.
pub struct FinalSegment {
    /// 0 = left = Me (microphone), 1 = right = Them (system audio).
    pub channel: u8,
    /// Diarized speaker within the channel; always `None` on channel 0.
    pub speaker: Option<u32>,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// Manual `Debug`: `text` is transcript content and must never reach a log
/// line, so only its length is printed, never the text itself.
impl std::fmt::Debug for FinalSegment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FinalSegment")
            .field("channel", &self.channel)
            .field("speaker", &self.speaker)
            .field("start_ms", &self.start_ms)
            .field("end_ms", &self.end_ms)
            .field("text_len", &self.text.len())
            .finish()
    }
}

/// Build the `/v1/listen` URL for the final pass: fixed `model=nova-3`,
/// `multichannel`, `diarize`, `utterances`, `smart_format`, `punctuate`, plus
/// one repeated `keyterm` param per spellbook term (URL-encoded, same as
/// [`crate::deepgram::listen_url`]). Pure for unit tests.
pub fn final_pass_url(base_url: &str, keyterms: &[String]) -> Result<String, SttError> {
    let base = format!("{}/v1/listen", base_url.trim_end_matches('/'));
    let mut url = reqwest::Url::parse(&base).map_err(|e| SttError::Provider {
        provider: "deepgram".to_string(),
        detail: format!("invalid base_url: {e}"),
    })?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("model", "nova-3");
        q.append_pair("multichannel", "true");
        q.append_pair("diarize", "true");
        q.append_pair("utterances", "true");
        q.append_pair("smart_format", "true");
        q.append_pair("punctuate", "true");
        for term in keyterms {
            q.append_pair("keyterm", term);
        }
    }
    Ok(url.into())
}

/// One raw `results.utterances[]` entry, deserialized straight from Deepgram's
/// shape. `speaker` is nullable (channel 0 sometimes omits it outright, and it
/// is dropped anyway); `start`/`end` are seconds as floats.
#[derive(serde::Deserialize)]
struct RawUtterance {
    channel: u8,
    speaker: Option<u32>,
    start: f64,
    end: f64,
    transcript: String,
}

#[derive(serde::Deserialize)]
struct RawResults {
    #[serde(default)]
    utterances: Vec<RawUtterance>,
}

#[derive(serde::Deserialize)]
struct RawResponse {
    results: Option<RawResults>,
}

fn secs_to_ms_clamped(secs: f64, audio_ms: u64) -> u64 {
    // Deepgram has invented an end time past the audio length before (CP0: a
    // Gemini response, not Deepgram, but the clamp is cheap insurance either
    // way); negative start never happens but `max(0.0)` keeps the cast honest.
    let ms = (secs.max(0.0) * 1000.0).round();
    if ms >= audio_ms as f64 {
        audio_ms
    } else {
        ms as u64
    }
}

/// Parse `results.utterances[]` into ordered, clamped [`FinalSegment`]s. Pure
/// for unit tests.
///
/// Rules: missing `results` is an error (the request nominally succeeded but
/// the shape is not one we can use); a `results` with no `utterances` array,
/// or an empty one, is `Ok(vec![])` (nothing was diarized, not a failure).
/// `speaker` is forced to `None` on channel 0 (Deepgram diarizes it too).
/// `start_ms`/`end_ms` are clamped into `[0, audio_ms]`, with `end_ms` never
/// left below `start_ms`. Blank transcripts are dropped. The result is sorted
/// by `(start_ms, channel)`.
pub fn parse_final_pass(json: &str, audio_ms: u64) -> Result<Vec<FinalSegment>, SttError> {
    let parsed: RawResponse = serde_json::from_str(json).map_err(|e| SttError::Provider {
        provider: "deepgram".to_string(),
        detail: json_error_detail("unexpected response shape", &e),
    })?;
    let results = parsed.results.ok_or_else(|| SttError::Provider {
        provider: "deepgram".to_string(),
        detail: "missing results".to_string(),
    })?;

    if results
        .utterances
        .iter()
        .any(|u| u.channel > 1 || !u.start.is_finite() || !u.end.is_finite())
    {
        return Err(SttError::Provider {
            provider: "deepgram".into(),
            detail: "invalid channel or timestamp in meeting result".into(),
        });
    }

    let mut segments: Vec<FinalSegment> = results
        .utterances
        .into_iter()
        .filter(|u| !u.transcript.trim().is_empty())
        .map(|u| {
            let start_ms = secs_to_ms_clamped(u.start, audio_ms);
            let end_ms = secs_to_ms_clamped(u.end, audio_ms).max(start_ms);
            FinalSegment {
                channel: u.channel,
                speaker: if u.channel == 0 { None } else { u.speaker },
                start_ms,
                end_ms,
                text: u.transcript,
            }
        })
        .collect();

    segments.sort_by_key(|s| (s.start_ms, s.channel));
    Ok(segments)
}

/// Walk the `source()` chain of a transport error for the underlying
/// `io::Error`, which keeps its real `ErrorKind` even when the error type
/// wrapping it (an opaque "receiver is gone" body-sender error, for a
/// streamed upload) does not. Returns `None` when no `io::Error` is anywhere
/// in the chain, so the caller can fall back to a generic transport error.
///
/// Takes `&dyn std::error::Error` rather than `reqwest::Error` specifically
/// so it is unit-testable with a synthetic error chain, without a live
/// socket (`reqwest::Error` has no public constructor for this shape).
fn classify_io_error_in_source_chain(
    label: &str,
    timeout_ms: u64,
    err: &dyn std::error::Error,
) -> Option<SttError> {
    use std::io::ErrorKind;
    let mut cause = err.source();
    while let Some(source) = cause {
        if let Some(io_err) = source.downcast_ref::<std::io::Error>() {
            return Some(match io_err.kind() {
                ErrorKind::TimedOut => SttError::Timeout {
                    provider: label.to_string(),
                    configured_ms: timeout_ms,
                },
                ErrorKind::ConnectionRefused
                | ErrorKind::ConnectionReset
                | ErrorKind::ConnectionAborted
                | ErrorKind::NotConnected
                | ErrorKind::BrokenPipe => SttError::Http {
                    provider: label.to_string(),
                    detail: format!("connect failed ({:?})", io_err.kind()),
                },
                _ => SttError::Http {
                    provider: label.to_string(),
                    detail: format!("I/O failed ({:?})", io_err.kind()),
                },
            });
        }
        cause = source.source();
    }
    None
}

/// Classify a transport error from the streamed final-pass upload.
///
/// [`error_for_transport`]'s `is_timeout()`/`is_connect()` split assumes a
/// buffered body; a streamed one hands off to reqwest's internal channel the
/// same way a multipart body does, so a connect/timeout failure mid-upload
/// can surface as an opaque body-sender error with both flags false (see the
/// module doc). Fall back to [`classify_io_error_in_source_chain`] before
/// giving up on a generic transport error.
fn classify_final_pass_error(label: &str, timeout_ms: u64, err: &reqwest::Error) -> SttError {
    if err.is_timeout() || err.is_connect() {
        return error_for_transport(label, timeout_ms, err);
    }
    classify_io_error_in_source_chain(label, timeout_ms, err)
        .unwrap_or_else(|| error_for_transport(label, timeout_ms, err))
}

/// Run the Deepgram final pass over one complete meeting recording.
///
/// `body` is a streamed 16 kHz stereo i16 WAV (L = Me, R = Them); `body_len`
/// is its exact byte length, required by [`Body::sized`]. Blocking; meant to
/// be called from the meeting worker thread after the session ends, never
/// from a UI thread. Overrides the shared client's default (PTT-sized) total
/// timeout with [`FINAL_PASS_TIMEOUT_MS`] for this one request.
pub fn deepgram_final_pass(
    client: &Client,
    base_url: &str,
    api_key: &str,
    body: Box<dyn Read + Send>,
    body_len: u64,
    keyterms: &[String],
    audio_ms: u64,
) -> Result<Vec<FinalSegment>, SttError> {
    deepgram_final_pass_encoded(
        client,
        base_url,
        api_key,
        body,
        body_len,
        keyterms,
        audio_ms,
        MeetingEncoding::Wav,
    )
}

/// Upload the retained stereo archive unchanged, or the original stereo WAV.
/// The caller can reopen the recording for a retry; this function sends once.
#[allow(clippy::too_many_arguments)]
pub fn deepgram_final_pass_encoded(
    client: &Client,
    base_url: &str,
    api_key: &str,
    body: Box<dyn Read + Send>,
    body_len: u64,
    keyterms: &[String],
    audio_ms: u64,
    encoding: MeetingEncoding,
) -> Result<Vec<FinalSegment>, SttError> {
    let label = "deepgram";
    let url = final_pass_url(base_url, keyterms)?;

    let response = client
        .post(&url)
        .header("Authorization", format!("Token {api_key}"))
        .header("Content-Type", encoding.content_type())
        .timeout(Duration::from_millis(FINAL_PASS_TIMEOUT_MS))
        .body(Body::sized(body, body_len))
        .send()
        .map_err(|e| classify_final_pass_error(label, FINAL_PASS_TIMEOUT_MS, &e))?;

    let status = response.status();
    let retry_after_s = retry_after_secs(response.headers());
    let text = response
        .text()
        .map_err(|e| classify_final_pass_error(label, FINAL_PASS_TIMEOUT_MS, &e))?;

    if !status.is_success() {
        return Err(error_for_status(
            label,
            status.as_u16(),
            retry_after_s,
            &text,
        ));
    }
    parse_final_pass(&text, audio_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|t| t.to_string()).collect()
    }

    // --- URL builder ---

    #[test]
    fn url_has_fixed_query_params_and_repeats_keyterm() {
        let url =
            final_pass_url("https://api.deepgram.com", &s(&["Hark", "edit distance"])).unwrap();
        assert!(url.starts_with("https://api.deepgram.com/v1/listen?"));
        assert!(url.contains("model=nova-3"));
        assert!(url.contains("multichannel=true"));
        assert!(url.contains("diarize=true"));
        assert!(url.contains("utterances=true"));
        assert!(url.contains("smart_format=true"));
        assert!(url.contains("punctuate=true"));
        assert_eq!(url.matches("keyterm=").count(), 2);
        assert!(url.contains("keyterm=edit+distance") || url.contains("keyterm=edit%20distance"));
        assert!(!url.contains(' '));
    }

    #[test]
    fn url_without_keyterms_has_none_and_tolerates_trailing_slash() {
        let url = final_pass_url("https://api.deepgram.com/", &[]).unwrap();
        assert!(!url.contains("keyterm="));
        assert!(url.contains("api.deepgram.com/v1/listen"));
        assert!(!url.contains("//v1"));
    }

    #[test]
    fn url_rejects_an_invalid_base() {
        let err = final_pass_url("not a url", &[]).unwrap_err();
        assert!(matches!(err, SttError::Provider { .. }));
    }

    // --- parsing ---

    fn utterance(channel: u8, speaker: Option<u32>, start: f64, end: f64, text: &str) -> String {
        let speaker = speaker
            .map(|s| s.to_string())
            .unwrap_or_else(|| "null".to_string());
        format!(
            r#"{{"channel":{channel},"speaker":{speaker},"start":{start},"end":{end},"transcript":"{text}"}}"#
        )
    }

    #[test]
    fn channel_zero_speaker_is_always_dropped() {
        let body = format!(
            r#"{{"results":{{"utterances":[{}]}}}}"#,
            utterance(0, Some(1), 0.0, 1.0, "alpha bravo")
        );
        let segments = parse_final_pass(&body, 10_000).unwrap();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].channel, 0);
        assert_eq!(segments[0].speaker, None);
    }

    #[test]
    fn non_zero_channel_keeps_its_speaker() {
        let body = format!(
            r#"{{"results":{{"utterances":[{}]}}}}"#,
            utterance(1, Some(2), 0.0, 1.0, "charlie delta")
        );
        let segments = parse_final_pass(&body, 10_000).unwrap();
        assert_eq!(segments[0].channel, 1);
        assert_eq!(segments[0].speaker, Some(2));
    }

    #[test]
    fn start_and_end_are_clamped_into_the_audio_length() {
        // Mirrors CP0's invented-end-time finding: an end time past the audio
        // length must not survive into a segment.
        let body = format!(
            r#"{{"results":{{"utterances":[{}]}}}}"#,
            utterance(1, Some(0), 9.5, 15.0, "echo foxtrot")
        );
        let segments = parse_final_pass(&body, 10_000).unwrap();
        assert_eq!(segments[0].start_ms, 9_500);
        assert_eq!(segments[0].end_ms, 10_000);
        assert!(segments[0].end_ms >= segments[0].start_ms);
    }

    #[test]
    fn end_never_lands_below_start_after_clamping() {
        // A start already past the clamped audio length would otherwise
        // leave end < start once end is independently clamped down.
        let body = format!(
            r#"{{"results":{{"utterances":[{}]}}}}"#,
            utterance(1, Some(0), 12.0, 12.5, "golf hotel")
        );
        let segments = parse_final_pass(&body, 10_000).unwrap();
        assert_eq!(segments[0].start_ms, 10_000);
        assert_eq!(segments[0].end_ms, 10_000);
    }

    #[test]
    fn blank_transcripts_are_dropped() {
        let body = format!(
            r#"{{"results":{{"utterances":[{},{}]}}}}"#,
            utterance(0, None, 0.0, 1.0, ""),
            utterance(0, None, 1.0, 2.0, "india juliet")
        );
        let segments = parse_final_pass(&body, 10_000).unwrap();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].text, "india juliet");
    }

    #[test]
    fn segments_are_sorted_by_start_then_channel() {
        let body = format!(
            r#"{{"results":{{"utterances":[{},{},{}]}}}}"#,
            utterance(1, Some(0), 5.0, 6.0, "kilo lima"),
            utterance(0, None, 0.0, 1.0, "mike november"),
            utterance(1, Some(1), 0.0, 2.0, "oscar papa"),
        );
        let segments = parse_final_pass(&body, 10_000).unwrap();
        let order: Vec<(u64, u8)> = segments.iter().map(|s| (s.start_ms, s.channel)).collect();
        assert_eq!(order, vec![(0, 0), (0, 1), (5_000, 1)]);
    }

    #[test]
    fn malformed_json_is_a_provider_error() {
        let err = parse_final_pass("not json at all", 10_000).unwrap_err();
        assert!(matches!(err, SttError::Provider { .. }));
    }

    #[test]
    fn missing_results_is_an_error() {
        let err = parse_final_pass(r#"{"metadata":{}}"#, 10_000).unwrap_err();
        assert!(matches!(err, SttError::Provider { .. }));
    }

    #[test]
    fn missing_utterances_array_is_ok_empty() {
        let segments = parse_final_pass(r#"{"results":{}}"#, 10_000).unwrap();
        assert!(segments.is_empty());
    }

    #[test]
    fn empty_utterances_array_is_ok_empty() {
        let segments = parse_final_pass(r#"{"results":{"utterances":[]}}"#, 10_000).unwrap();
        assert!(segments.is_empty());
    }

    // --- Debug hides transcript text ---

    #[test]
    fn debug_prints_only_the_text_length_never_the_text() {
        let segment = FinalSegment {
            channel: 1,
            speaker: Some(2),
            start_ms: 100,
            end_ms: 200,
            text: "quebec romeo secret-looking-words".to_string(),
        };
        let rendered = format!("{segment:?}");
        assert!(!rendered.contains("quebec"));
        assert!(!rendered.contains("secret"));
        assert!(rendered.contains("text_len"));
        assert!(rendered.contains(&segment.text.len().to_string()));
    }

    // --- transport error classification (pure: synthetic error chain, no
    // network -- reqwest::Error has no public constructor for the opaque
    // "receiver is gone" shape a streamed body produces on a dead connection) ---

    /// Stands in for reqwest's opaque body-sender error, with a real
    /// `io::Error` underneath in its `source()` chain.
    #[derive(Debug)]
    struct FakeStreamedBodyError(std::io::Error);

    impl std::fmt::Display for FakeStreamedBodyError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "error sending request for body")
        }
    }

    impl std::error::Error for FakeStreamedBodyError {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&self.0)
        }
    }

    #[derive(Debug)]
    struct OpaqueErrorWithNoSource;

    impl std::fmt::Display for OpaqueErrorWithNoSource {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "send failed because receiver is gone")
        }
    }

    impl std::error::Error for OpaqueErrorWithNoSource {}

    #[test]
    fn source_chain_walk_recovers_a_timed_out_io_error() {
        let wrapped = FakeStreamedBodyError(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "connection timed out",
        ));
        let mapped = classify_io_error_in_source_chain("deepgram", FINAL_PASS_TIMEOUT_MS, &wrapped)
            .expect("io::Error is in the chain");
        assert!(matches!(mapped, SttError::Timeout { .. }));
    }

    #[test]
    fn source_chain_walk_recovers_a_connection_reset_io_error() {
        let wrapped = FakeStreamedBodyError(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "reset by peer",
        ));
        let mapped = classify_io_error_in_source_chain("deepgram", FINAL_PASS_TIMEOUT_MS, &wrapped)
            .expect("io::Error is in the chain");
        match mapped {
            SttError::Http { detail, .. } => assert!(detail.contains("connect failed")),
            other => panic!("expected Http, got {other}"),
        }
    }

    #[test]
    fn source_chain_diagnostics_keep_kind_without_echoing_io_detail() {
        for kind in [
            std::io::ErrorKind::ConnectionReset,
            std::io::ErrorKind::Other,
        ] {
            let wrapped =
                FakeStreamedBodyError(std::io::Error::new(kind, "private_meeting_transcript"));
            let mapped =
                classify_io_error_in_source_chain("deepgram", FINAL_PASS_TIMEOUT_MS, &wrapped)
                    .unwrap();
            let detail = format!("{mapped} {mapped:?}");
            assert!(!detail.contains("private_meeting_transcript"));
            assert!(detail.contains(&format!("{kind:?}")));
        }
    }

    #[test]
    fn source_chain_walk_returns_none_when_no_io_error_is_present() {
        assert!(classify_io_error_in_source_chain(
            "deepgram",
            FINAL_PASS_TIMEOUT_MS,
            &OpaqueErrorWithNoSource
        )
        .is_none());
    }
}

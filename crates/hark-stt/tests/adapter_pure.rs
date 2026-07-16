//! Pure-logic tests for the spike (no network): multipart field assembly,
//! Deepgram URL building, HTTP-status error mapping, percentile math, and the
//! WAV encode helper (validated with `hound`).

use hark_stt::metrics::{contains_term, divergence_ratio, normalize_text, LatencyTally};
use hark_stt::{deepgram, error_for_status, openai_compatible, wav, SttError};
use std::io::Cursor;

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|t| t.to_string()).collect()
}

// --- openai-compatible form assembly ---

#[test]
fn form_fields_include_model_format_language() {
    let fields = openai_compatible::form_text_fields("whisper-large-v3-turbo", None);
    assert!(fields.contains(&("model", "whisper-large-v3-turbo".to_string())));
    assert!(fields.contains(&("response_format", "json".to_string())));
    assert!(fields.contains(&("language", "en".to_string())));
    assert!(!fields.iter().any(|(name, _)| *name == "prompt"));
}

#[test]
fn form_fields_carry_prompt_when_bias_terms_exist() {
    let (prompt, included) =
        openai_compatible::prompt_from_bias_terms(&s(&["Hark", "Levenshtein"]));
    assert_eq!(prompt.as_deref(), Some("Hark, Levenshtein"));
    assert_eq!(included, 2);
    let fields = openai_compatible::form_text_fields("whisper-1", prompt.as_deref());
    assert!(fields.contains(&("prompt", "Hark, Levenshtein".to_string())));
}

#[test]
fn no_bias_terms_means_no_prompt() {
    assert_eq!(openai_compatible::prompt_from_bias_terms(&[]), (None, 0));
}

// --- prompt token budget (Whisper-family models truncate at 224 tokens;
// the cap is 200 tokens at ~4 chars per token = 800 chars) ---

#[test]
fn prompt_under_budget_includes_every_term() {
    let terms = s(&["Modero", "Vossburg", "hark-stt", "nova-3"]);
    let (prompt, included) = openai_compatible::prompt_from_bias_terms(&terms);
    assert_eq!(
        prompt.as_deref(),
        Some("Modero, Vossburg, hark-stt, nova-3")
    );
    assert_eq!(included, 4);
}

#[test]
fn prompt_exactly_at_budget_includes_every_term() {
    // 398 + 2 (separator) + 400 = 800 chars = exactly 200 approx tokens.
    let terms = vec!["a".repeat(398), "b".repeat(400)];
    let (prompt, included) = openai_compatible::prompt_from_bias_terms(&terms);
    assert_eq!(included, 2);
    assert_eq!(prompt.expect("both fit").chars().count(), 800);
}

#[test]
fn prompt_over_budget_drops_terms_beyond_the_cap() {
    // The second term would push the total to 801 chars: dropped, along
    // with everything after it (order is the user's priority signal).
    let terms = vec!["a".repeat(398), "b".repeat(401), "short".to_string()];
    let (prompt, included) = openai_compatible::prompt_from_bias_terms(&terms);
    assert_eq!(included, 1);
    assert_eq!(prompt.expect("first term fits").chars().count(), 398);
}

#[test]
fn prompt_is_omitted_when_even_the_first_term_exceeds_budget() {
    let terms = vec!["x".repeat(900)];
    assert_eq!(openai_compatible::prompt_from_bias_terms(&terms), (None, 0));
}

#[test]
fn transcriptions_url_tolerates_trailing_slash() {
    assert_eq!(
        openai_compatible::transcriptions_url("https://api.groq.com/openai/v1/"),
        "https://api.groq.com/openai/v1/audio/transcriptions"
    );
}

#[test]
fn openai_response_parses_text() {
    let text = openai_compatible::parse_response("openai", r#"{"text":"hello world"}"#).unwrap();
    assert_eq!(text, "hello world");
    let err = openai_compatible::parse_response("openai", "<html>gateway</html>").unwrap_err();
    assert!(matches!(err, SttError::Provider { .. }));
}

#[test]
fn multipart_body_carries_fields_file_and_terminator() {
    let fields = openai_compatible::form_text_fields("whisper-1", Some("Hark, Levenshtein"));
    let file_bytes = b"RIFFfakewav";
    let boundary = openai_compatible::multipart_boundary(file_bytes);
    let body = openai_compatible::build_multipart_body(&boundary, &fields, file_bytes);
    let text = String::from_utf8_lossy(&body);

    assert!(text.contains("Content-Disposition: form-data; name=\"model\"\r\n\r\nwhisper-1\r\n"));
    assert!(text.contains("name=\"response_format\"\r\n\r\njson\r\n"));
    assert!(text.contains("name=\"language\"\r\n\r\nen\r\n"));
    assert!(text.contains("name=\"prompt\"\r\n\r\nHark, Levenshtein\r\n"));
    assert!(text.contains(
        "Content-Disposition: form-data; name=\"file\"; filename=\"spike_clip.wav\"\r\n"
    ));
    assert!(text.contains("Content-Type: audio/wav\r\n\r\nRIFFfakewav"));
    assert!(text.ends_with(&format!("\r\n--{boundary}--\r\n")));
    // Every part opens with the boundary line: 4 text fields + 1 file + terminator.
    assert_eq!(text.matches(&format!("--{boundary}")).count(), 6);
}

#[test]
fn multipart_boundary_never_occurs_in_payload() {
    let base = openai_compatible::multipart_boundary(b"");
    // Adversarial payload that contains the default boundary.
    let payload = format!("xx{base}yy");
    let boundary = openai_compatible::multipart_boundary(payload.as_bytes());
    assert!(!payload.contains(&boundary));
}

// --- deepgram URL building + response parsing ---

#[test]
fn deepgram_url_repeats_keyterm_and_encodes_spaces() {
    let url = deepgram::listen_url(
        "https://api.deepgram.com",
        "nova-3",
        &s(&["Hark", "edit distance"]),
    )
    .unwrap();
    assert!(url.starts_with("https://api.deepgram.com/v1/listen?"));
    assert!(url.contains("model=nova-3"));
    assert!(url.contains("smart_format=true"));
    assert_eq!(url.matches("keyterm=").count(), 2);
    // Multi-word terms must be URL-encoded, never raw spaces.
    assert!(url.contains("keyterm=edit+distance") || url.contains("keyterm=edit%20distance"));
    assert!(!url.contains(' '));
}

#[test]
fn deepgram_url_without_terms_has_no_keyterm() {
    let url = deepgram::listen_url("https://api.deepgram.com/", "nova-3", &[]).unwrap();
    assert!(!url.contains("keyterm="));
    // Trailing slash on base_url must not produce a double slash.
    assert!(url.contains("api.deepgram.com/v1/listen"));
}

#[test]
fn deepgram_response_parses_nested_transcript() {
    let body = r#"{"results":{"channels":[{"alternatives":[{"transcript":"hark uses levenshtein","confidence":0.99}]}]}}"#;
    assert_eq!(
        deepgram::parse_response("deepgram", body).unwrap(),
        "hark uses levenshtein"
    );
    let err = deepgram::parse_response("deepgram", r#"{"err":"nope"}"#).unwrap_err();
    assert!(matches!(err, SttError::Provider { .. }));
}

// --- error taxonomy mapping ---

#[test]
fn status_401_and_403_map_to_auth_without_echoing_body() {
    for status in [401u16, 403] {
        let err = error_for_status(
            "groq",
            status,
            None,
            "{\"error\":\"invalid api key sk-secret\"}",
        );
        assert!(matches!(err, SttError::Auth { .. }));
        // Auth errors must not carry the response body (it can echo key prefixes).
        assert!(!err.to_string().contains("sk-secret"));
    }
}

#[test]
fn status_429_maps_to_rate_limited_with_retry_after() {
    let err = error_for_status("groq", 429, Some(7), "slow down");
    match err {
        SttError::RateLimited {
            ref provider,
            retry_after_s,
        } => {
            assert_eq!(provider, "groq");
            assert_eq!(retry_after_s, Some(7));
        }
        other => panic!("expected RateLimited, got {other}"),
    }
}

#[test]
fn status_500_maps_to_provider_with_truncated_snippet() {
    let long_body = "x".repeat(2_000);
    let err = error_for_status("openai", 500, None, &long_body);
    let msg = err.to_string();
    assert!(matches!(err, SttError::Provider { .. }));
    assert!(msg.contains("HTTP 500"));
    assert!(
        msg.len() < 500,
        "snippet not truncated: {} chars",
        msg.len()
    );
}

// --- percentile math ---

#[test]
fn percentiles_on_known_data() {
    // 20 samples, like the harness's real N: 50, 100, ..., 1000.
    let mut tally = LatencyTally::default();
    for ms in (1..=20u128).map(|i| i * 50) {
        tally.record(ms);
    }
    // Nearest-rank: idx = round((n-1) * pct); round(9.5) = 10, round(18.05) = 18.
    assert_eq!(tally.p50(), Some(550));
    assert_eq!(tally.p95(), Some(950));
    assert_eq!(tally.min(), Some(50));
    assert_eq!(tally.max(), Some(1000));
    assert_eq!(LatencyTally::default().p50(), None);
}

#[test]
fn single_sample_is_every_percentile() {
    let mut tally = LatencyTally::default();
    tally.record(42);
    assert_eq!(tally.p50(), Some(42));
    assert_eq!(tally.p95(), Some(42));
}

// --- wav helper, validated against hound ---

#[test]
fn encoded_wav_is_valid_16k_mono_pcm16() {
    let samples: Vec<f32> = (0..16_000)
        .map(|i| (i as f32 / 16_000.0 * 440.0 * std::f32::consts::TAU).sin() * 0.5)
        .collect();
    let bytes = wav::encode_wav_16k_mono(&samples);

    let reader = hound::WavReader::new(Cursor::new(&bytes)).expect("hound parses our header");
    let spec = reader.spec();
    assert_eq!(spec.sample_rate, 16_000);
    assert_eq!(spec.channels, 1);
    assert_eq!(spec.bits_per_sample, 16);
    assert_eq!(spec.sample_format, hound::SampleFormat::Int);
    assert_eq!(reader.len(), 16_000);
}

#[test]
fn wav_roundtrip_preserves_samples() {
    let samples = vec![0.0f32, 0.5, -0.5, 1.0, -1.0, 0.25];
    let bytes = wav::encode_wav_16k_mono(&samples);
    let parsed = wav::parse_wav_16k_mono(&bytes).unwrap();
    assert_eq!(parsed.samples.len(), samples.len());
    for (a, b) in parsed.samples.iter().zip(&samples) {
        assert!((a - b).abs() < 0.001, "roundtrip drift: {a} vs {b}");
    }
}

#[test]
fn parse_rejects_wrong_rate_and_garbage() {
    // 8 kHz stereo header built with hound, rejected by our validator.
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 8_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec).unwrap();
        writer.write_sample(0i16).unwrap();
        writer.write_sample(0i16).unwrap();
        writer.finalize().unwrap();
    }
    let err = wav::parse_wav_16k_mono(cursor.get_ref()).unwrap_err();
    assert!(matches!(err, SttError::BadAudio(_)));
    assert!(matches!(
        wav::parse_wav_16k_mono(b"not a wav at all"),
        Err(SttError::BadAudio(_))
    ));
}

// --- text helpers the A/B relies on ---

#[test]
fn contains_term_is_case_and_punctuation_insensitive() {
    assert!(contains_term(
        "It uses the levenshtein distance, obviously.",
        "Levenshtein"
    ));
    assert!(!contains_term(
        "It uses the leader distance.",
        "Levenshtein"
    ));
    // Multi-word, and no partial-word false positives.
    assert!(contains_term("the edit distance metric", "edit distance"));
    assert!(!contains_term("harking back", "Hark"));
}

#[test]
fn divergence_ratio_zero_for_formatting_noise() {
    assert_eq!(divergence_ratio("Hello, WORLD!", "hello world"), 0.0);
    assert_eq!(normalize_text("  A  b,C. "), "a b c");
}

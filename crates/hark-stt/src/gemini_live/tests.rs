//! The pure half of the adapter: wire shapes, frame classification, how a
//! transcript is collected and joined, the SMART refusal check, and scrubbing.

use super::*;
use serde_json::{json, Value};

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
fn the_server_is_told_not_to_guess_turn_boundaries() {
    // Push-to-talk knows exactly when speech starts and ends. Leaving the
    // server's VAD on made the session sensitive to how fast audio
    // arrived: a finished clip replayed far faster than real time could
    // leave a turn open forever, emitting interims and no final.
    let m = setup_message("m", &[], TranscribeMode::Verbatim);
    assert_eq!(
        m["setup"]["realtimeInputConfig"]["automaticActivityDetection"]["disabled"],
        json!(true)
    );
}

#[test]
fn the_turn_is_opened_and_closed_by_hand() {
    // NOTE: this proves the messages are well-formed, NOT that they are
    // sent. It passed for a release in which `activityEnd` was built,
    // tested here, and never written to a socket -- so the server was
    // never told the turn had ended. Anything asserted in this test is
    // only worth as much as the send site in `session::finish`; check
    // there before trusting it.
    assert_eq!(
        activity_start_message(),
        json!({ "realtimeInput": { "activityStart": {} } })
    );
    assert_eq!(
        activity_end_message(),
        json!({ "realtimeInput": { "activityEnd": {} } })
    );
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
    let event =
        parse_server_message(r#"{"serverContent":{"inputTranscription":{"text":"hello world"}}}"#)
            .unwrap();
    assert_eq!(event, ServerEvent::Final("hello world".to_string()));
}

#[test]
fn interim_hypotheses_are_recognised_and_not_mistaken_for_finals() {
    let event =
        parse_server_message(r#"{"serverContent":{"interimInputTranscription":{"text":"hel"}}}"#)
            .unwrap();
    assert_eq!(event, ServerEvent::Interim);
}

#[test]
fn generation_complete_ends_the_turn() {
    // What the real API actually sends. Treating it as noise meant
    // collecting the transcript and then timing out discarding it.
    let event = parse_server_message(r#"{"serverContent":{"generationComplete":true}}"#).unwrap();
    assert_eq!(event, ServerEvent::TurnComplete);
}

#[test]
fn voice_activity_frames_are_not_mistaken_for_content() {
    // {"serverContent":{}, "voiceActivity":{...}} brackets every turn.
    let event = parse_server_message(
        r#"{"serverContent":{},"voiceActivity":{"type":"ACTIVITY_END","audioOffset":"10.3s"}}"#,
    )
    .unwrap();
    assert_eq!(event, ServerEvent::Other);
}

#[test]
fn turn_complete_ends_the_read_loop() {
    let event = parse_server_message(r#"{"serverContent":{"turnComplete":true}}"#).unwrap();
    assert_eq!(event, ServerEvent::TurnComplete);
}

#[test]
fn the_setup_ack_is_recognised_rather_than_treated_as_noise() {
    // Sending audio before this arrives gets it discarded, and the session
    // then finalises an empty turn -- an 8 s timeout with no stated cause.
    assert_eq!(
        parse_server_message(r#"{"setupComplete":{}}"#).unwrap(),
        ServerEvent::SetupComplete
    );
}

#[test]
fn unknown_frames_are_ignored_not_errors() {
    assert_eq!(
        parse_server_message(r#"{"usageMetadata":{"totalTokenCount":5}}"#).unwrap(),
        ServerEvent::Other
    );
}

#[test]
fn diagnostics_name_a_frames_keys_without_printing_its_contents() {
    let v: Value = serde_json::from_str(r#"{"usageMetadata":{"totalTokenCount":5}}"#).unwrap();
    assert_eq!(top_keys(&v), vec!["usageMetadata".to_string()]);
}

#[test]
fn a_non_json_frame_is_an_error_not_a_silent_skip() {
    assert!(parse_server_message("<html>502</html>").is_err());
}

#[test]
fn a_transcript_frame_is_kept_not_merely_classified() {
    // The regression: frames were parsed and then dropped, so everything
    // said before the last pause never reached the injected text.
    let mut segments = Vec::new();
    let event = absorb(
        r#"{"serverContent":{"inputTranscription":{"text":"hello world"}}}"#,
        &mut segments,
    )
    .unwrap();
    assert_eq!(event, ServerEvent::Final("hello world".to_string()));
    assert_eq!(segments, vec!["hello world".to_string()]);
}

#[test]
fn everything_said_during_the_hold_survives_to_the_transcript() {
    // One real dictation as the wire delivers it: the server finalises a
    // segment per pause *while the key is still down*, interleaved with
    // interims, and closes the turn only after the key comes up. Hark used
    // to inject "and that is all" -- the tail alone.
    let during_the_hold = [
        r#"{"serverContent":{"interimInputTranscription":{"text":"the quick"}}}"#,
        r#"{"serverContent":{"inputTranscription":{"text":"the quick brown fox"}}}"#,
        r#"{"serverContent":{"interimInputTranscription":{"text":"jumps"}}}"#,
        r#"{"serverContent":{"inputTranscription":{"text":"jumps over the lazy dog"}}}"#,
    ];
    let after_the_key_came_up = [
        r#"{"serverContent":{"inputTranscription":{"text":"and that is all"}}}"#,
        r#"{"serverContent":{"generationComplete":true}}"#,
    ];

    let mut segments = Vec::new();
    for body in during_the_hold {
        absorb(body, &mut segments).unwrap();
    }
    assert_eq!(
        segments.len(),
        2,
        "mid-hold finals must accumulate, not be discarded"
    );
    let mut complete = false;
    for body in after_the_key_came_up {
        complete |= absorb(body, &mut segments).unwrap() == ServerEvent::TurnComplete;
    }
    assert!(complete);
    assert_eq!(
        join_segments(&segments),
        "the quick brown fox jumps over the lazy dog and that is all"
    );
}

#[test]
fn interims_and_noise_contribute_nothing_to_the_transcript() {
    // Only finalised text may be injected; a revised hypothesis or a
    // keepalive landing in the transcript would be worse than losing it.
    let mut segments = Vec::new();
    for body in [
        r#"{"serverContent":{"interimInputTranscription":{"text":"hel"}}}"#,
        r#"{"usageMetadata":{"totalTokenCount":5}}"#,
        r#"{"setupComplete":{}}"#,
        r#"{"serverContent":{},"voiceActivity":{"type":"ACTIVITY_END"}}"#,
    ] {
        absorb(body, &mut segments).unwrap();
    }
    assert!(segments.is_empty());
}

#[test]
fn a_malformed_frame_leaves_what_was_already_collected_intact() {
    // Finalise propagates this error; the mid-hold drain swallows it. What
    // neither may do is lose the transcript collected up to that point.
    let mut segments = Vec::new();
    absorb(
        r#"{"serverContent":{"inputTranscription":{"text":"keep me"}}}"#,
        &mut segments,
    )
    .unwrap();
    assert!(absorb("<html>502</html>", &mut segments).is_err());
    assert_eq!(segments, vec!["keep me".to_string()]);
}

// The relationship between FINALIZE_TOTAL_MS and FINALIZE_TIMEOUT_MS is
// asserted where those constants are defined, not here: both are consts,
// so it holds at compile time and a violation is a build error rather than
// a test failure.

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
fn gemini_canned_refusals_are_recognised() {
    for refusal in [
        "I'm just a language model and can't help with that.",
        "I\u{2019}m just a language model, so I can\u{2019}t help you with that.",
        "I'm a text-based AI and can't assist with that.",
        "As a language model, I'm not able to assist you with that.",
        "I'm unable to help, as I am only a language model and don't have the \
         ability to process and understand that.",
        "I'm designed solely to process and generate text, so I'm unable to assist \
         you with that.",
    ] {
        assert!(is_canned_refusal(refusal), "{refusal}");
    }
}

#[test]
fn ordinary_dictations_are_not_refusals() {
    for said in [
        "Add the following MCP server.",
        "I can't help with that today, I'm out of the office.",
        "Users are trying to send emails to other Reddoor addresses from Gmail.",
        // Talking about language models at length is not the canned reply.
        "So the issue is that the language model can't help but repeat itself \
         whenever the context window fills up, which means we need to trim the \
         history before every call and keep the system prompt short enough that \
         it never gets pushed out of the window by a long transcript.",
    ] {
        assert!(!is_canned_refusal(said), "{said}");
    }
}

#[test]
fn a_smart_refusal_becomes_an_error_so_the_clip_replays_verbatim() {
    let refusal = "I'm just a language model and can't help with that.";
    let err = checked_text(TranscribeMode::Smart, refusal.to_string()).unwrap_err();
    // The error is a log line; the refusal is transcript content.
    assert!(!err.to_string().contains("language model"), "{err}");
    assert_eq!(
        checked_text(TranscribeMode::Smart, "Hello there.".to_string()).unwrap(),
        "Hello there."
    );
}

#[test]
fn verbatim_text_is_never_second_guessed() {
    // In VERBATIM, this sentence is what the user said.
    let said = "I'm just a language model and can't help with that.";
    assert_eq!(
        checked_text(TranscribeMode::Verbatim, said.to_string()).unwrap(),
        said
    );
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

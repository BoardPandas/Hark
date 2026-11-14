//! The pure half of the adapter: wire shapes, event classification, the
//! turn state machine, error mapping and scrubbing.

use super::wire::IGNORED_EVENTS;
use super::*;
use crate::wav::samples_to_pcm16_le;
use serde_json::{json, Value};

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
        .absorb(&json!({"type": "input_audio_buffer.committed", "item_id": "item_1"}).to_string())
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
            &json!({"type": "input_audio_buffer.committed", "item_id": "item_ours"}).to_string()
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

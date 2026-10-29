use hark_stt::{
    deepgram, error_for_status, error_for_transport, gemini, gemini_live, meeting,
    openai_compatible, openai_live,
};

const PRIVATE: &str = "PRIVATE_TRANSCRIPT_sk_sensitive";

fn assert_private(error: impl std::fmt::Display + std::fmt::Debug) {
    let rendered = format!("{error} {error:?}");
    assert!(
        !rendered.contains(PRIVATE),
        "error leaked provider content: {rendered}"
    );
}

#[test]
fn status_errors_never_echo_body_or_unrecognized_reason_codes() {
    for status in [400, 401, 403, 429, 500, 502] {
        let body = format!(
            r#"{{"error":{{"message":"{PRIVATE}","code":"{PRIVATE}","type":"{PRIVATE}"}}}}"#
        );
        assert_private(error_for_status("test", status, None, &body));
    }
}

#[test]
fn batch_and_meeting_parse_errors_never_echo_content() {
    for body in [
        format!("<html>{PRIVATE}</html>"),
        format!(r#"{{"results":"{PRIVATE}","unexpected":"{PRIVATE}"}}"#),
    ] {
        assert_private(openai_compatible::parse_response("test", &body).unwrap_err());
        assert_private(deepgram::parse_response("test", &body).unwrap_err());
        assert_private(gemini::parse_response("test", &body, true).unwrap_err());
        assert_private(meeting::parse_final_pass(&body, 1000).unwrap_err());
        if !body.starts_with('{') {
            assert_private(gemini_live::parse_server_message(&body).unwrap_err());
            assert_private(openai_live::parse_server_message(&body).unwrap_err());
        }
    }
    let body =
        serde_json::json!({"output_text": format!(r#"{{"raw":"{PRIVATE}","cleaned":{{}}}}"#)})
            .to_string();
    assert_private(gemini::parse_response("test", &body, true).unwrap_err());
}

#[test]
fn parser_errors_preserve_structural_category_and_location() {
    let body = format!(r#"{{"text":{{"{PRIVATE}":true}}}}"#);
    let error = openai_compatible::parse_response("test", &body).unwrap_err();
    let detail = error.to_string();
    assert!(detail.contains("JSON Data at line 1, column"));
    assert_private(error);
}

#[test]
fn transport_errors_never_echo_urls_or_query_values() {
    let error = reqwest::blocking::Client::new()
        .get("http://[")
        .build()
        .unwrap_err()
        .with_url(
            reqwest::Url::parse(&format!("https://example.invalid/?keyterm={PRIVATE}")).unwrap(),
        );
    assert_private(error_for_transport("test", 1000, &error));
}

#[test]
fn live_diagnostics_do_not_echo_unknown_json_keys() {
    let body = serde_json::json!({PRIVATE: "irrelevant", "usageMetadata": {}});
    assert!(!gemini_live::top_keys(&body).join(" ").contains(PRIVATE));
}

#[test]
fn openai_live_errors_never_echo_the_message_or_a_quoted_key() {
    // OpenAI's realtime errors quote a masked key in `message`; a completed
    // event missing its transcript must not echo the frame either.
    for body in [
        format!(
            r#"{{"type":"error","error":{{"code":"{PRIVATE}","message":"Incorrect API key provided: sk-proj-****{PRIVATE}"}}}}"#
        ),
        format!(
            r#"{{"type":"conversation.item.input_audio_transcription.completed","item_id":"{PRIVATE}"}}"#
        ),
    ] {
        match openai_live::parse_server_message(&body) {
            Ok(event) => assert!(!format!("{event:?}").contains("sk-")),
            Err(error) => assert_private(error),
        }
    }
    assert_private(openai_live::close_error(3000, PRIVATE, "mid-stream"));
    assert_private(openai_live::close_error(1011, PRIVATE, "mid-stream"));
}

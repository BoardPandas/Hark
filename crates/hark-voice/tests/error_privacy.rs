use hark_voice::{
    error_for_status, error_for_transport, openai_compatible::parse_response, parse_notes,
    MeetingNotes,
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
fn cleanup_shape_errors_and_finish_reasons_never_echo_content() {
    for body in [
        format!("<html>{PRIVATE}</html>"),
        format!(r#"{{"choices":"{PRIVATE}"}}"#),
        format!(r#"{{"choices":[{{"message":{{"content":""}},"finish_reason":"{PRIVATE}"}}]}}"#),
    ] {
        assert_private(parse_response("test", &body).unwrap_err());
    }
}

#[test]
fn summary_and_stored_notes_errors_never_echo_content() {
    for body in [
        PRIVATE.to_string(),
        format!(r#"{{"summary":"{PRIVATE}""#),
        format!(r#"{{"summary":"safe","key_points":"{PRIVATE}"}}"#),
    ] {
        assert_private(parse_notes(&body).unwrap_err());
    }
    let invalid = format!(
        r#"{{"title":"safe","summary":"safe","key_points":[],"decisions":[],"action_items":[{{"text":"safe","done":"{PRIVATE}"}}]}}"#
    );
    assert_private(MeetingNotes::from_json(&invalid).unwrap_err());
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
fn parser_errors_preserve_structural_category_and_location() {
    let error = parse_response("test", &format!(r#"{{"choices":"{PRIVATE}"}}"#)).unwrap_err();
    assert!(error.to_string().contains("JSON Data at line 1, column"));
    assert_private(error);
}

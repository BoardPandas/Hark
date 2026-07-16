//! Pure-logic tests for the chat-completions layer (no network): URL
//! building, request-body assembly (temperature/effort omission), the
//! max_completion_tokens derivation, response parsing, and HTTP-status error
//! mapping.

use hark_voice::openai_compatible::{
    build_request_body, chat_completions_url, max_completion_tokens, parse_response,
    retry_after_secs, CleanupConfig, OpenAiCompatibleChat, MAX_COMPLETION_TOKENS_CAP,
    MAX_COMPLETION_TOKENS_FLOOR,
};
use hark_voice::{error_for_status, CleanupError, CleanupProvider, Voice};

fn body_json(bytes: &[u8]) -> serde_json::Value {
    serde_json::from_slice(bytes).expect("request body is valid JSON")
}

// --- URL building ---

#[test]
fn chat_completions_url_appends_path() {
    assert_eq!(
        chat_completions_url("https://api.openai.com/v1"),
        "https://api.openai.com/v1/chat/completions"
    );
}

#[test]
fn chat_completions_url_tolerates_trailing_slash() {
    assert_eq!(
        chat_completions_url("https://api.groq.com/openai/v1/"),
        "https://api.groq.com/openai/v1/chat/completions"
    );
}

// --- request body assembly ---

#[test]
fn body_carries_model_messages_and_token_cap() {
    let bytes = build_request_body("gpt-5-nano", "You clean text.", "hello there", None, None);
    let v = body_json(&bytes);
    assert_eq!(v["model"], "gpt-5-nano");
    assert_eq!(v["messages"][0]["role"], "system");
    assert_eq!(v["messages"][0]["content"], "You clean text.");
    assert_eq!(v["messages"][1]["role"], "user");
    assert_eq!(v["messages"][1]["content"], "hello there");
    assert_eq!(
        v["max_completion_tokens"],
        u64::from(max_completion_tokens("hello there"))
    );
    assert_eq!(v["messages"].as_array().map(Vec::len), Some(2));
}

#[test]
fn temperature_and_effort_are_omitted_when_unset() {
    // GPT-5 family rejects any non-default temperature with a 400, so the
    // fields must be absent, not null.
    let bytes = build_request_body("gpt-5-nano", "sys", "user", None, None);
    let v = body_json(&bytes);
    assert!(v.get("temperature").is_none());
    assert!(v.get("reasoning_effort").is_none());
}

#[test]
fn temperature_and_effort_are_serialized_when_set() {
    let bytes = build_request_body(
        "llama-3.1-8b-instant",
        "sys",
        "user",
        Some(0.2),
        Some("minimal"),
    );
    let v = body_json(&bytes);
    assert!((v["temperature"].as_f64().expect("temperature is a number") - 0.2).abs() < 1e-6);
    assert_eq!(v["reasoning_effort"], "minimal");
}

// --- max_completion_tokens derivation: 2 * (chars/4) + 256, clamp [512, 4096] ---

#[test]
fn token_cap_floor_covers_short_inputs() {
    // Empty and short inputs land below the floor: 2*0 + 256 = 256 -> 512.
    assert_eq!(
        max_completion_tokens(""),
        MAX_COMPLETION_TOKENS_FLOOR as u32
    );
    assert_eq!(
        max_completion_tokens("send it"),
        MAX_COMPLETION_TOKENS_FLOOR as u32
    );
}

#[test]
fn token_cap_scales_with_input_length() {
    // 2048 chars -> estimate 512 tokens -> 2*512 + 256 = 1280.
    assert_eq!(max_completion_tokens(&"x".repeat(2048)), 1280);
}

#[test]
fn token_cap_is_clamped_at_the_ceiling() {
    // 7680 chars -> estimate 1920 -> exactly 4096; anything longer clamps.
    assert_eq!(
        max_completion_tokens(&"x".repeat(7680)),
        MAX_COMPLETION_TOKENS_CAP as u32
    );
    assert_eq!(
        max_completion_tokens(&"x".repeat(50_000)),
        MAX_COMPLETION_TOKENS_CAP as u32
    );
}

#[test]
fn token_cap_counts_chars_not_bytes() {
    // Multibyte chars must not inflate the estimate.
    let ascii = "a".repeat(2048);
    let multibyte = "é".repeat(2048);
    assert_eq!(
        max_completion_tokens(&ascii),
        max_completion_tokens(&multibyte)
    );
}

// --- response parsing ---

#[test]
fn parse_response_extracts_and_trims_content() {
    let body = r#"{"choices":[{"message":{"role":"assistant","content":"  Cleaned text.\n"},"finish_reason":"stop"}],"usage":{"completion_tokens":12}}"#;
    assert_eq!(
        parse_response("openai", body).expect("valid body parses"),
        "Cleaned text."
    );
}

#[test]
fn parse_response_rejects_missing_choices() {
    let err = parse_response("openai", r#"{"choices":[]}"#).unwrap_err();
    match err {
        CleanupError::Provider { detail, .. } => assert!(detail.contains("no choices")),
        other => panic!("expected Provider, got {other}"),
    }
}

#[test]
fn parse_response_rejects_empty_content_and_names_finish_reason() {
    // Reasoning tokens exhausting the budget yields empty content with
    // finish_reason "length"; the error detail must surface that.
    let body =
        r#"{"choices":[{"message":{"role":"assistant","content":""},"finish_reason":"length"}]}"#;
    let err = parse_response("openai", body).unwrap_err();
    match err {
        CleanupError::Provider { detail, .. } => assert!(detail.contains("length")),
        other => panic!("expected Provider, got {other}"),
    }
}

#[test]
fn parse_response_rejects_null_content() {
    let body =
        r#"{"choices":[{"message":{"role":"assistant","content":null},"finish_reason":"length"}]}"#;
    assert!(matches!(
        parse_response("openai", body),
        Err(CleanupError::Provider { .. })
    ));
}

#[test]
fn parse_response_rejects_whitespace_only_content() {
    let body = r#"{"choices":[{"message":{"content":"   \n  "},"finish_reason":"stop"}]}"#;
    assert!(matches!(
        parse_response("openai", body),
        Err(CleanupError::Provider { .. })
    ));
}

#[test]
fn parse_response_rejects_junk_with_snippet() {
    let err = parse_response("groq", "<html>gateway timeout</html>").unwrap_err();
    match err {
        CleanupError::Provider { detail, .. } => {
            assert!(detail.contains("unexpected response body"));
            assert!(detail.contains("gateway timeout"));
        }
        other => panic!("expected Provider, got {other}"),
    }
}

// --- HTTP status error mapping ---

#[test]
fn status_401_and_403_map_to_auth_without_body_echo() {
    for status in [401, 403] {
        let err = error_for_status("openai", status, None, r#"{"error":{"message":"bad key"}}"#);
        match err {
            CleanupError::Auth { ref provider } => assert_eq!(provider, "openai"),
            ref other => panic!("expected Auth, got {other}"),
        }
        // The rendered message must not echo the response body.
        assert!(!err.to_string().contains("bad key"));
    }
}

#[test]
fn status_429_maps_to_rate_limited_with_retry_after() {
    let err = error_for_status("groq", 429, Some(7), "{}");
    match err {
        CleanupError::RateLimited {
            provider,
            retry_after_s,
        } => {
            assert_eq!(provider, "groq");
            assert_eq!(retry_after_s, Some(7));
        }
        other => panic!("expected RateLimited, got {other}"),
    }
}

#[test]
fn other_statuses_map_to_provider_with_truncated_snippet() {
    let long_body = "x".repeat(1000);
    let err = error_for_status("openai", 500, None, &long_body);
    match err {
        CleanupError::Provider { detail, .. } => {
            assert!(detail.starts_with("HTTP 500:"));
            assert!(detail.ends_with('…'));
            // 300-char snippet cap plus the prefix; nowhere near 1000.
            assert!(detail.chars().count() < 320);
        }
        other => panic!("expected Provider, got {other}"),
    }
}

// --- adapter construction (the live path is thin over the pure layer;
// network behavior is proven by the spike and re-proven at the live gate) ---

fn config(voice: Voice) -> CleanupConfig {
    CleanupConfig {
        label: "openai".to_string(),
        base_url: "https://api.openai.com/v1".to_string(),
        model: "gpt-5-nano".to_string(),
        api_key: "sk-SENTINEL-NEVER-IN-LOGS".to_string(),
        temperature: None,
        reasoning_effort: Some("minimal".to_string()),
        voice,
        custom_prompt: String::new(),
        dictionary_terms: vec!["Hark".to_string()],
    }
}

fn client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::new()
}

#[test]
fn adapter_builds_and_reports_its_label() {
    let adapter =
        OpenAiCompatibleChat::new(&config(Voice::Clean), client()).expect("clean voice constructs");
    assert_eq!(adapter.label(), "openai");
}

#[test]
fn adapter_is_send_for_the_worker_thread() {
    fn require_send<T: Send>(_: &T) {}
    let adapter = OpenAiCompatibleChat::new(&config(Voice::Clean), client()).unwrap();
    require_send(&adapter);
}

#[test]
fn verbatim_voice_is_rejected_at_construction() {
    // No expect_err: the adapter (deliberately) has no Debug impl.
    let err = match OpenAiCompatibleChat::new(&config(Voice::Verbatim), client()) {
        Ok(_) => panic!("verbatim must not construct an adapter"),
        Err(e) => e,
    };
    match &err {
        CleanupError::Provider { detail, .. } => assert!(detail.contains("verbatim")),
        other => panic!("expected Provider, got {other}"),
    }
    // The construction error must not leak the key either.
    assert!(!err.to_string().contains("SENTINEL"));
}

#[test]
fn cleanup_config_debug_never_leaks_key_or_user_content() {
    let mut cfg = config(Voice::Custom);
    cfg.custom_prompt = "SECRET custom prompt".to_string();
    let debug = format!("{cfg:?}");
    assert!(!debug.contains("SENTINEL"), "api_key leaked: {debug}");
    assert!(!debug.contains("SECRET"), "custom prompt leaked: {debug}");
    assert!(debug.contains("<redacted>"));
}

// --- Retry-After parsing ---

#[test]
fn retry_after_parses_seconds_form() {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(reqwest::header::RETRY_AFTER, " 12 ".parse().unwrap());
    assert_eq!(retry_after_secs(&headers), Some(12));
}

#[test]
fn retry_after_ignores_missing_or_unparseable_values() {
    assert_eq!(retry_after_secs(&reqwest::header::HeaderMap::new()), None);
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::RETRY_AFTER,
        "Wed, 21 Oct 2026 07:28:00 GMT".parse().unwrap(),
    );
    assert_eq!(retry_after_secs(&headers), None);
}

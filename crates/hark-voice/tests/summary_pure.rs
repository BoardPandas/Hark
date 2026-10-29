//! Pure-logic tests for the meeting-notes summary layer (no network): request
//! building (model, template default/override, output clause, timeout/format
//! flags, transcript placement), `parse_notes` variants (fenced/prose JSON,
//! missing arrays, empty-item dropping, oversize and multibyte truncation,
//! empty-summary rejection), the `to_json`/`from_json` round trip, and that
//! `Debug` hides transcript-derived content.

use hark_voice::{
    build_summary_request, parse_notes, ActionItem, MeetingNotes, SpeakerRefs, SummaryConfig,
    SummaryError, DEFAULT_SUMMARY_TEMPLATE,
};

fn config() -> SummaryConfig {
    SummaryConfig {
        label: "openai".to_string(),
        base_url: "https://api.openai.com/v1".to_string(),
        model: "gpt-5-nano".to_string(),
        api_key: "sk-SENTINEL-NEVER-IN-LOGS".to_string(),
        temperature: None,
        reasoning_effort: Some("minimal".to_string()),
    }
}

// --- request building ---

#[test]
fn request_carries_model_and_transcript_in_user_message() {
    let v = build_summary_request(&config(), "[12:04] Me: let's ship it", None);
    assert_eq!(v["model"], "gpt-5-nano");
    assert_eq!(v["messages"][0]["role"], "system");
    assert_eq!(v["messages"][1]["role"], "user");
    assert_eq!(v["messages"][1]["content"], "[12:04] Me: let's ship it");
    assert_eq!(v["messages"].as_array().map(Vec::len), Some(2));
}

#[test]
fn request_uses_default_template_when_none_given() {
    let v = build_summary_request(&config(), "transcript", None);
    let system = v["messages"][0]["content"].as_str().unwrap();
    assert!(system.contains(DEFAULT_SUMMARY_TEMPLATE));
}

#[test]
fn request_uses_caller_template_when_given() {
    let v = build_summary_request(&config(), "transcript", Some("Summarize this 1:1."));
    let system = v["messages"][0]["content"].as_str().unwrap();
    assert!(system.contains("Summarize this 1:1."));
    assert!(!system.contains(DEFAULT_SUMMARY_TEMPLATE));
}

#[test]
fn output_clause_is_always_present_default_and_custom_template() {
    for template in [None, Some("A custom template with no JSON mention at all.")] {
        let v = build_summary_request(&config(), "transcript", template);
        let system = v["messages"][0]["content"].as_str().unwrap();
        // The schema clause names every required key; presence of one that
        // no template would plausibly contain on its own is enough to prove
        // it was appended rather than supplied by the template.
        assert!(system.contains("action_items"), "missing for {template:?}");
        assert!(system.contains("key_points"), "missing for {template:?}");
    }
}

#[test]
fn request_sets_json_response_format() {
    let v = build_summary_request(&config(), "transcript", None);
    assert_eq!(v["response_format"]["type"], "json_object");
}

#[test]
fn temperature_and_effort_follow_config_like_cleanup() {
    let mut cfg = config();
    cfg.temperature = Some(0.2);
    cfg.reasoning_effort = None;
    let v = build_summary_request(&cfg, "transcript", None);
    assert!((v["temperature"].as_f64().unwrap() - 0.2).abs() < 1e-6);
    assert!(v.get("reasoning_effort").is_none());

    let v = build_summary_request(&config(), "transcript", None);
    assert!(v.get("temperature").is_none());
    assert_eq!(v["reasoning_effort"], "minimal");
}

// --- parse_notes: happy paths ---

fn full_json() -> &'static str {
    r#"{"title":"Sprint planning","summary":"We planned the sprint.",
        "key_points":["Scope agreed","Risks flagged"],
        "decisions":["Ship behind a flag"],
        "action_items":[{"text":"File the ticket","owner":"Dana","done":false}]}"#
}

#[test]
fn parse_notes_accepts_raw_json() {
    let notes = parse_notes(full_json(), &[]).expect("valid notes parse");
    assert_eq!(notes.title, "Sprint planning");
    assert_eq!(notes.summary, "We planned the sprint.");
    assert_eq!(notes.key_points, vec!["Scope agreed", "Risks flagged"]);
    assert_eq!(notes.decisions, vec!["Ship behind a flag"]);
    assert_eq!(notes.action_items.len(), 1);
    assert_eq!(notes.action_items[0].text, "File the ticket");
    assert_eq!(notes.action_items[0].owner.as_deref(), Some("Dana"));
    assert!(!notes.action_items[0].done);
}

#[test]
fn parse_notes_accepts_fenced_json() {
    let fenced = format!("```json\n{}\n```", full_json());
    let notes = parse_notes(&fenced, &[]).expect("fenced notes parse");
    assert_eq!(notes.title, "Sprint planning");
}

#[test]
fn parse_notes_accepts_prose_around_json() {
    let prose = format!(
        "Sure, here are the notes:\n\n{}\n\nLet me know!",
        full_json()
    );
    let notes = parse_notes(&prose, &[]).expect("prose-wrapped notes parse");
    assert_eq!(notes.title, "Sprint planning");
}

#[test]
fn parse_notes_ignores_braces_inside_string_values() {
    let body = r#"{"title":"t","summary":"has a { brace } in it","key_points":[],
        "decisions":[],"action_items":[]}"#;
    let notes = parse_notes(body, &[]).expect("brace-in-string body parses");
    // Braces that name no speaker are stripped: raw braces never reach the
    // user, and the JSON extraction still was not fooled by them.
    assert_eq!(notes.summary, "has a brace in it");
}

#[test]
fn parse_notes_defaults_missing_arrays_to_empty() {
    let body = r#"{"title":"t","summary":"s"}"#;
    let notes = parse_notes(body, &[]).expect("missing arrays default to empty");
    assert!(notes.key_points.is_empty());
    assert!(notes.decisions.is_empty());
    assert!(notes.action_items.is_empty());
}

#[test]
fn parse_notes_trims_and_drops_empty_list_items() {
    let body = r#"{"title":"t","summary":"s","key_points":["  ","kept  ",""],
        "decisions":[],"action_items":[{"text":"   ","owner":null,"done":false}]}"#;
    let notes = parse_notes(body, &[]).expect("body with blanks parses");
    assert_eq!(notes.key_points, vec!["kept"]);
    assert!(notes.action_items.is_empty());
}

#[test]
fn parse_notes_drops_empty_owner_but_keeps_item() {
    let body = r#"{"title":"t","summary":"s","key_points":[],"decisions":[],
        "action_items":[{"text":"do it","owner":"  ","done":false}]}"#;
    let notes = parse_notes(body, &[]).expect("blank owner still parses");
    assert_eq!(notes.action_items[0].owner, None);
}

// --- parse_notes: rejection and error taxonomy ---

#[test]
fn parse_notes_rejects_empty_summary() {
    let body = r#"{"title":"t","summary":"   ","key_points":[],"decisions":[],"action_items":[]}"#;
    let err = parse_notes(body, &[]).unwrap_err();
    match err {
        SummaryError::Provider { detail, .. } => assert!(detail.contains("empty summary")),
        other => panic!("expected Provider, got {other}"),
    }
}

#[test]
fn parse_notes_rejects_no_json_object() {
    let err = parse_notes("no JSON here at all", &[]).unwrap_err();
    match err {
        SummaryError::Provider { detail, .. } => assert!(detail.contains("no JSON object")),
        other => panic!("expected Provider, got {other}"),
    }
}

#[test]
fn parse_notes_rejects_unterminated_json() {
    let err = parse_notes(r#"{"title":"t","summary":"s""#, &[]).unwrap_err();
    match err {
        SummaryError::Provider { detail, .. } => assert!(detail.contains("unterminated")),
        other => panic!("expected Provider, got {other}"),
    }
}

#[test]
fn parse_notes_rejects_junk_json_with_safe_diagnostics() {
    let err = parse_notes("{not valid json}", &[]).unwrap_err();
    match err {
        SummaryError::Provider { detail, .. } => assert!(detail.contains("unexpected notes body")),
        other => panic!("expected Provider, got {other}"),
    }
}

// --- oversize and multibyte truncation ---

#[test]
fn parse_notes_truncates_oversize_title_and_summary() {
    let long_title = "t".repeat(500);
    let long_summary = "s".repeat(20_000);
    let body = serde_json::json!({
        "title": long_title,
        "summary": long_summary,
        "key_points": [],
        "decisions": [],
        "action_items": [],
    })
    .to_string();
    let notes = parse_notes(&body, &[]).expect("oversize body still parses");
    assert_eq!(notes.title.chars().count(), 120);
    assert_eq!(notes.summary.chars().count(), 8000);
}

#[test]
fn parse_notes_caps_list_length() {
    let key_points: Vec<String> = (0..80).map(|i| format!("point {i}")).collect();
    let body = serde_json::json!({
        "title": "t",
        "summary": "s",
        "key_points": key_points,
        "decisions": [],
        "action_items": [],
    })
    .to_string();
    let notes = parse_notes(&body, &[]).expect("body with many points still parses");
    assert_eq!(notes.key_points.len(), 50);
    assert_eq!(notes.key_points[0], "point 0");
}

#[test]
fn parse_notes_truncates_multibyte_text_without_panicking() {
    // Multibyte scalar values must not be split mid-character.
    let long_summary = "\u{00e9}".repeat(9000); // "é", 2 bytes each in UTF-8
    let body = serde_json::json!({
        "title": "t",
        "summary": long_summary,
        "key_points": [],
        "decisions": [],
        "action_items": [],
    })
    .to_string();
    let notes = parse_notes(&body, &[]).expect("multibyte body still parses");
    assert_eq!(notes.summary.chars().count(), 8000);
    assert!(notes.summary.chars().all(|c| c == '\u{00e9}'));
}

#[test]
fn parse_notes_truncates_multibyte_title_embedded_in_prose() {
    let long_title = "\u{1f600}".repeat(200); // 4-byte emoji scalar values
    let body = format!(
        "Here you go: {}",
        serde_json::json!({
            "title": long_title,
            "summary": "s",
            "key_points": [],
            "decisions": [],
            "action_items": [],
        })
    );
    let notes = parse_notes(&body, &[]).expect("multibyte title in prose still parses");
    assert_eq!(notes.title.chars().count(), 120);
}

// --- to_json / from_json round trip ---

#[test]
fn to_json_from_json_round_trips() {
    let notes = MeetingNotes {
        title: "Weekly sync".to_string(),
        summary: "Reviewed status.".to_string(),
        key_points: vec!["Point A".to_string()],
        decisions: vec!["Decision A".to_string()],
        action_items: vec![ActionItem {
            text: "Follow up".to_string(),
            owner: Some("Dana".to_string()),
            done: true,
        }],
        speaker_refs: SpeakerRefs::Tokens,
    };
    let json = notes.to_json();
    let restored = MeetingNotes::from_json(&json).expect("round trip parses");
    assert_eq!(restored, notes);
}

#[test]
fn from_json_rejects_invalid_json() {
    assert!(MeetingNotes::from_json("not json").is_err());
}

// --- Debug hides content ---

#[test]
fn meeting_notes_debug_hides_content_shows_counts() {
    let notes = MeetingNotes {
        title: "SECRET title".to_string(),
        summary: "SECRET summary text".to_string(),
        key_points: vec!["SECRET point".to_string()],
        decisions: vec![],
        action_items: vec![ActionItem {
            text: "SECRET action".to_string(),
            owner: Some("SECRET owner".to_string()),
            done: false,
        }],
        speaker_refs: SpeakerRefs::Tokens,
    };
    let debug = format!("{notes:?}");
    assert!(!debug.contains("SECRET"), "content leaked: {debug}");
    assert!(debug.contains("key_points"));
}

#[test]
fn summary_config_debug_never_leaks_key() {
    let debug = format!("{:?}", config());
    assert!(!debug.contains("SENTINEL"), "api_key leaked: {debug}");
    assert!(debug.contains("<redacted>"));
}

// --- speaker references (spec §2.7) ---

fn labels() -> Vec<(String, String)> {
    [("Me", "me"), ("Speaker 2", "them:1")]
        .map(|(l, k)| (l.to_string(), k.to_string()))
        .to_vec()
}

fn names() -> Vec<(String, String, String)> {
    [
        ("me", "Me", "Chaz"),
        ("them:1", "Speaker 2", "Sam"),
        ("them:11", "Speaker 12", "Lee"),
    ]
    .map(|(k, d, n)| (k.to_string(), d.to_string(), n.to_string()))
    .to_vec()
}

#[test]
fn the_speaker_clause_is_appended_to_every_template_before_the_schema() {
    for template in [
        None,
        Some("A custom template that never mentions speakers."),
    ] {
        let v = build_summary_request(&config(), "[00:01] {Me}: hi", template);
        let system = v["messages"][0]["content"].as_str().unwrap();
        let clause = system.find("{Speaker 2}").expect("speaker clause missing");
        assert!(clause < system.find("key_points").unwrap(), "{template:?}");
    }
}

#[test]
fn braced_labels_parse_into_key_tokens_in_every_field() {
    let body = r#"{"title":"{Speaker 2} and {Me} sync","summary":"{Speaker 2} spoke.",
        "key_points":["{Me} agreed"],"decisions":["Ask {Finance}"],
        "action_items":[{"text":"Send the deck","owner":"{Speaker 2}","done":false}]}"#;
    let notes = parse_notes(body, &labels()).unwrap();
    assert_eq!(notes.speaker_refs, SpeakerRefs::Tokens);
    assert_eq!(notes.title, "{{them:1}} and {{me}} sync");
    assert_eq!(notes.summary, "{{them:1}} spoke.");
    assert_eq!(notes.key_points, ["{{me}} agreed"]);
    assert_eq!(
        notes.decisions,
        ["Ask Finance"],
        "an unknown brace is stripped"
    );
    assert_eq!(notes.action_items[0].owner.as_deref(), Some("{{them:1}}"));
}

#[test]
fn token_notes_render_names_owners_included_with_no_call() {
    let body = r#"{"title":"t","summary":"{Speaker 2} owes {Me}; Speaker 2 said so.",
        "action_items":[{"text":"Reply to {Me}","owner":"{Speaker 2}"}]}"#;
    let stored = parse_notes(body, &labels()).unwrap();
    let shown = stored.render(&names());
    // Default labels written without braces are replaced too, as a safety net.
    assert_eq!(shown.summary, "Sam owes Chaz; Sam said so.");
    assert_eq!(shown.action_items[0].text, "Reply to Chaz");
    assert_eq!(shown.action_items[0].owner.as_deref(), Some("Sam"));
    // The copy can never pass for token notes, and the stored notes keep
    // their tokens.
    assert_eq!(shown.speaker_refs, SpeakerRefs::Frozen);
    assert!(stored.summary.contains("{{them:1}}"));
}

#[test]
fn notes_stored_before_tokens_render_by_whole_default_labels() {
    let legacy = r#"{"title":"t","summary":"Speaker 2 met Speaker 12 and Me.","key_points":[],
        "decisions":[],"action_items":[{"text":"x","owner":"Speaker 2","done":true}]}"#;
    let stored = MeetingNotes::from_json(legacy).unwrap();
    assert_eq!(stored.speaker_refs, SpeakerRefs::Labels);
    let shown = stored.render(&names());
    assert_eq!(shown.summary, "Sam met Lee and Chaz.");
    assert_eq!(shown.action_items[0].owner.as_deref(), Some("Sam"));
    assert!(shown.action_items[0].done);
}

#[test]
fn frozen_and_unknown_refs_render_exactly_as_stored() {
    for refs in ["frozen", "from-a-newer-hark"] {
        let json = format!(
            r#"{{"title":"t","summary":"Speaker 2 and {{{{me}}}}","key_points":[],"decisions":[],"action_items":[],"speaker_refs":"{refs}"}}"#
        );
        let stored = MeetingNotes::from_json(&json).unwrap();
        assert_eq!(stored.speaker_refs, SpeakerRefs::Frozen);
        assert_eq!(stored.render(&names()).summary, "Speaker 2 and {{me}}");
    }
}

#[test]
fn baking_writes_the_names_in_for_good() {
    let stored = parse_notes(r#"{"title":"t","summary":"{Me} led."}"#, &labels()).unwrap();
    let baked = stored.bake(&names());
    assert_eq!(baked.summary, "Chaz led.");
    let reread = MeetingNotes::from_json(&baked.to_json()).unwrap();
    assert_eq!(reread.render(&[]).summary, "Chaz led.");
}

fn item(text: &str, done: bool) -> ActionItem {
    ActionItem {
        text: text.to_string(),
        owner: None,
        done,
    }
}

#[test]
fn ticks_carry_to_matching_items_and_unmatched_ticked_work_is_kept() {
    let old = MeetingNotes {
        action_items: vec![
            item("Speaker 2 will send the deck to finance", true),
            item("Book the venue", false),
            item("Email the vendor about pricing", true),
        ],
        ..Default::default()
    };
    let mut fresh = MeetingNotes {
        action_items: vec![
            item("{{them:1}} will send the deck to finance!", false),
            item("Book the venue", false),
            item("Draft the agenda", false),
        ],
        ..Default::default()
    };
    fresh.carry_done_from(&old, &["Speaker 2"]);
    let ticks: Vec<bool> = fresh.action_items.iter().map(|a| a.done).collect();
    assert_eq!(ticks, [true, false, false, true]);
    assert_eq!(fresh.action_items[3].text, "Email the vendor about pricing");
}

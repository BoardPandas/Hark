//! Pure-logic tests for voices, prompt assembly, and the word gate.

use hark_voice::{present_terms, skips_cleanup, system_prompt, Voice, RETURN_ONLY_CLAUSE};
use std::str::FromStr;

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|t| t.to_string()).collect()
}

// --- Voice parsing (config and CLI share it) ---

#[test]
fn every_listed_name_parses_and_round_trips() {
    for name in Voice::NAMES {
        let voice = Voice::from_str(name).expect("listed name parses");
        assert_eq!(voice.name(), name);
    }
}

#[test]
fn parsing_is_case_insensitive_and_trims() {
    assert_eq!(Voice::from_str("Clean").unwrap(), Voice::Clean);
    assert_eq!(Voice::from_str("  VERBATIM  ").unwrap(), Voice::Verbatim);
}

#[test]
fn unknown_voice_error_lists_the_valid_names() {
    let err = Voice::from_str("shakespearean").expect_err("must be rejected");
    let msg = err.to_string();
    assert!(msg.contains("shakespearean"));
    for name in Voice::NAMES {
        assert!(msg.contains(name), "error must list {name}: {msg}");
    }
}

// --- system prompt templates ---

#[test]
fn each_voice_template_carries_its_instruction_and_the_closing_clause() {
    let cases = [
        (Voice::Clean, "filler words"),
        (Voice::Professional, "professional"),
        (Voice::Casual, "casual"),
    ];
    for (voice, marker) in cases {
        let prompt = system_prompt(voice, "", &[]).expect("non-verbatim builds a prompt");
        assert!(
            prompt.contains(marker),
            "{} prompt must mention {marker:?}: {prompt}",
            voice.name()
        );
        assert!(
            prompt.ends_with(RETURN_ONLY_CLAUSE),
            "{} prompt must end with the return-only clause",
            voice.name()
        );
        assert!(
            prompt.contains("add or remove content"),
            "{} prompt must forbid content changes",
            voice.name()
        );
    }
}

#[test]
fn custom_voice_uses_the_user_prompt_verbatim() {
    let prompt =
        system_prompt(Voice::Custom, "Rewrite as a pirate.", &[]).expect("custom builds a prompt");
    assert!(prompt.starts_with("Rewrite as a pirate."));
    assert!(prompt.ends_with(RETURN_ONLY_CLAUSE));
}

#[test]
fn verbatim_never_builds_a_prompt() {
    assert_eq!(system_prompt(Voice::Verbatim, "", &["Hark"]), None);
}

#[test]
fn protected_terms_clause_appears_only_when_terms_are_present() {
    let with = system_prompt(Voice::Clean, "", &["Hark", "Modero Cloud"]).unwrap();
    assert!(with.contains("Leave these terms exactly as written: Hark, Modero Cloud."));

    let without = system_prompt(Voice::Clean, "", &[]).unwrap();
    assert!(!without.contains("Leave these terms"));
}

// --- present-in-text subsetting ---

#[test]
fn terms_absent_from_the_text_stay_out() {
    let terms = s(&["Hark", "Levenshtein", "Modero"]);
    let present = present_terms("the hark pipeline is fast", &terms);
    assert_eq!(present, vec!["Hark"]);
}

#[test]
fn containment_is_case_insensitive_both_ways() {
    let terms = s(&["Müller", "nova-3"]);
    assert_eq!(
        present_terms("talk to MÜLLER about Nova-3 today", &terms),
        vec!["Müller", "nova-3"]
    );
}

#[test]
fn multi_word_terms_match_as_a_unit() {
    let terms = s(&["Modero Cloud"]);
    assert_eq!(
        present_terms("deploy it on modero cloud tonight", &terms),
        vec!["Modero Cloud"]
    );
    assert!(present_terms("modero is separate from cloud", &terms).is_empty());
}

#[test]
fn blank_terms_are_ignored() {
    let terms = s(&["", "  ", "Hark"]);
    assert_eq!(present_terms("hark hark", &terms), vec!["Hark"]);
}

// --- token budget: same drop rule as prompt_from_bias_terms ---

#[test]
fn budget_crossing_term_is_dropped_with_everything_after_it() {
    // 400 tokens * 4 chars = 1600-char budget. First term fits; the second
    // would cross; a later short term is dropped too (order is priority).
    let big_a = "a".repeat(1598);
    let big_b = "b".repeat(200);
    let terms = [big_a.as_str(), big_b.as_str(), "short"];
    let prompt = system_prompt(Voice::Clean, "", &terms).unwrap();
    assert!(prompt.contains(&big_a));
    assert!(!prompt.contains(&big_b));
    assert!(!prompt.contains("short,"));
    assert!(!prompt.contains(", short"));
}

#[test]
fn clause_is_omitted_when_even_the_first_term_exceeds_the_budget() {
    let huge = "x".repeat(1601);
    let terms = [huge.as_str()];
    let prompt = system_prompt(Voice::Clean, "", &terms).unwrap();
    assert!(!prompt.contains("Leave these terms"));
}

// --- word-count gate ---

#[test]
fn gate_skips_below_threshold_only() {
    // "fewer than" semantics: 4 words < 5 skips; exactly 5 does not.
    assert!(skips_cleanup("um okay send it", 5));
    assert!(!skips_cleanup("um okay send it now", 5));
    assert!(!skips_cleanup("one two three four five six", 5));
}

#[test]
fn gate_zero_disables_even_for_empty_text() {
    assert!(!skips_cleanup("", 0));
    assert!(!skips_cleanup("hi", 0));
}

#[test]
fn empty_text_skips_under_any_active_gate() {
    assert!(skips_cleanup("", 1));
    assert!(skips_cleanup("   ", 1));
}

#[test]
fn unicode_words_and_odd_whitespace_count_sanely() {
    // Five words split on Unicode whitespace, accents intact.
    assert!(!skips_cleanup("héllo wörld naïve tëst ök", 5));
    assert!(skips_cleanup("héllo wörld naïve tëst", 5));
    // Runs of mixed whitespace do not inflate the count.
    assert!(skips_cleanup("one \t two\n three", 4));
}

//! Pure-logic tests for voices, prompt assembly, and the word gate.

use hark_voice::{
    over_expanded, present_terms, skips_cleanup, system_prompt, Voice, LENGTH_DISCIPLINE_CLAUSE,
    RETURN_ONLY_CLAUSE,
};
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
            prompt.contains("Never add sentences"),
            "{} prompt must forbid added content",
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

// --- length discipline: prompt clause ---

#[test]
fn every_built_in_voice_carries_the_length_clause() {
    // Professional and Casual are the voices that drifted into writing prose;
    // the clause is what tells the model they are still edits.
    for voice in [Voice::Clean, Voice::Professional, Voice::Casual] {
        let prompt = system_prompt(voice, "", &[]).unwrap();
        assert!(
            prompt.contains(LENGTH_DISCIPLINE_CLAUSE),
            "{} is missing the length clause",
            voice.name()
        );
    }
}

#[test]
fn custom_voice_is_not_given_the_length_clause() {
    // The user's own prompt is used verbatim; "expand this into an email" is
    // a legitimate custom voice.
    let prompt = system_prompt(Voice::Custom, "Turn this into a formal email.", &[]).unwrap();
    assert!(!prompt.contains(LENGTH_DISCIPLINE_CLAUSE));
    assert!(prompt.starts_with("Turn this into a formal email."));
    assert!(prompt.ends_with(RETURN_ONLY_CLAUSE));
}

#[test]
fn length_clause_precedes_the_protected_terms_clause() {
    // Ordering matters only for readability, but a swap would bury the terms
    // clause mid-prompt; pin the assembled shape.
    let prompt = system_prompt(Voice::Clean, "", &["Modero"]).unwrap();
    let clause = prompt.find(LENGTH_DISCIPLINE_CLAUSE).unwrap();
    let terms = prompt.find("Leave these terms").unwrap();
    assert!(clause < terms);
    assert!(terms < prompt.find(RETURN_ONLY_CLAUSE).unwrap());
}

// --- length discipline: the programmatic guard ---

#[test]
fn a_short_remark_blown_into_a_paragraph_is_rejected() {
    // The reported failure: five words in, a paragraph out.
    let input = "we should ship it friday";
    let output = "I wanted to follow up regarding our release timeline. After giving it some \
                  thought, I believe we should aim to ship this coming Friday. Please let me \
                  know if that works for you.";
    assert!(over_expanded(input, output, 1.4));
}

#[test]
fn a_legitimate_tidy_of_a_short_remark_survives() {
    // 4 -> 5 words. Pure ratio would allow only 5.6 and this squeaks by, but
    // the grace allowance is what makes short utterances reliably safe.
    assert!(!over_expanded(
        "yeah sounds good to me",
        "Yes, that sounds good to me.",
        1.4
    ));
    // Filler removal shrinks text; shrinking is never a rejection.
    assert!(!over_expanded(
        "um so I think we should uh ship it",
        "I think we should ship it.",
        1.4
    ));
}

#[test]
fn grace_allowance_governs_short_input_and_ratio_governs_long() {
    // 5 words: allowance is max(7, 8) = 8, so 8 passes and 9 fails.
    let five = "one two three four five";
    assert!(!over_expanded(five, "a b c d e f g h", 1.4));
    assert!(over_expanded(five, "a b c d e f g h i", 1.4));

    // 20 words: ratio (28) now exceeds the grace floor (23) and governs.
    let twenty = "one two three four five six seven eight nine ten eleven twelve thirteen \
                  fourteen fifteen sixteen seventeen eighteen nineteen twenty";
    let words = |n: usize| vec!["w"; n].join(" ");
    assert!(!over_expanded(twenty, &words(28), 1.4));
    assert!(over_expanded(twenty, &words(29), 1.4));
}

#[test]
fn ratio_of_zero_or_nonfinite_disables_the_guard() {
    let input = "five little words right here";
    let paragraph = vec!["w"; 200].join(" ");
    assert!(!over_expanded(input, &paragraph, 0.0));
    // A hand-edited TOML `nan` must not silently reject every cleanup; config
    // validation rejects it first, this is the belt-and-braces path.
    assert!(!over_expanded(input, &paragraph, f32::NAN));
    assert!(!over_expanded(input, &paragraph, f32::INFINITY));
}

#[test]
fn a_tighter_ratio_is_honored_once_past_the_grace_floor() {
    let ten = "one two three four five six seven eight nine ten";
    let words = |n: usize| vec!["w"; n].join(" ");
    // 1.1x of 10 is 11, but grace still allows 13.
    assert!(!over_expanded(ten, &words(13), 1.1));
    assert!(over_expanded(ten, &words(14), 1.1));
}

#[test]
fn empty_input_does_not_panic_or_reject_a_short_output() {
    // Guard runs before any emptiness check upstream; must be total.
    assert!(!over_expanded("", "", 1.4));
    assert!(!over_expanded("", "Hi.", 1.4));
    assert!(over_expanded("", &["w"; 10].join(" "), 1.4));
}

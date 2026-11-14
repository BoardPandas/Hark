use super::engines::transcribe_with_retry;
use super::shape::{
    cleaned_text, corrected_text, expanded_text, should_run_cleanup, strip_single_word_period,
};
use super::*;
use hark_stt::{SttError, Transcript};
use std::cell::Cell;

/// Scripted provider: pops one result per call, counts calls.
struct MockProvider {
    calls: Cell<u32>,
    script: std::cell::RefCell<Vec<Result<Transcript, SttError>>>,
}

impl MockProvider {
    fn new(script: Vec<Result<Transcript, SttError>>) -> Self {
        MockProvider {
            calls: Cell::new(0),
            script: std::cell::RefCell::new(script),
        }
    }
    fn ok(text: &str) -> Result<Transcript, SttError> {
        Ok(Transcript {
            text: text.to_string(),
            cleaned: None,
            request_ms: 1,
        })
    }
    fn timeout() -> Result<Transcript, SttError> {
        Err(SttError::Timeout {
            provider: "mock".to_string(),
            configured_ms: 15_000,
        })
    }
    fn auth() -> Result<Transcript, SttError> {
        Err(SttError::Auth {
            provider: "mock".to_string(),
            status: 401,
            detail: String::new(),
        })
    }
}

impl SttProvider for MockProvider {
    fn transcribe(&self, _wav: &[u8]) -> Result<Transcript, SttError> {
        self.calls.set(self.calls.get() + 1);
        self.script.borrow_mut().remove(0)
    }
    fn label(&self) -> &str {
        "mock"
    }
}

#[test]
fn success_needs_one_call() {
    let p = MockProvider::new(vec![MockProvider::ok("hello")]);
    let t = transcribe_with_retry(&p, b"wav", true, &Default::default()).unwrap();
    assert_eq!(t.text, "hello");
    assert_eq!(p.calls.get(), 1);
}

#[test]
fn timeout_retries_exactly_once_then_succeeds() {
    let p = MockProvider::new(vec![
        MockProvider::timeout(),
        MockProvider::ok("second try"),
    ]);
    let t = transcribe_with_retry(&p, b"wav", true, &Default::default()).unwrap();
    assert_eq!(t.text, "second try");
    assert_eq!(p.calls.get(), 2);
}

// Transcript (frozen hark-stt) has no Debug impl, so unwrap_err is
// unavailable; unwrap the error by hand.
fn expect_err(result: Result<Transcript, SttError>) -> SttError {
    match result {
        Err(e) => e,
        Ok(t) => panic!(
            "expected an error, got a transcript of {} chars",
            t.text.len()
        ),
    }
}

#[test]
fn double_timeout_fails_after_two_calls_total() {
    let p = MockProvider::new(vec![MockProvider::timeout(), MockProvider::timeout()]);
    let err = expect_err(transcribe_with_retry(&p, b"wav", true, &Default::default()));
    assert!(matches!(err, SttError::Timeout { .. }));
    assert_eq!(p.calls.get(), 2, "never a second retry");
}

#[test]
fn a_replay_after_a_failed_stream_is_the_retry() {
    // The stream already made the first attempt. Retrying its replay as
    // well was three attempts; against a stranded Gemini turn, 26 s.
    let p = MockProvider::new(vec![MockProvider::timeout()]);
    let err = expect_err(transcribe_with_retry(
        &p,
        b"wav",
        false,
        &Default::default(),
    ));
    assert!(matches!(err, SttError::Timeout { .. }));
    assert_eq!(p.calls.get(), 1);
}

#[test]
fn auth_fails_without_any_retry() {
    let p = MockProvider::new(vec![MockProvider::auth()]);
    let err = expect_err(transcribe_with_retry(&p, b"wav", true, &Default::default()));
    assert!(matches!(err, SttError::Auth { .. }));
    assert_eq!(p.calls.get(), 1, "4xx must never retry");
}

// The worker path from a provider transcript to the text handed to
// hark_inject::inject: transcribe (with retry) then the spellbook
// pass. Injection itself is I/O, validated on real hardware at CP6.

#[test]
fn misspelled_transcript_is_corrected_before_injection() {
    let p = MockProvider::new(vec![MockProvider::ok(
        "tell vosburg the madero build is green",
    )]);
    let corrector = Corrector::from_terms(&["Vossburg".to_string(), "Modero".to_string()]);

    let transcript = transcribe_with_retry(&p, b"wav", true, &Default::default()).unwrap();
    let (text, replacements) = corrected_text(&corrector, &transcript.text);
    assert_eq!(text, "tell Vossburg the Modero build is green");
    assert_eq!(replacements, 2);
}

#[test]
fn empty_spellbook_leaves_the_transcript_untouched() {
    let corrector = Corrector::new(&[]);
    assert_eq!(
        corrected_text(&corrector, "as it was"),
        ("as it was".into(), 0)
    );
}

// --- single-word period stripping (pure seam) ---

#[test]
fn a_single_word_loses_its_trailing_period() {
    assert_eq!(strip_single_word_period("Hello."), "Hello");
    assert_eq!(strip_single_word_period("yes"), "yes");
    assert_eq!(strip_single_word_period("42."), "42");
    // Trimmed only on the strip path; a lone word carries no real spacing.
    assert_eq!(strip_single_word_period("  Okay.  "), "Okay");
}

#[test]
fn multi_word_and_other_endings_are_left_alone() {
    // More than one word is a sentence; its period stays.
    assert_eq!(strip_single_word_period("we shipped it."), "we shipped it.");
    // Only '.' is touched: a lone word may actually have meant the query.
    assert_eq!(strip_single_word_period("What?"), "What?");
    assert_eq!(strip_single_word_period("Stop!"), "Stop!");
    // No trailing period, nothing to do (and a mid-word dot is not one).
    assert_eq!(strip_single_word_period("3.14"), "3.14");
    // A word must remain: a bare period is returned as-is, never emptied.
    assert_eq!(strip_single_word_period("."), ".");
}

// --- the cleanup pass (scripted MockCleaner, pattern: MockProvider) ---

use hark_voice::{Cleaned, CleanupError, CleanupProvider};

/// Scripted cleaner: pops one result per call, counts calls.
struct MockCleaner {
    calls: Cell<u32>,
    script: std::cell::RefCell<Vec<Result<Cleaned, CleanupError>>>,
}

impl MockCleaner {
    fn ok(text: &str) -> Result<Cleaned, CleanupError> {
        Ok(Cleaned {
            text: text.to_string(),
            request_ms: 1,
        })
    }
    fn provider_err() -> Result<Cleaned, CleanupError> {
        Err(CleanupError::Provider {
            provider: "mock".to_string(),
            detail: "scripted failure".to_string(),
        })
    }
    fn plan(script: Vec<Result<Cleaned, CleanupError>>, skip_below_words: u32) -> CleanupPlan {
        CleanupPlan {
            cleaner: Box::new(MockCleaner {
                calls: Cell::new(0),
                script: std::cell::RefCell::new(script),
            }),
            voice: hark_voice::Voice::Clean,
            model: "mock-model".to_string(),
            skip_below_words,
            max_expansion_ratio: 1.4,
            prewarm_url: None,
        }
    }
}

impl CleanupProvider for MockCleaner {
    fn clean(&self, _text: &str) -> Result<Cleaned, CleanupError> {
        self.calls.set(self.calls.get() + 1);
        self.script.borrow_mut().remove(0)
    }
    fn label(&self) -> &str {
        "mock"
    }
}

#[test]
fn an_ordinary_dictation_runs_cleanup() {
    assert!(should_run_cleanup(false, false));
}

#[test]
fn a_fired_invocation_never_reaches_a_cleanup_model() {
    assert!(!should_run_cleanup(true, false));
}

#[test]
fn a_fused_provider_result_is_not_cleaned_twice() {
    // Gemini Live in SMART mode already rewrote the utterance; a second
    // voice pass would rewrite the rewrite and bill a second model.
    assert!(!should_run_cleanup(false, true));
    assert!(!should_run_cleanup(true, true));
}

#[test]
fn cleaned_text_is_injected_after_spellbook_pass_2() {
    let corrector = Corrector::new(&[]);
    let plan = MockCleaner::plan(vec![MockCleaner::ok("Cleaned and polished.")], 0);
    let out = cleaned_text(
        Some(&plan),
        &corrector,
        "so um cleaned and uh polished".to_string(),
    );
    assert_eq!(out.text, "Cleaned and polished.");
    // A successful cleanup reports its wall time for the record.
    assert!(out.request_ms.is_some());
    assert_eq!(out.spellbook_replacements, 0);
}

#[test]
fn insights_count_actual_corrections_in_both_passes() {
    let corrector = Corrector::from_terms(&["Vossburg".into(), "Modero".into()]);
    let (text, first) = corrected_text(&corrector, "tell vosburg the madero build is green");
    let plan = MockCleaner::plan(
        vec![MockCleaner::ok("Tell Vossburg the madero build is green.")],
        0,
    );
    let cleaned = cleaned_text(Some(&plan), &corrector, text);
    assert_eq!(first, 2);
    assert_eq!(cleaned.spellbook_replacements, 1);
    assert_eq!(cleaned.text, "Tell Vossburg the Modero build is green.");
    assert_eq!(first + cleaned.spellbook_replacements, 3);
}

#[test]
fn an_over_expanded_cleanup_is_discarded_for_the_pass_1_text() {
    let corrector = Corrector::new(&[]);
    // Five words in, a paragraph out: the reported Professional/Clean
    // failure. The call still happened, so the cost was paid, but the
    // user gets what they said rather than what the model wrote.
    let plan = MockCleaner::plan(
        vec![MockCleaner::ok(
            "I wanted to follow up regarding our release timeline. After giving it some \
             thought, I believe we should aim to ship this coming Friday. Please let me \
             know if that works for you.",
        )],
        0,
    );
    let out = cleaned_text(
        Some(&plan),
        &corrector,
        "we should ship it friday".to_string(),
    );
    assert_eq!(out.text, "we should ship it friday");
    // Rejected cleanup must not claim a model shaped the record.
    assert_eq!(out.request_ms, None);
}

#[test]
fn a_cleanup_that_answers_the_dictation_is_discarded_for_the_pass_1_text() {
    let corrector = Corrector::new(&[]);
    // The reported failure, word for word: same length, so the expansion
    // guard passed it, and it was injected into the user's chat box.
    let plan = MockCleaner::plan(
        vec![MockCleaner::ok(
            "Please provide the transcript you would like me to rewrite.",
        )],
        0,
    );
    let said = "Proceed however you recommend to make this as best as possible.";
    let out = cleaned_text(Some(&plan), &corrector, said.to_string());
    assert_eq!(out.text, said);
    assert_eq!(out.request_ms, None);
}

#[test]
fn a_same_length_cleanup_is_kept() {
    let corrector = Corrector::new(&[]);
    let plan = MockCleaner::plan(vec![MockCleaner::ok("We should ship it Friday.")], 0);
    let out = cleaned_text(
        Some(&plan),
        &corrector,
        "so um we should ship it friday".to_string(),
    );
    assert_eq!(out.text, "We should ship it Friday.");
    assert!(out.request_ms.is_some());
}

#[test]
fn custom_voice_may_expand_freely() {
    let corrector = Corrector::new(&[]);
    let long = "Dear team, I wanted to write and let you know that we should aim to ship \
                this coming Friday. Please let me know if that works. Best regards.";
    let mut plan = MockCleaner::plan(vec![MockCleaner::ok(long)], 0);
    plan.voice = hark_voice::Voice::Custom;
    let out = cleaned_text(
        Some(&plan),
        &corrector,
        "we should ship it friday".to_string(),
    );
    // The user's own prompt asked for this; the guard must not fight it.
    assert_eq!(out.text, long);
    assert!(out.request_ms.is_some());
}

#[test]
fn ratio_of_zero_disables_the_expansion_guard() {
    let corrector = Corrector::new(&[]);
    // Long, but still the speaker's words, so only the ratio is on trial.
    let long = vec!["we should ship it friday"; 40].join(" ");
    let mut plan = MockCleaner::plan(vec![MockCleaner::ok(&long)], 0);
    plan.max_expansion_ratio = 0.0;
    let out = cleaned_text(
        Some(&plan),
        &corrector,
        "we should ship it friday".to_string(),
    );
    assert_eq!(out.text, long);
}

#[test]
fn cleanup_error_injects_the_pass_1_text() {
    let corrector = Corrector::new(&[]);
    let plan = MockCleaner::plan(vec![MockCleaner::provider_err()], 0);
    let out = cleaned_text(
        Some(&plan),
        &corrector,
        "the original transcript stands".to_string(),
    );
    assert_eq!(out.text, "the original transcript stands");
    // Degraded: the record must not claim a cleanup model shaped this.
    assert_eq!(out.request_ms, None);
    assert_eq!(out.spellbook_replacements, 0);
}

#[test]
fn no_plan_means_verbatim_and_no_calls() {
    let corrector = Corrector::new(&[]);
    let out = cleaned_text(None, &corrector, "exactly as spoken".to_string());
    assert_eq!(out.text, "exactly as spoken");
    assert_eq!(out.request_ms, None);
}

#[test]
fn short_utterance_never_calls_the_cleaner() {
    let corrector = Corrector::new(&[]);
    // An empty script would panic on any call; not panicking IS the
    // assertion that the gate short-circuited before the cleaner.
    let plan = MockCleaner::plan(vec![], 5);
    let out = cleaned_text(Some(&plan), &corrector, "um send it".to_string());
    assert_eq!(out.text, "um send it");
    assert_eq!(out.request_ms, None);
}

#[test]
fn gate_threshold_zero_always_calls() {
    let corrector = Corrector::new(&[]);
    let plan = MockCleaner::plan(vec![MockCleaner::ok("Hi.")], 0);
    // One word, but the gate is disabled: the (scripted) cleaner runs.
    assert_eq!(
        cleaned_text(Some(&plan), &corrector, "hi".to_string()).text,
        "Hi."
    );
}

// --- invocations: the expansion must never reach the cleanup model ---

fn expander(phrase: &str, expansion: &str, scope: hark_spellbook::Scope) -> Expander {
    Expander::new(&[(phrase.to_string(), expansion.to_string(), scope)])
}

const CANNED: &str = "You have access to the Support Forge tools: ticketing, remote \
                      assist, and the asset inventory. Raise a ticket for anything \
                      that needs an escalation, and the on-call engineer will pick \
                      it up within the hour.";

/// The load-bearing test for the whole feature.
///
/// The cleaner's script is **empty**, so any call to it panics: not
/// panicking IS the assertion. It proves the `Option::filter` in
/// `dictate` short-circuits before the cleanup call, which is what
/// keeps `over_expanded` from silently discarding the expansion (a
/// 2-word utterance gets an allowance of 5 words, so a 40-word
/// expansion would lose every time).
///
/// The corrector is hostile on purpose: it would rewrite "Forge" inside
/// the canned text if spellbook pass 2 ever ran over it. It must not.
#[test]
fn a_fired_invocation_never_calls_the_cleaner() {
    let corrector = Corrector::from_terms(&["Forj".to_string()]);
    let expander = expander("access granted", CANNED, hark_spellbook::Scope::Utterance);
    // Shaped like `Worker::cleanup` so the branch below is the real one.
    let cleanup = Some(MockCleaner::plan(vec![], 0));

    let expanded = expanded_text(&expander, "access granted".to_string());
    assert_eq!(expanded.fired.as_deref(), Some("access granted"));

    // Exactly the branch `dictate` runs.
    let plan = cleanup.as_ref().filter(|_| expanded.fired.is_none());
    assert!(plan.is_none(), "a fired invocation must null the plan");
    let out = cleaned_text(plan, &corrector, expanded.text);

    assert_eq!(
        out.text, CANNED,
        "canned text is injected byte for byte: no cleanup, no pass 2"
    );
    assert_eq!(
        out.request_ms, None,
        "no cleanup ran, so the record must not claim one did"
    );
}

#[test]
fn no_invocation_leaves_the_cleanup_path_identical() {
    let corrector = Corrector::new(&[]);
    let expander = expander("access granted", CANNED, hark_spellbook::Scope::Utterance);
    let cleanup = Some(MockCleaner::plan(
        vec![MockCleaner::ok("Cleaned and polished.")],
        0,
    ));

    let expanded = expanded_text(&expander, "so um cleaned and uh polished".to_string());
    assert_eq!(expanded.fired, None);
    assert_eq!(expanded.text, "so um cleaned and uh polished");

    let plan = cleanup.as_ref().filter(|_| expanded.fired.is_none());
    assert!(plan.is_some(), "an ordinary dictation still gets cleanup");
    let out = cleaned_text(plan, &corrector, expanded.text);
    assert_eq!(out.text, "Cleaned and polished.");
    assert!(out.request_ms.is_some());
}

#[test]
fn an_empty_invocation_set_is_a_pure_passthrough() {
    let expander = Expander::new(&[]);
    let out = expanded_text(&expander, "nothing configured".to_string());
    assert_eq!(out.text, "nothing configured");
    assert_eq!(out.fired, None);
}

#[test]
fn terms_mangled_by_the_model_are_repaired_by_pass_2() {
    let corrector = Corrector::from_terms(&["Vossburg".to_string(), "Modero".to_string()]);
    // The mock "model" re-mangles both spellbook terms.
    let plan = MockCleaner::plan(
        vec![MockCleaner::ok("Tell vosburg the madero build is green.")],
        0,
    );
    let out = cleaned_text(
        Some(&plan),
        &corrector,
        "tell Vossburg the Modero build is green".to_string(),
    );
    assert_eq!(out.text, "Tell Vossburg the Modero build is green.");
}

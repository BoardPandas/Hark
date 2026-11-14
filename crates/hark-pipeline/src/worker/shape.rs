//! Transcript shaping: the text steps between a transcript and injection.
//! Spellbook pass 1, invocations, the optional voice cleanup (with spellbook
//! pass 2), and the single-word period.

use super::{CleanupPlan, Worker};
use hark_spellbook::{Corrector, Expander, Expansion};
use hark_stt::Transcript;
use hark_voice::{over_expanded, reads_as_reply, skips_cleanup, Voice};
use std::time::Instant;

/// The text steps between a transcript and injection: spellbook pass 1,
/// invocations, optional cleanup, single-word period. Shared by a live
/// dictation and a retry of a held clip so both go through exactly the same
/// path.
pub(super) struct Shaped {
    pub(super) cleaned: CleanupOutcome,
    pub(super) fired: Option<String>,
    pub(super) pass_one_replacements: u64,
    pub(super) provider_cleaned: bool,
}

/// `None` when the run was retired partway, so the caller drops the work.
pub(super) fn shape_transcript(worker: &Worker, transcript: &Transcript) -> Option<Shaped> {
    let (text, pass_one_replacements) = corrected_text(&worker.corrector, &transcript.text);
    let expanded = expanded_text(&worker.expander, text);
    // Canned text is authored, not spoken: it never reaches a cleanup model
    // that would rewrite it, and never pays for the call. `None` here makes
    // `cleaned_text` hit its passthrough on the first line, which is the
    // whole protection -- see `expanded_text` for why it must be control
    // flow and not a prompt clause.
    // A fused provider (Gemini Live in SMART mode) already returned cleaned
    // text. Running a voice on top would rewrite an already-rewritten utterance
    // and bill a second model to do it, so the plan is dropped rather than the
    // call being made and discarded.
    let provider_cleaned = transcript.cleaned.is_some();
    let plan = worker
        .cleanup
        .as_ref()
        .filter(|_| should_run_cleanup(expanded.fired.is_some(), provider_cleaned));
    if worker.control.is_cancelled() {
        return None;
    }
    let mut cleaned = cleaned_text(plan, &worker.corrector, expanded.text);
    // A single spoken word is almost never a sentence, so the trailing period
    // the provider or a cleanup voice adds is noise. Gated on the setting, and
    // never applied to a fired invocation: its canned text is authored
    // verbatim (a one-token expansion like "Ph.D." must keep its dot).
    if worker.strip_single_word_period && expanded.fired.is_none() {
        cleaned.text = strip_single_word_period(&cleaned.text);
    }
    Some(Shaped {
        cleaned,
        fired: expanded.fired,
        pass_one_replacements,
        provider_cleaned,
    })
}

/// Outcome of the cleanup pass. `request_ms` is present only when a cleanup
/// response actually shaped `text`; every degraded path (no plan, gate,
/// provider error) leaves it `None` so the dictation record stays honest.
pub(crate) struct CleanupOutcome {
    pub text: String,
    pub request_ms: Option<u64>,
    pub spellbook_replacements: u64,
}

/// Whether Hark should spend a cleanup call on this dictation.
///
/// Two independent reasons not to, both control flow rather than prompt
/// wording: a fired invocation is authored text that must reach the cursor
/// verbatim, and a fused provider has already done the rewrite in the
/// transcription call.
pub(crate) fn should_run_cleanup(fired_invocation: bool, provider_cleaned: bool) -> bool {
    !fired_invocation && !provider_cleaned
}

/// The optional voice-cleanup pass between spellbook pass 1 and injection.
/// Fail-open at every layer: no plan (Verbatim or degraded), a gated short
/// utterance, or any cleanup error all inject the pass-1 text unchanged; a
/// dictation is never lost to the optional feature. Spellbook pass 2 runs
/// only when cleanup actually rewrote the text, repairing any term the model
/// re-mangled. Logs counts, millis, and config labels only.
pub(super) fn cleaned_text(
    plan: Option<&CleanupPlan>,
    corrector: &Corrector,
    text: String,
) -> CleanupOutcome {
    let passthrough = |text: String| CleanupOutcome {
        text,
        request_ms: None,
        spellbook_replacements: 0,
    };
    let Some(plan) = plan else {
        return passthrough(text);
    };
    if skips_cleanup(&text, plan.skip_below_words) {
        log::info!(
            "cleanup skipped (short utterance: {} words)",
            text.split_whitespace().count()
        );
        return passthrough(text);
    }
    match plan.cleaner.clean(&text) {
        // Custom is the user's own prompt, so an expansion there was asked
        // for; every built-in voice is an edit and must not grow the text.
        Ok(cleaned)
            if plan.voice != Voice::Custom
                && over_expanded(&text, &cleaned.text, plan.max_expansion_ratio) =>
        {
            log::warn!(
                "cleanup rejected: voice={} model={} expanded {} -> {} words (limit {}x); \
                 injecting uncleaned transcript",
                plan.voice.name(),
                plan.model,
                text.split_whitespace().count(),
                cleaned.text.split_whitespace().count(),
                plan.max_expansion_ratio
            );
            passthrough(text)
        }
        // Dictation that sounds like a request ("proceed however you
        // recommend") can be answered instead of edited. Custom is exempt for
        // the same reason as above: a translate prompt keeps no words at all.
        Ok(cleaned) if plan.voice != Voice::Custom && reads_as_reply(&text, &cleaned.text) => {
            log::warn!(
                "cleanup rejected: voice={} model={} replied to the transcript instead of \
                 editing it; injecting uncleaned transcript",
                plan.voice.name(),
                plan.model
            );
            passthrough(text)
        }
        Ok(cleaned) => {
            log::info!(
                "cleanup: voice={} model={} {}->{} chars, request {} ms",
                plan.voice.name(),
                plan.model,
                text.chars().count(),
                cleaned.text.chars().count(),
                cleaned.request_ms
            );
            let (text, spellbook_replacements) = corrected_text(corrector, &cleaned.text);
            CleanupOutcome {
                text,
                request_ms: Some(cleaned.request_ms as u64),
                spellbook_replacements,
            }
        }
        Err(e) => {
            log::warn!("cleanup failed ({e}); injecting uncleaned transcript");
            passthrough(text)
        }
    }
}

/// The invocation pass, between spellbook pass 1 and cleanup. Pure (the
/// testable seam); logs whether one fired and the millis, never the trigger
/// phrase or the expansion (both are user content).
///
/// Running after pass 1 is deliberate: pass 1 canonicalizes the *spoken*
/// words, which can only help a trigger match (matching lowercases both
/// sides, so casing changes are harmless). The expansion is inserted
/// afterwards and is therefore never touched by the spellbook.
///
/// A caller that sees `fired.is_some()` **must** skip cleanup. That is not a
/// preference, it is the four failure modes this avoids:
///
/// 1. `hark_voice::over_expanded` allows `max(words * ratio, words + 3)`, so
///    a 3-word utterance expanding to 60 words gets an allowance of 6 and
///    the expansion would be *silently discarded* in favour of the literal
///    words the user said.
/// 2. A short, free, instant dictation would turn into a billed LLM round
///    trip on the release-to-inject path.
/// 3. No HTTP call at all: no latency, no cost, no failure mode.
/// 4. Spellbook pass 2 never runs over the expansion, so phonetic
///    post-correction cannot mangle URLs or product names the user typed
///    into their own canned text.
///
/// It has to be control flow rather than persuasion: every built-in voice
/// already carries `LENGTH_DISCIPLINE_CLAUSE` telling the model not to
/// expand a short remark into a paragraph, so a prompt clause would be
/// fighting the rest of the prompt.
pub(super) fn expanded_text(expander: &Expander, text: String) -> Expansion {
    let started = Instant::now();
    let expansion = expander.expand(&text);
    if expansion.fired.is_some() {
        log::info!(
            "invocation fired: {} -> {} chars in {} ms; cleanup skipped",
            text.split_whitespace().count(),
            expansion.text.chars().count(),
            started.elapsed().as_millis()
        );
    }
    expansion
}

/// The spellbook pass between transcript and injection. Pure (the testable
/// seam); logs counts and millis only, never transcript text or terms.
pub(super) fn corrected_text(corrector: &Corrector, transcript_text: &str) -> (String, u64) {
    let started = Instant::now();
    let (text, replacements) = corrector.correct(transcript_text);
    log::info!(
        "spellbook: {replacements} replacements in {} ms",
        started.elapsed().as_millis()
    );
    (text, replacements as u64)
}

/// Drop a trailing period from a single-word utterance ("Hello." -> "Hello").
/// Pure (the testable seam). Only a single `.` is removed, and only when a
/// word is left after it: multi-word text, other sentence punctuation (`?` and
/// `!` can be exactly what a lone word meant), and a bare "." are all returned
/// unchanged. Surrounding whitespace is trimmed only on the strip path, where
/// a lone word never carries meaningful spacing.
pub(super) fn strip_single_word_period(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.split_whitespace().count() == 1 {
        if let Some(stripped) = trimmed.strip_suffix('.') {
            if !stripped.is_empty() {
                return stripped.to_string();
            }
        }
    }
    text.to_string()
}

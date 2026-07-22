//! Invocations: a user-authored trigger phrase heard in a dictation is
//! replaced by canned text, injected exactly as the user wrote it.
//!
//! Matching reuses [`crate::matcher`]'s guarded phonetic path rather than a
//! second implementation. LL-G `rust/phonetic-code-equality-needs-confirm-guard`
//! is HIGH severity: Double Metaphone code equality alone collides common
//! words with proper nouns, so the >= 4-char / all-alphabetic gate, the
//! digit/short-word exact-only path, and the Jaro-Winkler confirm are all
//! load-bearing. One guarded matcher is easier to keep correct than two.
//!
//! Pure text processing, no I/O, no async: same hot-path contract as
//! [`crate::Corrector`], and the expansion text never reaches a log line.

use crate::matcher::{self, Codes, TermEntry};
use crate::tokenize;

/// Jaro-Winkler confirmation threshold for invocation triggers, deliberately
/// tighter than the dictionary's 0.85.
///
/// A dictionary false positive corrupts one word; an invocation false
/// positive pastes a whole paragraph the user did not ask for. The recall
/// cost is small (a genuine trigger is a phrase the user chose and says on
/// purpose), and the fix for a stubborn one is free: matching runs *after*
/// dictionary pass 1, so adding the word to the Dictionary repairs the
/// transcript before the trigger is ever compared.
const INVOCATION_JW_THRESHOLD: f64 = 0.90;

/// The fewest words a trigger may have. One-word triggers fire constantly
/// against ordinary speech and are the single biggest false-positive source,
/// so they are refused at build time rather than merely discouraged.
pub const MIN_TRIGGER_WORDS: usize = 2;

/// Where in a dictation a trigger is allowed to fire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The trigger must account for the entire dictation.
    Utterance,
    /// The trigger fires anywhere inside a longer dictation.
    Anywhere,
}

/// The outcome of the invocation pass.
pub struct Expansion {
    pub text: String,
    /// The trigger phrase that fired, for the history record and to tell the
    /// pipeline to skip cleanup. `None` means nothing fired.
    pub fired: Option<String>,
}

/// One armed invocation: the matching side plus what to paste.
struct Entry {
    /// `canonical` here is the trigger phrase exactly as the user wrote it.
    term: TermEntry,
    expansion: String,
    scope: Scope,
}

/// Expands user-authored trigger phrases into canned text.
///
/// Construction precomputes per-trigger phonetic data once; `expand` is
/// called per dictation on the hot path.
pub struct Expander {
    dm: rphonetic::DoubleMetaphone,
    entries: Vec<Entry>,
    skipped: usize,
}

impl Expander {
    /// Build from `(phrase, expansion, scope)` triples.
    ///
    /// Fails soft, never loudly: an entry that cannot arm is skipped and
    /// counted, so a hand-edited config still loads and the user keeps a UI
    /// to fix it in. The Invocations page explains each skip per row.
    ///
    /// Skipped: triggers under [`MIN_TRIGGER_WORDS`] words, empty
    /// expansions, and duplicate triggers (compared as normalized token
    /// sequences, first wins). The phrase is the identity — there are no
    /// hidden ids, so the TOML stays hand-editable (LL-G
    /// `sqlite/upsert-by-name-collision`: decide duplicate semantics before
    /// users have data, not after).
    pub fn new(invocations: &[(String, String, Scope)]) -> Expander {
        let dm = rphonetic::DoubleMetaphone::default();
        let mut entries: Vec<Entry> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        let mut skipped = 0;
        for (phrase, expansion, scope) in invocations {
            let key = normalized_phrase(phrase);
            if expansion.is_empty()
                || phrase_word_count(phrase) < MIN_TRIGGER_WORDS
                || seen.contains(&key)
            {
                skipped += 1;
                continue;
            }
            // One term in, at most one entry out; the word-count gate above
            // already guarantees the phrase tokenizes to something.
            let Some(term) = matcher::build_entries(&dm, std::slice::from_ref(phrase)).pop() else {
                skipped += 1;
                continue;
            };
            seen.push(key);
            entries.push(Entry {
                term,
                expansion: expansion.clone(),
                scope: *scope,
            });
        }
        // Longest first so a multi-word trigger wins an overlap with a
        // shorter one, matching Corrector's ordering contract. `sort_by` is
        // stable, so equal-length triggers keep their configured order.
        entries.sort_by_key(|e| std::cmp::Reverse(e.term.word_count()));
        Expander {
            dm,
            entries,
            skipped,
        }
    }

    /// How many configured invocations are armed and can fire.
    pub fn armed(&self) -> usize {
        self.entries.len()
    }

    /// How many configured invocations will never fire. The pipeline logs
    /// this count and nothing else: phrases and expansions are user content.
    pub fn skipped(&self) -> usize {
        self.skipped
    }

    /// Run the invocation pass over one transcript.
    ///
    /// Never fails: no armed invocations, no tokens, or no match all return
    /// the input unchanged with `fired: None`.
    pub fn expand(&self, text: &str) -> Expansion {
        let unchanged = || Expansion {
            text: text.to_string(),
            fired: None,
        };
        if self.entries.is_empty() || text.is_empty() {
            return unchanged();
        }
        let tokens = tokenize::tokenize(text);
        if tokens.is_empty() {
            return unchanged();
        }
        let codes: Vec<Codes> = tokens
            .iter()
            .map(|t| matcher::encode(&self.dm, &t.lower))
            .collect();

        // Pass 1: whole-dictation triggers. The trigger has to account for
        // every spoken word, so a hit *is* the entire result and nothing
        // else can apply. Punctuation and casing around the words sit
        // outside token spans and are therefore irrelevant.
        for entry in &self.entries {
            if entry.scope != Scope::Utterance || entry.term.word_count() != tokens.len() {
                continue;
            }
            if matcher::window_matches(&entry.term, &tokens, &codes, INVOCATION_JW_THRESHOLD) {
                return Expansion {
                    text: entry.expansion.clone(),
                    fired: Some(entry.term.canonical.clone()),
                };
            }
        }

        // Pass 2: anywhere triggers, spliced over their byte spans — the
        // same consume-and-splice algorithm as `Corrector::correct`, so
        // surrounding punctuation survives without a reattachment step.
        let mut consumed = vec![false; tokens.len()];
        let mut splices: Vec<(usize, usize, &str)> = Vec::new();
        let mut fired: Option<&str> = None;
        for entry in &self.entries {
            if entry.scope != Scope::Anywhere {
                continue;
            }
            let n = entry.term.word_count();
            if n == 0 || n > tokens.len() {
                continue;
            }
            for i in 0..=(tokens.len() - n) {
                if consumed[i..i + n].iter().any(|&c| c) {
                    continue;
                }
                if !matcher::window_matches(
                    &entry.term,
                    &tokens[i..i + n],
                    &codes[i..i + n],
                    INVOCATION_JW_THRESHOLD,
                ) {
                    continue;
                }
                consumed[i..i + n].fill(true);
                splices.push((tokens[i].start, tokens[i + n - 1].end, &entry.expansion));
                fired.get_or_insert(entry.term.canonical.as_str());
            }
        }
        let Some(fired) = fired else {
            return unchanged();
        };

        splices.sort_unstable_by_key(|&(start, _, _)| start);
        let mut out = String::with_capacity(text.len() + 64);
        let mut cursor = 0;
        for (start, end, expansion) in splices {
            out.push_str(&text[cursor..start]);
            out.push_str(expansion);
            cursor = end;
        }
        out.push_str(&text[cursor..]);
        Expansion {
            text: out,
            fired: Some(fired.to_string()),
        }
    }

    /// The trigger this text came closest to, and how close (mean
    /// Jaro-Winkler over the best-aligned window, 0.0 to 1.0). Powers the
    /// Invocations test panel's near-miss hint; never a matching decision,
    /// so it ignores scope and phonetic codes entirely.
    pub fn closest(&self, text: &str) -> Option<(&str, f64)> {
        let tokens = tokenize::tokenize(text);
        let mut best: Option<(&str, f64)> = None;
        for entry in &self.entries {
            let n = entry.term.word_count();
            if n == 0 || n > tokens.len() {
                continue;
            }
            for i in 0..=(tokens.len() - n) {
                let score = matcher::window_similarity(&entry.term, &tokens[i..i + n]);
                if best.is_none_or(|(_, high)| score > high) {
                    best = Some((entry.term.canonical.as_str(), score));
                }
            }
        }
        best
    }
}

/// A trigger's word count under the matcher's own tokenizer, so the editor's
/// "needs at least two words" check and the build-time gate can never
/// disagree (hyphens split: "access-granted" is two words, not one).
pub fn phrase_word_count(phrase: &str) -> usize {
    tokenize::tokenize(phrase).len()
}

/// A trigger's identity for duplicate detection: its lowercased token
/// sequence, so "Access Granted", "access  granted!" and "Access-Granted"
/// are all the same trigger.
pub fn normalized_phrase(phrase: &str) -> String {
    tokenize::tokenize(phrase)
        .into_iter()
        .map(|t| t.lower)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rphonetic::{DoubleMetaphone, Encoder};

    const PARAGRAPH: &str = "You have access to the Support Forge tools:\nticketing, \
                             remote assist, and the asset inventory.";

    fn expander(entries: &[(&str, &str, Scope)]) -> Expander {
        let owned: Vec<(String, String, Scope)> = entries
            .iter()
            .map(|(p, e, s)| (p.to_string(), e.to_string(), *s))
            .collect();
        Expander::new(&owned)
    }

    fn granted(scope: Scope) -> Expander {
        expander(&[("access granted", PARAGRAPH, scope)])
    }

    // --- firing

    #[test]
    fn whole_utterance_trigger_fires_and_returns_only_the_expansion() {
        let out = granted(Scope::Utterance).expand("Access granted.");
        assert_eq!(out.text, PARAGRAPH, "the expansion replaces everything");
        assert_eq!(out.fired.as_deref(), Some("access granted"));
    }

    /// Permanent guard. Whole-dictation scope is the default precisely so a
    /// trigger buried in a sentence cannot paste a paragraph mid-thought; if
    /// this ever fires, the default has stopped being safe.
    #[test]
    fn trigger_inside_a_sentence_never_fires_in_utterance_scope() {
        let text = "I confirmed access granted for the new tech";
        let out = granted(Scope::Utterance).expand(text);
        assert_eq!(out.text, text);
        assert_eq!(out.fired, None);
    }

    #[test]
    fn anywhere_scope_splices_inline_and_preserves_punctuation() {
        let out = granted(Scope::Anywhere).expand("I confirmed (access granted), thanks.");
        assert_eq!(out.text, format!("I confirmed ({PARAGRAPH}), thanks."));
        assert_eq!(out.fired.as_deref(), Some("access granted"));
    }

    #[test]
    fn access_granite_still_fires_for_access_granted() {
        // The recall case that justifies fuzzy matching at all: the provider
        // mishears the trigger's last word, and the invocation still fires.
        let out = granted(Scope::Utterance).expand("access granite");
        assert_eq!(out.text, PARAGRAPH);
        assert_eq!(out.fired.as_deref(), Some("access granted"));
    }

    #[test]
    fn casing_and_surrounding_punctuation_are_irrelevant_to_a_whole_utterance_hit() {
        for spoken in [
            "ACCESS GRANTED",
            "  access granted!  ",
            "\"Access Granted?\"",
        ] {
            let out = granted(Scope::Utterance).expand(spoken);
            assert_eq!(out.text, PARAGRAPH, "spoken: {spoken:?}");
        }
    }

    // --- the tightened confirm threshold

    /// Permanent guard against a future session "unifying the matchers" back
    /// down to the dictionary's 0.85. The pair below is chosen so the two
    /// thresholds actually disagree: the dictionary would replace it, the
    /// invocation must not.
    #[test]
    fn phonetic_near_miss_below_0_90_never_fires() {
        let dm = DoubleMetaphone::default();
        assert_eq!(
            dm.encode("granted"),
            dm.encode("grantor"),
            "the pair must clear the phonetic gate, or this guards nothing"
        );
        let score = strsim::jaro_winkler("granted", "grantor");
        assert!(
            (matcher::JW_CONFIRM_THRESHOLD..INVOCATION_JW_THRESHOLD).contains(&score),
            "the pair must sit between the two thresholds; got {score}"
        );

        let out = granted(Scope::Utterance).expand("access grantor");
        assert_eq!(out.text, "access grantor", "0.90 must reject it");
        assert_eq!(out.fired, None);

        // The same pair on the dictionary's unchanged 0.85 path still fires,
        // which is what makes this a threshold test and not a spelling test.
        let (corrected, n) = crate::Corrector::new(&["granted".to_string()]).correct("grantor");
        assert_eq!((corrected.as_str(), n), ("granted", 1));
    }

    // --- build-time hygiene

    #[test]
    fn one_word_phrase_is_skipped_at_build_time() {
        let e = expander(&[("granted", PARAGRAPH, Scope::Utterance)]);
        assert_eq!(e.armed(), 0);
        assert_eq!(e.skipped(), 1);
        assert_eq!(e.expand("granted").fired, None);
    }

    #[test]
    fn empty_expansion_is_skipped_at_build_time() {
        let e = expander(&[("access granted", "", Scope::Utterance)]);
        assert_eq!(e.armed(), 0);
        assert_eq!(e.skipped(), 1);
    }

    #[test]
    fn duplicate_phrase_is_first_wins() {
        let e = expander(&[
            ("access granted", "first", Scope::Utterance),
            // Same trigger, different spelling of the same token sequence.
            ("Access-Granted!", "second", Scope::Utterance),
        ]);
        assert_eq!(e.armed(), 1);
        assert_eq!(e.skipped(), 1);
        assert_eq!(e.expand("access granted").text, "first");
    }

    #[test]
    fn empty_invocation_set_is_identity() {
        let e = expander(&[]);
        assert_eq!(e.armed(), 0);
        let out = e.expand("nothing to do here");
        assert_eq!(out.text, "nothing to do here");
        assert_eq!(out.fired, None);
    }

    #[test]
    fn identity_on_empty_and_punctuation_only_input() {
        let e = granted(Scope::Anywhere);
        for text in ["", "...!"] {
            let out = e.expand(text);
            assert_eq!(out.text, text);
            assert_eq!(out.fired, None);
        }
    }

    // --- overlap and ordering

    #[test]
    fn longer_trigger_wins_an_overlap() {
        let e = expander(&[
            ("access granted", "SHORT", Scope::Anywhere),
            ("access granted today", "LONG", Scope::Anywhere),
        ]);
        assert_eq!(e.expand("so access granted today ok").text, "so LONG ok");
        // The shorter trigger still fires when the longer does not apply.
        assert_eq!(e.expand("so access granted ok").text, "so SHORT ok");
    }

    #[test]
    fn utterance_scope_is_checked_before_anywhere_scope() {
        let e = expander(&[
            ("access granted", "ANYWHERE", Scope::Anywhere),
            ("access granted", "WHOLE", Scope::Utterance),
        ]);
        // Same trigger twice is a duplicate: first wins, so only the
        // anywhere entry is armed and it still splices a bare utterance.
        assert_eq!(e.armed(), 1);
        assert_eq!(e.expand("access granted").text, "ANYWHERE");
    }

    #[test]
    fn two_different_anywhere_triggers_both_splice_and_fired_reports_the_first() {
        let e = expander(&[
            ("access granted", "AAA", Scope::Anywhere),
            ("ticket closed", "BBB", Scope::Anywhere),
        ]);
        let out = e.expand("access granted then ticket closed");
        assert_eq!(out.text, "AAA then BBB");
        assert!(out.fired.is_some(), "a splice always names a trigger");
    }

    #[test]
    fn a_scope_mismatch_leaves_the_text_completely_alone() {
        // An anywhere-only set that matches nothing must not allocate a
        // rewritten string or claim a trigger fired.
        let out = granted(Scope::Anywhere).expand("entirely unrelated words");
        assert_eq!(out.text, "entirely unrelated words");
        assert_eq!(out.fired, None);
    }

    // --- helpers shared with the editor

    #[test]
    fn phrase_word_count_matches_the_matcher_tokenizer() {
        assert_eq!(phrase_word_count(""), 0);
        assert_eq!(phrase_word_count("  !!  "), 0);
        assert_eq!(phrase_word_count("granted"), 1);
        assert_eq!(phrase_word_count("access granted"), 2);
        // Hyphens split, so this clears the two-word gate.
        assert_eq!(phrase_word_count("access-granted"), 2);
    }

    #[test]
    fn normalized_phrase_collapses_casing_punctuation_and_hyphens() {
        assert_eq!(normalized_phrase("Access Granted"), "access granted");
        assert_eq!(normalized_phrase("  access   granted!  "), "access granted");
        assert_eq!(normalized_phrase("Access-Granted"), "access granted");
        assert_eq!(normalized_phrase(""), "");
    }

    // --- the near-miss hint

    #[test]
    fn closest_reports_the_nearest_trigger_and_its_score() {
        let e = expander(&[
            ("access granted", PARAGRAPH, Scope::Utterance),
            ("ticket closed", "x", Scope::Utterance),
        ]);
        let (phrase, score) = e.closest("access grantor").expect("a trigger is near");
        assert_eq!(phrase, "access granted");
        assert!(
            score > 0.9,
            "a one-word near miss still scores high: {score}"
        );

        // An exact hit scores 1.0; nothing similar scores low.
        let (_, exact) = e.closest("access granted").expect("exact");
        assert_eq!(exact, 1.0);
        let (_, far) = e.closest("zzzz yyyy").expect("still reports the best");
        assert!(far < 0.5, "unrelated text must not look close: {far}");
    }

    #[test]
    fn closest_is_none_when_nothing_can_be_compared() {
        assert!(expander(&[]).closest("anything").is_none());
        // Fewer spoken words than the shortest trigger: no window exists.
        assert!(granted(Scope::Utterance).closest("access").is_none());
    }
}

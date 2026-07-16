//! Phonetic post-correction: after the STT provider returns a transcript,
//! find spans that *sound like* a dictionary term but are spelled wrong and
//! replace them with the canonical spelling, before injection.
//!
//! Pure text processing, no I/O, no async. Runs on the pipeline worker
//! thread inside the release-to-inject latency budget (target: well under
//! 10 ms for a 100-word utterance against a 200-term dictionary).
//!
//! Matching is Double Metaphone code equality confirmed by a Jaro-Winkler
//! score, with exact-only fallbacks for words the phonetic algorithm cannot
//! encode usefully (digits, very short words).

mod matcher;
mod tokenize;

use matcher::TermEntry;
use rphonetic::DoubleMetaphone;

/// Corrects transcripts against a fixed set of canonical terms.
///
/// Construction precomputes per-term data once; `correct` is called per
/// dictation on the hot path.
pub struct Corrector {
    dm: DoubleMetaphone,
    entries: Vec<TermEntry>,
}

impl Corrector {
    /// Precomputes phonetic codes per term word at construction; call once
    /// at startup, not per dictation.
    pub fn new(terms: &[String]) -> Corrector {
        let dm = DoubleMetaphone::default();
        Corrector {
            dm,
            entries: matcher::build_entries(&dm, terms),
        }
    }

    /// Returns the corrected text and the number of replacements made.
    ///
    /// Never fails: any internal anomaly (unencodable token, empty input)
    /// degrades to returning the input span unchanged. A "no match" outcome
    /// means "left as transcribed", not "verified correct".
    pub fn correct(&self, text: &str) -> (String, usize) {
        if self.entries.is_empty() || text.is_empty() {
            return (text.to_string(), 0);
        }
        let tokens = tokenize::tokenize(text);
        if tokens.is_empty() {
            return (text.to_string(), 0);
        }
        let token_codes: Vec<matcher::Codes> = tokens
            .iter()
            .map(|t| matcher::encode(&self.dm, &t.lower))
            .collect();

        // Matched windows become splices: (byte range, replacement). Tokens
        // consumed by a match are skipped by later (shorter) terms.
        let mut consumed = vec![false; tokens.len()];
        let mut splices: Vec<(usize, usize, &str)> = Vec::new();
        let mut replacements = 0;
        for entry in &self.entries {
            let n = entry.word_count();
            if n == 0 || n > tokens.len() {
                continue;
            }
            for i in 0..=(tokens.len() - n) {
                if consumed[i..i + n].iter().any(|&c| c) {
                    continue;
                }
                if !matcher::window_matches(entry, &tokens[i..i + n], &token_codes[i..i + n]) {
                    continue;
                }
                consumed[i..i + n].fill(true);
                let (start, end) = (tokens[i].start, tokens[i + n - 1].end);
                // Already canonical: consume (so overlapping terms skip it)
                // but splice nothing and count nothing.
                if text[start..end] != entry.canonical {
                    splices.push((start, end, &entry.canonical));
                    replacements += 1;
                }
            }
        }
        if splices.is_empty() {
            return (text.to_string(), 0);
        }

        splices.sort_unstable_by_key(|&(start, _, _)| start);
        let mut out = String::with_capacity(text.len() + 16);
        let mut cursor = 0;
        for (start, end, canonical) in splices {
            out.push_str(&text[cursor..start]);
            out.push_str(canonical);
            cursor = end;
        }
        out.push_str(&text[cursor..]);
        (out, replacements)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rphonetic::{DoubleMetaphone, Encoder};

    fn corrector(terms: &[&str]) -> Corrector {
        let owned: Vec<String> = terms.iter().map(|t| t.to_string()).collect();
        Corrector::new(&owned)
    }

    #[test]
    fn identity_when_dictionary_is_empty() {
        let (out, n) = corrector(&[]).correct("hello, world");
        assert_eq!(out, "hello, world");
        assert_eq!(n, 0);
    }

    #[test]
    fn identity_on_empty_and_punctuation_only_input() {
        let c = corrector(&["Modero"]);
        assert_eq!(c.correct(""), (String::new(), 0));
        assert_eq!(c.correct("...!"), (String::from("...!"), 0));
    }

    // --- CP3: single-word matching.

    #[test]
    fn phonetic_misspelling_is_replaced_with_canonical() {
        let c = corrector(&["Modero"]);
        let (out, n) = c.correct("I meant madero here");
        assert_eq!(out, "I meant Modero here");
        assert_eq!(n, 1);
    }

    #[test]
    fn dropped_double_consonant_is_replaced() {
        let c = corrector(&["Vossburg"]);
        let (out, n) = c.correct("ask vosburg about it");
        assert_eq!(out, "ask Vossburg about it");
        assert_eq!(n, 1);
    }

    #[test]
    fn phonetic_collision_failing_jw_guard_is_left_alone() {
        // "matter" shares Modero's Double Metaphone code, so only the
        // Jaro-Winkler guard stands between it and a false positive.
        let dm = DoubleMetaphone::default();
        assert_eq!(dm.encode("modero"), dm.encode("matter"));
        assert!(strsim::jaro_winkler("modero", "matter") < 0.85);

        let c = corrector(&["Modero"]);
        let (out, n) = c.correct("it does not matter");
        assert_eq!(out, "it does not matter");
        assert_eq!(n, 0);
    }

    #[test]
    fn case_insensitive_exact_hit_gets_canonical_casing() {
        let c = corrector(&["Modero"]);
        let (out, n) = c.correct("modero is live");
        assert_eq!(out, "Modero is live");
        assert_eq!(n, 1);
    }

    #[test]
    fn already_canonical_is_a_no_op_and_not_counted() {
        let c = corrector(&["Modero"]);
        let (out, n) = c.correct("Modero is live");
        assert_eq!(out, "Modero is live");
        assert_eq!(n, 0);
    }

    #[test]
    fn short_words_are_exact_only() {
        // <= 3 chars: no phonetic path, so near-misses stay put and only
        // the case-insensitive exact hit canonicalizes.
        let c = corrector(&["AWS"]);
        assert_eq!(c.correct("was it aws"), ("was it AWS".to_string(), 1));
        assert_eq!(c.correct("was it not"), ("was it not".to_string(), 0));
    }

    #[test]
    fn words_with_digits_are_exact_only() {
        let c = corrector(&["v2"]);
        assert_eq!(c.correct("ship V2 today"), ("ship v2 today".to_string(), 1));
        // "va" sounds close but digits never match phonetically.
        assert_eq!(c.correct("ship va today"), ("ship va today".to_string(), 0));
    }

    #[test]
    fn multiple_occurrences_all_replaced_and_counted() {
        let c = corrector(&["Modero"]);
        let (out, n) = c.correct("madero, then modero again");
        assert_eq!(out, "Modero, then Modero again");
        assert_eq!(n, 2);
    }

    #[test]
    fn punctuation_around_a_match_survives() {
        let c = corrector(&["Modero"]);
        let (out, n) = c.correct("(madero), right?");
        assert_eq!(out, "(Modero), right?");
        assert_eq!(n, 1);
    }

    #[test]
    fn non_ascii_term_corrects_ascii_misspelling() {
        // Umlauts encode like their base letters, so the phonetic path
        // bridges the accent gap; JW confirms.
        let dm = DoubleMetaphone::default();
        assert_eq!(dm.encode("müller"), dm.encode("muller"));

        let c = corrector(&["Müller"]);
        let (out, n) = c.correct("tell muller");
        assert_eq!(out, "tell Müller");
        assert_eq!(n, 1);
    }

    #[test]
    fn common_short_words_never_rewrite() {
        // Transcripts are full of these; none may ever be touched.
        let c = corrector(&["Modero", "Vossburg", "AWS"]);
        let text = "I was at a mode of work as ever";
        assert_eq!(c.correct(text), (text.to_string(), 0));
    }

    // --- CP0 proof tests: pin the third-party behavior the matcher relies
    // on. If a dependency upgrade breaks one of these, the matching
    // assumptions need re-checking before anything else.

    #[test]
    fn rphonetic_does_not_panic_on_edge_inputs() {
        let dm = DoubleMetaphone::default();
        // Empty, non-ASCII, digits, hyphens: all must encode without
        // panicking (the values themselves are unspecified by the docs).
        for input in ["", "müller", "nova3", "hark-stt", "3", "ü"] {
            let primary = dm.encode(input);
            let alternate = dm.encode_alternate(input);
            // Default max code length is 4 (documented).
            assert!(primary.len() <= 4, "primary {primary:?} for {input:?}");
            assert!(
                alternate.len() <= 4,
                "alternate {alternate:?} for {input:?}"
            );
        }
    }

    #[test]
    fn rphonetic_matches_vowel_variant_misspellings() {
        // The property the whole dictionary rests on: ASR vowel swaps do
        // not change the Double Metaphone code.
        let dm = DoubleMetaphone::default();
        assert_eq!(dm.encode("modero"), dm.encode("madero"));
        assert_eq!(dm.encode("smith"), dm.encode("smyth"));
    }

    #[test]
    fn rphonetic_codes_are_case_insensitive() {
        let dm = DoubleMetaphone::default();
        assert_eq!(dm.encode("Modero"), dm.encode("modero"));
        assert_eq!(dm.encode("VOSSBURG"), dm.encode("vossburg"));
    }

    #[test]
    fn strsim_jaro_winkler_is_one_for_equal_single_char_strings() {
        // Historical strsim bug (0 instead of 1, fixed in 0.9.3); transcripts
        // are full of "a"/"I", so keep this regression guard forever.
        assert_eq!(strsim::jaro_winkler("a", "a"), 1.0);
        assert_eq!(strsim::jaro_winkler("i", "i"), 1.0);
    }

    #[test]
    fn strsim_jaro_winkler_handles_empty_strings() {
        assert_eq!(strsim::jaro_winkler("", ""), 1.0);
        assert_eq!(strsim::jaro_winkler("a", ""), 0.0);
        assert_eq!(strsim::jaro_winkler("", "a"), 0.0);
    }

    #[test]
    fn strsim_jaro_winkler_clears_threshold_for_flagship_misspelling() {
        assert!(strsim::jaro_winkler("madero", "modero") >= 0.85);
    }
}

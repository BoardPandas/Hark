//! Phonetic post-correction: after the STT provider returns a transcript,
//! find spans that *sound like* a dictionary term but are spelled wrong and
//! replace them with the canonical spelling, before injection.
//!
//! Pure text processing, no I/O, no async. Runs on the pipeline worker
//! thread inside the release-to-inject latency budget (target: well under
//! 10 ms for a 100-word utterance against a 200-term dictionary).
//!
//! Matching (CP2-CP4) is Double Metaphone code equality confirmed by a
//! Jaro-Winkler score, with exact-only fallbacks for words the phonetic
//! algorithm cannot encode usefully (digits, very short words).

/// Corrects transcripts against a fixed set of canonical terms.
///
/// Construction precomputes per-term data once; `correct` is called per
/// dictation on the hot path.
pub struct Corrector {
    /// Canonical terms as configured. Precomputed match entries replace
    /// this raw list when matching lands (CP3).
    #[allow(dead_code)]
    terms: Vec<String>,
}

impl Corrector {
    /// Precomputes phonetic codes per term word at construction; call once
    /// at startup, not per dictation.
    pub fn new(terms: &[String]) -> Corrector {
        Corrector {
            terms: terms.to_vec(),
        }
    }

    /// Returns the corrected text and the number of replacements made.
    ///
    /// Never fails: any internal anomaly (unencodable token, empty input)
    /// degrades to returning the input span unchanged. A "no match" outcome
    /// means "left as transcribed", not "verified correct".
    pub fn correct(&self, text: &str) -> (String, usize) {
        // CP0 identity pass; matching lands in CP3/CP4.
        (text.to_string(), 0)
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
    fn identity_correct_returns_input_unchanged() {
        let c = corrector(&["Modero", "hark-stt"]);
        let (out, n) = c.correct("hello, world");
        assert_eq!(out, "hello, world");
        assert_eq!(n, 0);
    }

    #[test]
    fn identity_correct_handles_empty_input_and_empty_dictionary() {
        let (out, n) = corrector(&[]).correct("");
        assert_eq!(out, "");
        assert_eq!(n, 0);
    }

    // --- rphonetic proof tests: pin the third-party behavior the matcher
    // will rely on. If an upgrade breaks one of these, the matching
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

    // --- strsim proof tests.

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
        // "madero" -> "Modero" is the canonical CP3 example; prove the
        // planned 0.85 confirmation threshold accepts it.
        assert!(strsim::jaro_winkler("madero", "modero") >= 0.85);
    }
}

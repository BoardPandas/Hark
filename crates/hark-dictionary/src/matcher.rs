//! Term precomputation and window matching.
//!
//! Every term word takes one of two paths, decided at construction:
//! - **exact-only**: words with digits or of <= 3 chars (Double Metaphone
//!   codes degenerate at short lengths and cannot encode digits), matched
//!   by case-insensitive equality;
//! - **phonetic**: Double Metaphone code equality (primary or alternate on
//!   either side) confirmed by a Jaro-Winkler score, the false-positive
//!   guard.

use crate::tokenize::{tokenize, Token};
use rphonetic::{DoubleMetaphone, Encoder};

/// Jaro-Winkler confirmation threshold after a phonetic-code match.
/// Research-informed guess, tuned at the CP6 interactive gate; promote to
/// config only if real usage demands it.
const JW_CONFIRM_THRESHOLD: f64 = 0.85;

/// Double Metaphone primary + alternate codes.
pub(crate) struct Codes {
    primary: String,
    alternate: String,
}

pub(crate) fn encode(dm: &DoubleMetaphone, lower: &str) -> Codes {
    Codes {
        primary: dm.encode(lower),
        alternate: dm.encode_alternate(lower),
    }
}

/// One comparable word of a term.
struct TermWord {
    lower: String,
    /// `None` routes the word down the exact-only path.
    codes: Option<Codes>,
}

/// One dictionary term, precomputed for matching.
pub(crate) struct TermEntry {
    /// The replacement text, verbatim: canonical spelling includes its own
    /// casing, and that is the point of the dictionary.
    pub canonical: String,
    words: Vec<TermWord>,
}

impl TermEntry {
    /// The window size this term needs, in tokens.
    pub fn word_count(&self) -> usize {
        self.words.len()
    }
}

/// Precompute entries from the configured terms. Terms that tokenize to
/// nothing (empty, all punctuation) are dropped.
pub(crate) fn build_entries(dm: &DoubleMetaphone, terms: &[String]) -> Vec<TermEntry> {
    terms
        .iter()
        .filter_map(|term| {
            let words: Vec<TermWord> = tokenize(term)
                .into_iter()
                .map(|t| term_word(dm, t.lower))
                .collect();
            if words.is_empty() {
                return None;
            }
            Some(TermEntry {
                canonical: term.clone(),
                words,
            })
        })
        .collect()
}

fn term_word(dm: &DoubleMetaphone, lower: String) -> TermWord {
    let phonetic_eligible = lower.chars().count() >= 4 && lower.chars().all(|c| c.is_alphabetic());
    let codes = if phonetic_eligible {
        let codes = encode(dm, &lower);
        // An unencodable word (empty code) degrades to exact-only rather
        // than matching everything else that encodes to "".
        (!codes.primary.is_empty()).then_some(codes)
    } else {
        None
    };
    TermWord { lower, codes }
}

/// Does this term match the token window? `token_codes` is parallel to
/// `tokens` (precomputed once per transcript, not per term).
pub(crate) fn window_matches(entry: &TermEntry, tokens: &[Token], token_codes: &[Codes]) -> bool {
    debug_assert_eq!(entry.words.len(), tokens.len());
    entry
        .words
        .iter()
        .zip(tokens.iter().zip(token_codes))
        .all(|(word, (token, codes))| word_matches(word, token, codes))
}

fn word_matches(word: &TermWord, token: &Token, token_codes: &Codes) -> bool {
    // Equal spellings match on either path (and need no JW confirm).
    if word.lower == token.lower {
        return true;
    }
    let Some(term_codes) = &word.codes else {
        return false; // exact-only, and equality already failed
    };
    codes_intersect(term_codes, token_codes)
        && strsim::jaro_winkler(&word.lower, &token.lower) >= JW_CONFIRM_THRESHOLD
}

/// Any non-empty code equal on both sides. Empty codes (unencodable input)
/// never match anything.
fn codes_intersect(a: &Codes, b: &Codes) -> bool {
    let pairs = [
        (&a.primary, &b.primary),
        (&a.primary, &b.alternate),
        (&a.alternate, &b.primary),
        (&a.alternate, &b.alternate),
    ];
    pairs.iter().any(|(x, y)| !x.is_empty() && x == y)
}

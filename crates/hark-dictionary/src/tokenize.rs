//! Transcript tokenization: word cores with byte spans.
//!
//! A token is the comparable core of a word. Leading/trailing punctuation
//! is never part of the span, so replacement (splicing canonical text over
//! core spans) preserves it without any reattachment step. Interior hyphens
//! split a chunk into separate tokens so hyphen-split dictionary terms
//! ("hark-stt") match both "hark stt" and "hark-stt" with one window size.

/// One comparable word from the transcript: the byte span of its core in
/// the original text (original casing preserved there) plus a lowercased
/// copy for comparison.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Token {
    /// Byte offset of the core's first char in the original text.
    pub start: usize,
    /// Byte offset one past the core's last char.
    pub end: usize,
    /// Lowercased core, the comparison side.
    pub lower: String,
}

/// Byte offset of `slice` within `parent`. Only valid for subslices of
/// `parent`, which `split`/`split_whitespace` guarantee.
fn offset_in(parent: &str, slice: &str) -> usize {
    slice.as_ptr() as usize - parent.as_ptr() as usize
}

// Wired into the matcher at CP3.
#[allow(dead_code)]
pub(crate) fn tokenize(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    for chunk in text.split_whitespace() {
        for segment in chunk.split('-') {
            // Trim non-alphanumeric edges; what remains is the core.
            let Some(first) = segment.find(|c: char| c.is_alphanumeric()) else {
                continue; // all punctuation (or empty between hyphens)
            };
            let last = segment
                .char_indices()
                .rev()
                .find(|(_, c)| c.is_alphanumeric())
                .map(|(i, c)| i + c.len_utf8())
                .expect("a first alphanumeric char implies a last one");
            let start = offset_in(text, segment) + first;
            let end = offset_in(text, segment) + last;
            tokens.push(Token {
                start,
                end,
                lower: text[start..end].to_lowercase(),
            });
        }
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The core texts, as sliced from the original via the spans.
    fn cores<'a>(text: &'a str, tokens: &[Token]) -> Vec<&'a str> {
        tokens.iter().map(|t| &text[t.start..t.end]).collect()
    }

    #[test]
    fn plain_words_span_their_full_text() {
        let text = "hello world";
        let tokens = tokenize(text);
        assert_eq!(cores(text, &tokens), vec!["hello", "world"]);
        assert_eq!(tokens[0].start, 0);
        assert_eq!(tokens[1].start, 6);
    }

    #[test]
    fn trailing_punctuation_stays_outside_the_span() {
        let text = "modero, then";
        let tokens = tokenize(text);
        assert_eq!(cores(text, &tokens), vec!["modero", "then"]);
        assert_eq!(&text[tokens[0].end..tokens[0].end + 1], ",");
    }

    #[test]
    fn surrounding_punctuation_stays_outside_the_span() {
        let text = "(modero) \"quoted\" 'apos'";
        let tokens = tokenize(text);
        assert_eq!(cores(text, &tokens), vec!["modero", "quoted", "apos"]);
    }

    #[test]
    fn interior_apostrophe_stays_in_the_core() {
        let text = "don't stop";
        assert_eq!(cores(text, &tokenize(text)), vec!["don't", "stop"]);
    }

    #[test]
    fn hyphenated_chunks_split_into_separate_tokens() {
        let text = "run hark-stt now";
        let tokens = tokenize(text);
        assert_eq!(cores(text, &tokens), vec!["run", "hark", "stt", "now"]);
        // The split tokens keep true positions: "stt" starts after "hark-".
        assert_eq!(&text[tokens[1].start..tokens[2].end], "hark-stt");
    }

    #[test]
    fn hyphenated_chunk_with_punctuation_trims_each_segment() {
        let text = "(hark-stt),";
        assert_eq!(cores(text, &tokenize(text)), vec!["hark", "stt"]);
    }

    #[test]
    fn unicode_words_survive_with_correct_spans() {
        let text = "müller café.";
        let tokens = tokenize(text);
        assert_eq!(cores(text, &tokens), vec!["müller", "café"]);
        assert_eq!(tokens[0].lower, "müller");
    }

    #[test]
    fn casing_is_preserved_in_spans_and_lowered_in_copies() {
        let text = "Modero VOSSBURG";
        let tokens = tokenize(text);
        assert_eq!(cores(text, &tokens), vec!["Modero", "VOSSBURG"]);
        assert_eq!(tokens[0].lower, "modero");
        assert_eq!(tokens[1].lower, "vossburg");
    }

    #[test]
    fn empty_and_all_punctuation_inputs_yield_no_tokens() {
        assert!(tokenize("").is_empty());
        assert!(tokenize("   ").is_empty());
        assert!(tokenize("... -- !?").is_empty());
    }

    #[test]
    fn repeated_whitespace_and_newlines_are_skipped() {
        let text = "a   b\t\nc";
        let tokens = tokenize(text);
        assert_eq!(cores(text, &tokens), vec!["a", "b", "c"]);
        assert_eq!(tokens[2].start, 7);
    }

    #[test]
    fn digits_are_core_material() {
        let text = "nova-3 v2";
        let tokens = tokenize(text);
        assert_eq!(cores(text, &tokens), vec!["nova", "3", "v2"]);
    }
}

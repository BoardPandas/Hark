//! Snapping a text selection outward to whole spellbook tokens.
//!
//! The History panel lets the user highlight a span and add it to the
//! Spellbook. Two problems this solves, one cosmetic and one load-bearing:
//!
//! - **Forgiveness.** Dragging across "Al Drazi" and clipping it to "l Draz"
//!   is the normal outcome of a quick gesture, not a mistake worth punishing.
//! - **Correctness.** The captured span must tokenize exactly the way
//!   [`crate::Corrector`] will tokenize it later. A hand-rolled word split in
//!   the UI would drift from [`crate::tokenize`] the first time either changed,
//!   and the symptom would be an entry the user added that silently never
//!   fires. Snapping through the real tokenizer makes that class of bug
//!   impossible rather than unlikely.
//!
//! Selections arrive from egui as **character** indices (`CCursor::index`),
//! while the tokenizer works in **byte** offsets. The conversion lives here so
//! no caller has to think about it.

use crate::tokenize::tokenize;
use std::ops::Range;

/// Expand `chars` outward to cover every token it touches.
///
/// `chars` is a half-open range of character indices into `text`, in either
/// order (a right-to-left drag is normalized). The result is a character range
/// that begins at the start of the first token touched and ends at the end of
/// the last, with punctuation and whitespace at the edges excluded — the
/// tokenizer already keeps those outside token spans.
///
/// A zero-width range (a click, not a drag) snaps to the token it sits in or
/// against, so clicking a word selects that word.
///
/// Returns `None` when the selection touches no token at all: an empty range,
/// or one covering only whitespace and punctuation. There is nothing to add to
/// the Spellbook in that case, and the caller should leave its button disabled
/// rather than offer an entry made of spaces.
pub fn snap_to_tokens(text: &str, chars: Range<usize>) -> Option<Range<usize>> {
    let (from, to) = if chars.start <= chars.end {
        (chars.start, chars.end)
    } else {
        (chars.end, chars.start)
    };
    let start_byte = byte_of_char(text, from)?;
    let end_byte = byte_of_char(text, to)?;

    let tokens = tokenize(text);
    // Half-open overlap, except that a zero-width selection has no interior to
    // overlap with -- it is a caret, and a caret resting on either edge of a
    // word still means that word.
    let touched: Vec<_> = tokens
        .iter()
        .filter(|t| {
            if start_byte == end_byte {
                t.start <= start_byte && start_byte <= t.end
            } else {
                t.start < end_byte && start_byte < t.end
            }
        })
        .collect();

    let first = touched.first()?;
    let last = touched.last()?;
    Some(char_of_byte(text, first.start)..char_of_byte(text, last.end))
}

/// The snapped span itself, ready to become a Spellbook term.
pub fn snapped_text(text: &str, chars: Range<usize>) -> Option<&str> {
    let snapped = snap_to_tokens(text, chars)?;
    let start = byte_of_char(text, snapped.start)?;
    let end = byte_of_char(text, snapped.end)?;
    Some(&text[start..end])
}

/// Byte offset of character `n`. `None` if `n` is past the end, which is a
/// caller bug (a stale selection against changed text) rather than something
/// to paper over with a clamp: silently snapping to the end would add the
/// wrong term.
fn byte_of_char(text: &str, n: usize) -> Option<usize> {
    if n == 0 {
        return Some(0);
    }
    text.char_indices()
        .nth(n)
        .map(|(i, _)| i)
        .or(if n == text.chars().count() {
            Some(text.len())
        } else {
            None
        })
}

/// Character index of a byte offset. The offset always comes from the
/// tokenizer, so it is always on a char boundary.
fn char_of_byte(text: &str, byte: usize) -> usize {
    text[..byte].chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Character range of the first occurrence of `needle`, the way a user's
    /// drag over that text would arrive.
    fn sel(text: &str, needle: &str) -> Range<usize> {
        let byte = text.find(needle).expect("needle is present");
        let start = text[..byte].chars().count();
        start..start + needle.chars().count()
    }

    const HISTORY: &str = "I cast the Al Drazi commander";

    #[test]
    fn a_clipped_drag_expands_to_the_whole_phrase() {
        // The motivating case: a quick drag that misses both edges.
        assert_eq!(
            snapped_text(HISTORY, sel(HISTORY, "l Draz")),
            Some("Al Drazi")
        );
    }

    #[test]
    fn an_exact_selection_is_unchanged() {
        assert_eq!(
            snapped_text(HISTORY, sel(HISTORY, "Al Drazi")),
            Some("Al Drazi")
        );
    }

    #[test]
    fn trailing_whitespace_is_dropped_rather_than_extending_to_the_next_word() {
        assert_eq!(
            snapped_text(HISTORY, sel(HISTORY, "Al Drazi ")),
            Some("Al Drazi")
        );
    }

    #[test]
    fn a_click_selects_only_the_word_it_lands_in() {
        // Zero-width, inside "Drazi": must NOT drag "Al" along with it.
        let caret = sel(HISTORY, "raz").start;
        assert_eq!(snapped_text(HISTORY, caret..caret), Some("Drazi"));
    }

    #[test]
    fn a_click_at_either_edge_of_a_word_still_selects_it() {
        let word = sel(HISTORY, "Drazi");
        assert_eq!(snapped_text(HISTORY, word.start..word.start), Some("Drazi"));
        assert_eq!(snapped_text(HISTORY, word.end..word.end), Some("Drazi"));
    }

    #[test]
    fn a_right_to_left_drag_is_normalized() {
        let s = sel(HISTORY, "l Draz");
        assert_eq!(snapped_text(HISTORY, s.end..s.start), Some("Al Drazi"));
    }

    #[test]
    fn a_selection_of_only_whitespace_yields_nothing() {
        // Between "cast" and "the": a caret in a gap has no word to mean.
        let gap = sel(HISTORY, " the").start;
        assert_eq!(snapped_text(HISTORY, gap..gap + 1), None);
    }

    #[test]
    fn a_selection_of_only_punctuation_yields_nothing() {
        let text = "wait -- what";
        assert_eq!(snapped_text(text, sel(text, "--")), None);
    }

    #[test]
    fn an_empty_selection_in_empty_text_yields_nothing() {
        assert_eq!(snapped_text("", 0..0), None);
        assert_eq!(snapped_text("   ", 1..2), None);
    }

    #[test]
    fn punctuation_at_the_edges_is_excluded_from_the_term() {
        // Selecting generously around a quoted word must not bake the quotes
        // into the Spellbook entry.
        let text = "he said \"Eldrazi\" loudly";
        assert_eq!(
            snapped_text(text, sel(text, "\"Eldrazi\"")),
            Some("Eldrazi")
        );
    }

    #[test]
    fn a_hyphenated_word_snaps_across_both_of_its_tokens() {
        // "hark-stt" is two tokens; a selection touching both must produce the
        // whole hyphenated span, hyphen included, not just one side.
        let text = "run hark-stt now";
        assert_eq!(snapped_text(text, sel(text, "ark-st")), Some("hark-stt"));
    }

    #[test]
    fn multibyte_text_maps_char_indices_to_the_right_bytes() {
        // Char index != byte offset here; getting this wrong slices mid-glyph
        // or panics.
        let text = "call müller café now";
        assert_eq!(snapped_text(text, sel(text, "ülle")), Some("müller"));
        assert_eq!(
            snapped_text(text, sel(text, "ler caf")),
            Some("müller café")
        );
    }

    #[test]
    fn a_full_span_selection_returns_the_whole_phrase() {
        assert_eq!(
            snapped_text(HISTORY, 0..HISTORY.chars().count()),
            Some(HISTORY)
        );
    }

    #[test]
    fn an_out_of_range_selection_is_rejected_not_clamped() {
        // A stale selection against changed text must not silently produce
        // some other word.
        assert_eq!(snap_to_tokens(HISTORY, 0..9_999), None);
    }

    #[test]
    fn the_snapped_range_is_in_characters_not_bytes() {
        // Guards the conversion direction: with multibyte text ahead of the
        // match, a byte range would be larger than the char range.
        let text = "café Eldrazi";
        let snapped = snap_to_tokens(text, sel(text, "ldraz")).expect("snaps");
        assert_eq!(snapped, 5..12);
        assert_eq!(snapped_text(text, sel(text, "ldraz")), Some("Eldrazi"));
    }
}

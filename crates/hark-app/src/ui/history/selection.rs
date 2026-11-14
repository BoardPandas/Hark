//! Which History text a drag can select, and what it selects.
//!
//! A row paints two selectable texts -- the final-text preview and, expanded,
//! the raw transcript -- and the page keeps one live [`Selection`] across the
//! whole list. This module answers the page's questions about it: what span
//! does this row's selection cover, and is that text still on screen.

use crate::ui::selectable::Selection;
use egui::Id;
use hark_store::Entry;

/// The two texts a row shows that a Spellbook term can be picked from.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The final text, as the collapsed row's preview shows it.
    Final,
    /// The raw transcript in the expanded detail: what the provider heard, and
    /// so the better place to find a misheard name.
    Raw,
}

impl Source {
    const ALL: [Source; 2] = [Source::Final, Source::Raw];

    /// The exact string the row paints for this source. Selections are char
    /// indices into it, so the page must snap against this and nothing else.
    pub fn shown(self, entry: &Entry) -> String {
        match self {
            Source::Final => super::row::preview(&entry.final_text),
            Source::Raw => entry.raw_text.trim().to_string(),
        }
    }

    /// Whether the row currently paints this source at all.
    fn visible(self, expanded: bool) -> bool {
        self == Source::Final || expanded
    }
}

/// Selection key for one of a row's texts. Keyed on the entry id, not the
/// widget's positional id: a new dictation shifts entries between rows, and a
/// live selection must not follow the slot onto different text.
pub fn selection_id(entry_id: i64, source: Source) -> Id {
    match source {
        Source::Final => Id::unique(("history-final", entry_id)),
        Source::Raw => Id::unique(("history-raw", entry_id)),
    }
}

/// An invocation row holds canned text the user typed, not words anyone spoke,
/// so there is no misheard span in it to correct.
pub fn offers_selection(entry: &Entry) -> bool {
    entry.invocation.is_none()
}

/// The snapped span `selection` covers in this row, if it owns one that is
/// still on screen. Computed by the page, before the row draws, because the
/// action cluster is laid out above the transcript that owns the selection.
pub fn selected_text(
    entry: &Entry,
    expanded: bool,
    selection: &Option<Selection>,
) -> Option<String> {
    let selection = selection.as_ref()?;
    if !offers_selection(entry) {
        return None;
    }
    Source::ALL
        .into_iter()
        .filter(|source| source.visible(expanded))
        .filter(|source| selection.owned_by(selection_id(entry.id, *source)))
        .find_map(|source| {
            hark_spellbook::snapped_text(&source.shown(entry), selection.range()).map(str::to_owned)
        })
}

/// True when `selection` points at text this row still paints. A collapsed row
/// no longer shows its raw transcript, and a selection left on hidden text
/// would keep the Add button live for something the user cannot see.
pub fn holds_selection(entry: &Entry, expanded: bool, selection: &Selection) -> bool {
    offers_selection(entry)
        && Source::ALL.into_iter().any(|source| {
            source.visible(expanded) && selection.owned_by(selection_id(entry.id, source))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::selectable::{rig::Rig, Click};

    fn heard(raw: &str, final_text: &str) -> Entry {
        Entry {
            raw_text: raw.to_string(),
            final_text: final_text.to_string(),
            id: 1,
            ts_ms: 1_000,
            voice: "clean".to_string(),
            stt_provider: "deepgram".to_string(),
            stt_model: "nova-3".to_string(),
            cleanup_model: None,
            invocation: None,
            stt_ms: 400,
            cleanup_ms: None,
            total_ms: 800,
            delivered: true,
        }
    }

    /// Drag from inside "Al" to inside "Drazi" in `source`'s text with real
    /// pointer events, and hand back the selection the page would then hold.
    fn drag_in(entry: &Entry, source: Source) -> Option<Selection> {
        let text = source.shown(entry);
        let from = text.find("l D").expect("fixture has the phrase");
        let to = text.find("zi ").expect("fixture has the phrase");
        let index = |byte: usize| text[..byte].chars().count();
        let click = match source {
            Source::Final => Click::PassesThrough,
            Source::Raw => Click::SelectsWord,
        };
        let mut rig = Rig::new(selection_id(entry.id, source), &text, click);
        rig.drag(index(from), index(to));
        rig.state
    }

    #[test]
    fn dragging_over_the_raw_transcript_gives_the_row_a_term_to_add() {
        let row = heard(
            "I cast the Al Drazi commander",
            "I cast the Eldrazi commander",
        );
        let selection = drag_in(&row, Source::Raw);
        // This is the string the page hands the row as `RowView::selected`,
        // and the Add button is enabled exactly when it is `Some`.
        assert_eq!(
            selected_text(&row, true, &selection).as_deref(),
            Some("Al Drazi")
        );
        // Collapsed, the raw transcript is not on screen, so neither is its
        // selection: the button must not stay live for text nobody can see.
        assert_eq!(selected_text(&row, false, &selection), None);
        let live = selection.as_ref().expect("a drag made a selection");
        assert!(holds_selection(&row, true, live));
        assert!(!holds_selection(&row, false, live));
    }

    #[test]
    fn the_collapsed_rows_final_text_can_be_selected_too() {
        let row = heard("unused", "I cast the Al Drazi commander");
        let selection = drag_in(&row, Source::Final);
        assert_eq!(
            selected_text(&row, false, &selection).as_deref(),
            Some("Al Drazi")
        );
        assert_eq!(
            selected_text(&row, true, &selection).as_deref(),
            Some("Al Drazi"),
            "the preview stays on screen when the row expands"
        );
    }

    #[test]
    fn a_selection_in_another_row_adds_nothing_here() {
        let row = heard("I cast the Al Drazi commander", "");
        let selection = drag_in(&row, Source::Raw);
        let other = Entry { id: 2, ..row };
        assert_eq!(selected_text(&other, true, &selection), None);
    }

    #[test]
    fn invocation_rows_never_offer_a_term() {
        let said = "I cast the Al Drazi commander";
        let plain = heard(said, said);
        let canned = Entry {
            invocation: Some("access granted".to_string()),
            ..heard(said, said)
        };
        assert!(offers_selection(&plain));
        assert!(!offers_selection(&canned));
        // Even a selection made while the row was still a plain one must not
        // turn into a term.
        let selection = drag_in(&plain, Source::Raw);
        assert_eq!(selected_text(&canned, true, &selection), None);
        let live = selection.as_ref().expect("a drag made a selection");
        assert!(!holds_selection(&canned, true, live));
    }
}

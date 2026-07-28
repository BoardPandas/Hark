//! Selectable transcript text with word snapping (Slice 0 spike).
//!
//! egui can already select label text -- drag, shift+arrow, highlight -- but it
//! will not tell us *what* is selected: `selected_text()` is private and the
//! string is only assembled into a private field when a copy event fires. So we
//! own selection here instead, against the same public galley APIs egui uses
//! internally (`cursor_from_pos` / `pos_from_cursor`).
//!
//! egui's own selection is switched off on these labels on purpose. Running
//! both would give two answers to "what is selected" that agree in testing and
//! diverge on keyboard selection, elided text, and multi-row spans.
//!
//! The live range is snapped to whole spellbook tokens
//! ([`hark_spellbook::snap_to_tokens`]) before it is painted or returned, so
//! what the user sees highlighted is exactly what would become a Spellbook
//! entry -- and it is tokenized by the same code that will later have to match
//! it.

use egui::{self, text::CCursor, Label, Rect, Response, RichText, Sense, Ui};
use std::ops::Range;

/// Which widget owns the current selection, and where the drag began and ended.
/// Anchor and head are character indices and may be in either order; a
/// right-to-left drag is normalized at snap time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    owner: egui::Id,
    anchor: usize,
    head: usize,
}

impl Selection {
    /// True when this selection belongs to the widget with `id`. One selection
    /// exists at a time across the whole page, so every row asks.
    fn owned_by(&self, id: egui::Id) -> bool {
        self.owner == id
    }
}

/// Draw `text` as selectable, snapped-to-word transcript text.
///
/// Returns the snapped character range while this widget owns a selection that
/// covers at least one word. `None` means there is nothing addable selected
/// here -- either the selection is elsewhere, or it covers only whitespace and
/// punctuation.
pub fn selectable_text(
    ui: &mut Ui,
    text: &str,
    rich: RichText,
    state: &mut Option<Selection>,
) -> Option<Range<usize>> {
    // `selectable(false)`: see the module note on not running two selection
    // models. `click_and_drag` so a plain click can select the word under it.
    let (galley_pos, galley, response) = Label::new(rich)
        .selectable(false)
        .sense(Sense::click_and_drag())
        .layout_in_ui(ui);
    let id = response.id;

    let char_at = |response: &Response| {
        response
            .interact_pointer_pos()
            .map(|p| galley.cursor_from_pos(p - galley_pos).index.0)
    };

    if response.drag_started() || response.clicked() {
        if let Some(c) = char_at(&response) {
            *state = Some(Selection {
                owner: id,
                anchor: c,
                head: c,
            });
        }
    } else if response.dragged() {
        if let Some(c) = char_at(&response) {
            if let Some(sel) = state.as_mut().filter(|s| s.owned_by(id)) {
                sel.head = c;
            }
        }
    }

    let snapped = state
        .as_ref()
        .filter(|s| s.owned_by(id))
        .and_then(|s| hark_spellbook::snap_to_tokens(text, s.anchor..s.head));

    // Highlight first, then the text on top of it: `layout_in_ui` allocates and
    // lays out but never paints, so painting order here is ours to choose.
    if ui.is_rect_visible(response.rect) {
        let painter = ui.painter();
        if let Some(range) = &snapped {
            let fill = ui.visuals().selection.bg_fill;
            for rect in selection_rects(&galley, range.clone()) {
                painter.rect_filled(rect.translate(galley_pos.to_vec2()), 2.0, fill);
            }
        }
        painter.galley(galley_pos, galley, ui.visuals().text_color());
    }

    snapped
}

/// One rect per galley row the range covers. Multi-row spans cannot be a single
/// rectangle, and a wrapped transcript is the normal case in a history row, so
/// this walks rows rather than assuming the selection fits on one line.
fn selection_rects(galley: &egui::Galley, range: Range<usize>) -> Vec<Rect> {
    let mut rects = Vec::new();
    let mut row_start = 0usize;
    for row in galley.rows.iter() {
        let row_end = row_start + row.char_count_including_newline().0;
        let from = range.start.max(row_start);
        let to = range.end.min(row_end);
        if from < to {
            let a = galley.pos_from_cursor(CCursor::new(from));
            let b = galley.pos_from_cursor(CCursor::new(to));
            rects.push(Rect::from_min_max(a.min, b.max));
        }
        row_start = row_end;
    }
    rects
}

//! Selectable transcript text with word snapping.
//!
//! egui can already select label text -- drag, shift+arrow, highlight -- but it
//! will not tell us *what* is selected. Re-checked against egui 0.37.0:
//! `selected_text()` is still a private function and `text_to_copy` a private
//! field filled only when a copy event fires. The one new thing,
//! `OutputCommand::TextSelectionSettled(String)`, is queued for the platform
//! integration at the end of the pass, so UI code never sees it; it carries the
//! string but no char range, so a repeated word could not be snapped; and it
//! covers a selection spanning several labels. `LabelSelectionState::
//! has_selection()` still answers only yes or no. So we own selection here
//! instead, against the same public galley APIs egui uses internally
//! (`cursor_from_pos` / `pos_from_cursor`).
//!
//! egui's own selection is switched off on these labels on purpose. Running
//! both would give two answers to "what is selected" that agree in testing and
//! diverge on keyboard selection, elided text, and multi-row spans. The price of
//! owning it is that selection is mouse-only, and that Ctrl+C has to be
//! re-implemented here (below) or copying from a transcript would silently stop
//! working.
//!
//! The live range is snapped to whole spellbook tokens
//! ([`hark_spellbook::snap_to_tokens`]) before it is painted or returned, so
//! what the user sees highlighted is exactly what would become a Spellbook
//! entry -- and it is tokenized by the same code that will later have to match
//! it.

use egui::{self, text::CCursor, Event, Label, Rect, Response, RichText, Sense, Ui};
use std::ops::Range;

/// Which widget owns the current selection, and where the drag began and ended.
///
/// Both are character indices, and the two are **not** interchangeable:
/// `anchor` is where the button went down and `head` is where the pointer is
/// now, so a right-to-left drag has `anchor > head`. `snap_to_tokens` relies on
/// that direction — the anchor is inclusive of the word it lands on, the head
/// is not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    owner: egui::Id,
    anchor: usize,
    head: usize,
}

impl Selection {
    /// True when this selection belongs to `key`. One selection exists at a
    /// time across the whole page, so every row asks.
    pub fn owned_by(&self, key: egui::Id) -> bool {
        self.owner == key
    }

    /// Anchor-to-head, direction preserved: `snap_to_tokens` needs to know
    /// which end the user grabbed.
    pub fn range(&self) -> Range<usize> {
        self.anchor..self.head
    }
}

/// What a plain click (press and release with no drag) does to the text.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Click {
    /// Select the word under the pointer. For text that is only ever read.
    SelectsWord,
    /// Leave it to the caller, which reads the returned response. For text
    /// whose click already means something else (the history preview toggles
    /// its row), so only a drag selects.
    PassesThrough,
}

/// Draw `text` as selectable, snapped-to-word transcript text, and return its
/// response. What is selected is read back with the page's `Selection`, snapped
/// by the same [`hark_spellbook::snapped_text`] that painted it.
pub fn selectable_text(
    ui: &mut Ui,
    key: egui::Id,
    text: &str,
    rich: RichText,
    state: &mut Option<Selection>,
    click: Click,
) -> Response {
    // `selectable(false)`: see the module note on not running two selection
    // models. `click_and_drag` so a drag can select, and a click can be either
    // a word selection or the caller's.
    let (galley_pos, galley, response) = Label::new(rich)
        .selectable(false)
        .sense(Sense::click_and_drag())
        .layout_in_ui(ui);
    let response = response.on_hover_cursor(egui::CursorIcon::Text);
    // Keyed on the caller's stable key, never on `response.id`: widget ids in a
    // list are positional, so row 3 keeps the same id when a new dictation
    // shifts a different entry into that slot -- and the live selection would
    // silently start describing text it was never made against.
    let id = key;
    let before = state.clone();

    let char_at = |pos| galley.cursor_from_pos(pos - galley_pos).index.0;

    let starts = response.drag_started() || (click == Click::SelectsWord && response.clicked());
    if starts {
        // The anchor comes from `press_origin`, NOT `interact_pointer_pos`:
        // the latter is the pointer's *current* position, and `drag_started`
        // only fires once egui's drag threshold has been crossed, so by then
        // the pointer has already travelled several pixels in the drag
        // direction. Anchoring there starts the selection past the character
        // the user actually pressed on, which is why the first word of a drag
        // used to fall out of the highlight.
        let press = ui.input(|i| i.pointer.press_origin());
        if let Some(c) = press.or(response.interact_pointer_pos()).map(char_at) {
            *state = Some(Selection {
                owner: id,
                anchor: c,
                head: c,
            });
        }
    } else if response.dragged() {
        // The head is the live position, which is exactly what's wanted here.
        if let Some(c) = response.interact_pointer_pos().map(char_at) {
            if let Some(sel) = state.as_mut().filter(|s| s.owned_by(id)) {
                sel.head = c;
            }
        }
    } else if click == Click::PassesThrough
        && response.clicked()
        && state.as_ref().is_some_and(|s| s.owned_by(id))
    {
        // The click went to the caller; a highlight left behind would read as
        // still pending after the thing it was for has moved on.
        *state = None;
    }

    let live = state
        .as_ref()
        .filter(|s| s.owned_by(id))
        .map(Selection::range);
    let snapped = live
        .clone()
        .and_then(|range| hark_spellbook::snap_to_tokens(text, range));

    // Ctrl+C copies what is highlighted, which is the snapped span. Without
    // this, switching egui's selection off would have taken copying away from
    // text that could be copied before. Skipped while a text field has the
    // keyboard, where Ctrl+C belongs to the field.
    if let Some(range) = live {
        let copy = !ui.ctx().egui_wants_keyboard_input()
            && ui.input(|i| i.events.iter().any(|e| matches!(e, Event::Copy)));
        if let (true, Some(span)) = (copy, hark_spellbook::snapped_text(text, range)) {
            ui.ctx().copy_text(span.to_owned());
        }
    }

    // The Add button sits above this text and reads the selection from the
    // previous pass; one more pass makes it catch up after the last move
    // instead of waiting for the next input event.
    if *state != before {
        ui.ctx().request_repaint();
    }

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

    response
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

/// A headless rig that drives real pointer events over one selectable label,
/// so tests exercise the same galley hit-testing a mouse does. Shared with the
/// History row's tests.
#[cfg(test)]
pub(crate) mod rig {
    use super::*;
    use egui::{Color32, Context, Modifiers, OutputCommand, PointerButton, Pos2, RawInput, Vec2};
    use std::cell::{Cell, RefCell};

    pub struct Rig {
        ctx: Context,
        key: egui::Id,
        text: String,
        click: Click,
        pub state: Option<Selection>,
        /// Left edge of each char boundary 0..=len, measured in the last frame.
        xs: RefCell<Vec<f32>>,
        origin: Cell<Pos2>,
        /// Whether the label reported a click on the last frame.
        pub clicked: bool,
    }

    impl Rig {
        pub fn new(key: egui::Id, text: &str, click: Click) -> Self {
            let ctx = Context::default();
            crate::theme::apply(&ctx);
            let mut rig = Rig {
                ctx,
                key,
                text: text.to_owned(),
                click,
                state: None,
                xs: RefCell::new(Vec::new()),
                origin: Cell::new(Pos2::ZERO),
                clicked: false,
            };
            // Fonts installed by `theme::apply` only take effect on the pass
            // after, so measure on the second.
            rig.frame(vec![]);
            rig.frame(vec![]);
            rig
        }

        /// Run one frame; returns what the frame asked the OS to copy.
        pub fn frame(&mut self, events: Vec<Event>) -> Vec<String> {
            let mut clicked = false;
            let mut output = self.ctx.run_ui(
                RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 700.0))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    self.origin.set(ui.next_widget_position());
                    let font = egui::TextStyle::Body.resolve(ui.style());
                    let galley =
                        ui.painter()
                            .layout_no_wrap(self.text.clone(), font, Color32::WHITE);
                    *self.xs.borrow_mut() = (0..=self.text.chars().count())
                        .map(|n| galley.pos_from_cursor(CCursor::new(n)).min.x)
                        .collect();
                    clicked = selectable_text(
                        ui,
                        self.key,
                        &self.text,
                        RichText::new(&self.text),
                        &mut self.state,
                        self.click,
                    )
                    .clicked();
                },
            );
            self.clicked = clicked;
            // egui panics on a dropped, unapplied texture delta; there is no
            // renderer here to apply it.
            output.textures_delta.clear();
            output
                .platform_output
                .commands
                .into_iter()
                .filter_map(|c| match c {
                    OutputCommand::CopyText(text) => Some(text),
                    _ => None,
                })
                .collect()
        }

        /// The middle of character `n`, where a user would aim.
        pub fn at(&self, n: usize) -> Pos2 {
            let xs = self.xs.borrow();
            Pos2::new(
                self.origin.get().x + (xs[n] + xs[n + 1]) / 2.0,
                self.origin.get().y + 6.0,
            )
        }

        fn button(pos: Pos2, pressed: bool) -> Event {
            Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            }
        }

        /// Press on character `from`, drag to `to`, release.
        pub fn drag(&mut self, from: usize, to: usize) {
            let (a, b) = (self.at(from), self.at(to));
            self.frame(vec![Event::PointerMoved(a)]);
            self.frame(vec![Self::button(a, true)]);
            self.frame(vec![Event::PointerMoved(b)]);
            // egui only starts a drag once the pointer has travelled; one more
            // move is the frame where the head follows it.
            self.frame(vec![Event::PointerMoved(b)]);
            self.frame(vec![Self::button(b, false)]);
        }

        pub fn click(&mut self, at: usize) {
            let p = self.at(at);
            self.frame(vec![Event::PointerMoved(p)]);
            self.frame(vec![Self::button(p, true)]);
            self.frame(vec![Self::button(p, false)]);
        }

        pub fn selected(&self) -> Option<&str> {
            let range = self.state.as_ref()?.range();
            hark_spellbook::snapped_text(&self.text, range)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::rig::Rig;
    use super::*;

    const TEXT: &str = "I cast the Al Drazi commander";

    fn index_of(needle: &str) -> usize {
        TEXT[..TEXT.find(needle).expect("present")].chars().count()
    }

    fn rig(click: Click) -> Rig {
        Rig::new(egui::Id::unique("test-text"), TEXT, click)
    }

    #[test]
    fn dragging_between_two_half_words_selects_the_whole_phrase() {
        let mut rig = rig(Click::SelectsWord);
        // Press inside "Al" and release inside "Drazi": both ends are partial.
        rig.drag(index_of("l D"), index_of("zi c"));
        assert_eq!(rig.selected(), Some("Al Drazi"));
    }

    #[test]
    fn a_right_to_left_drag_selects_the_same_phrase() {
        let mut rig = rig(Click::SelectsWord);
        rig.drag(index_of("zi c"), index_of("l D"));
        assert_eq!(rig.selected(), Some("Al Drazi"));
    }

    #[test]
    fn a_click_selects_the_word_under_it_when_asked_to() {
        let mut rig = rig(Click::SelectsWord);
        rig.click(index_of("raz"));
        assert_eq!(rig.selected(), Some("Drazi"));
    }

    #[test]
    fn a_click_is_left_to_the_caller_when_it_has_another_meaning() {
        let mut rig = rig(Click::PassesThrough);
        rig.click(index_of("raz"));
        assert!(rig.clicked, "the caller must see the click");
        assert_eq!(rig.selected(), None, "a click alone must not select");

        // A drag on the same text still selects.
        rig.drag(index_of("l D"), index_of("zi c"));
        assert_eq!(rig.selected(), Some("Al Drazi"));
        // Clicking afterwards hands the click over and retires the highlight.
        rig.click(index_of("cast"));
        assert!(rig.clicked);
        assert_eq!(rig.selected(), None);
    }

    #[test]
    fn copy_puts_the_highlighted_span_on_the_clipboard() {
        let mut rig = rig(Click::SelectsWord);
        assert!(rig.frame(vec![Event::Copy]).is_empty(), "nothing selected");
        rig.drag(index_of("l D"), index_of("zi c"));
        assert_eq!(rig.frame(vec![Event::Copy]), ["Al Drazi"]);
    }
}

//! The book view (spec §5.2): an optional second renderer for the Spellbook
//! and Invocations pages, beside their lists. Entries are packed onto facing
//! pages and turned with a row of controls or PageUp/PageDown/Home/End.
//!
//! Each page owns its entries; this module owns only the frame, the page
//! turning, and position. Position is an anchor entry index (see `paginate`),
//! so a reflow never loses the reader's place.
//!
//! Pagination uses each entry's *painted* height once it has been painted,
//! and the page's own estimate before that. When the two disagree the frame
//! is discarded and laid out again, so what is measured is what is painted
//! even for an entry whose editor is open.

mod paginate;

pub use paginate::clamp_anchor;

use crate::theme;
use egui::{
    Align, Context, Direction, Galley, Id, Key, Layout, Modifiers, Rect, RichText, Sense,
    TextStyle, Ui, UiBuilder, Vec2,
};
use paginate::{paginate, spread_count, spread_of};
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BookView {
    List,
    Book,
}

impl BookView {
    fn key(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Book => "book",
        }
    }
}

/// The page's saved view: List unless the user chose Book. An unknown saved
/// value (a newer build's) also reads as List, so nothing changes for anyone
/// who never opens the book.
pub fn view(ctx: &Context, id: Id) -> BookView {
    match ctx.data_mut(|d| d.get_persisted::<String>(id)).as_deref() {
        Some("book") => BookView::Book,
        _ => BookView::List,
    }
}

fn set_view(ctx: &Context, id: Id, value: BookView) {
    ctx.data_mut(|d| d.insert_persisted(id, value.key().to_owned()));
}

/// The `List | Book` control. Returns the view to draw this frame. The
/// selection shows as a fill, a highlight and `selected(true)`, never by
/// color alone (`theme::nav_button`), and each option has a focus ring.
pub fn view_toggle(ui: &mut Ui, id: Id) -> BookView {
    let mut current = view(ui.ctx(), id);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (value, glyph, label) in [
            (BookView::List, theme::icons::LIST_BULLETS, "List"),
            (BookView::Book, theme::icons::BOOK_OPEN, "Book"),
        ] {
            let job = theme::icon_label_job(ui.style(), glyph, label);
            if theme::nav_button(ui, job, current == value).clicked() && current != value {
                current = value;
                set_view(ui.ctx(), id, value);
            }
        }
    });
    current
}

/// What a page puts in the book.
pub trait Entries {
    fn len(&self) -> usize;
    /// Height of entry `index` laid out `width` wide: a first guess, used
    /// until the entry has been painted once at this width.
    fn estimate(&mut self, ui: &Ui, index: usize, width: f32) -> f32;
    /// Draw entry `index` top-down, `ui.available_width()` wide, with
    /// vertical item spacing at zero (the entry spaces its own rows).
    fn paint(&mut self, ui: &mut Ui, index: usize);
    /// The opening page of an empty book.
    fn empty(&mut self, ui: &mut Ui);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Turn {
    Previous,
    Next,
    First,
    Last,
}

/// Position, plus the heights entries were last painted at.
#[derive(Default)]
pub struct Book {
    /// The entry the open spread must show. Pages set it to an entry being
    /// edited or just added; turning a page sets it to that spread's first.
    pub anchor: usize,
    painted: HashMap<usize, f32>,
    /// (entry count, page width) the painted heights belong to.
    painted_at: (usize, f32),
    /// The page-turn row's text, kept for the disabled row of `show_flat`.
    pages_label: String,
}

impl Book {
    pub fn show(&mut self, ui: &mut Ui, head: &str, entries: &mut impl Entries) {
        // Before any ScrollArea exists, or an inner one may take the key.
        let turn = page_keys(ui);
        self.with_height(ui, |book, ui, height| {
            book.spread(ui, head, entries, height, turn)
        });
    }

    /// The book lying flat: one writing surface across the whole spread, for
    /// an editor that must not be split across pages. The page-turn row stays
    /// in place, disabled, so nothing shifts when the editor closes.
    pub fn show_flat(&mut self, ui: &mut Ui, head: &str, body: impl FnOnce(&mut Ui)) {
        self.with_height(ui, |book, ui, height| {
            let width = ui.available_width();
            let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
            ui.add_space(theme::GAP);
            let (row, _) =
                ui.allocate_exact_size(Vec2::new(width, theme::CONTROL_HEIGHT), Sense::hover());
            turn_row(ui, row, &book.pages_label, false, false);
            let inner = page_frame(ui, rect, head, None);
            ui.scope_builder(
                UiBuilder::new()
                    .id_salt("book-flat")
                    .max_rect(inner)
                    .layout(Layout::top_down(Align::Min)),
                body,
            );
        });
    }

    /// The book fills the rest of the page down to `BOOK_MIN_HEIGHT`; in a
    /// shorter window it keeps that height and the page around it scrolls.
    fn with_height(&mut self, ui: &mut Ui, draw: impl FnOnce(&mut Self, &mut Ui, f32)) {
        let chrome = theme::GAP + theme::CONTROL_HEIGHT;
        let fill = ui.available_height() - chrome;
        if fill >= theme::BOOK_MIN_HEIGHT {
            draw(self, ui, fill);
        } else {
            egui::ScrollArea::vertical()
                .id_salt("book-outer")
                .auto_shrink([false, false])
                .show(ui, |ui| draw(self, ui, theme::BOOK_MIN_HEIGHT));
        }
    }

    fn spread(
        &mut self,
        ui: &mut Ui,
        head: &str,
        entries: &mut impl Entries,
        height: f32,
        keys: Option<Turn>,
    ) {
        let width = ui.available_width();
        let per = if width >= theme::BOOK_SPREAD_MIN_WIDTH {
            2
        } else {
            1
        };
        let page_width = ((width - theme::BOOK_GUTTER * (per - 1) as f32) / per as f32).floor();
        let (body_width, capacity) = body_size(ui, page_width, height);

        let len = entries.len();
        if self.painted_at != (len, body_width) {
            self.painted.clear();
            self.painted_at = (len, body_width);
        }
        let heights: Vec<f32> = (0..len)
            .map(|i| match self.painted.get(&i) {
                Some(&painted) => painted,
                None => entries.estimate(ui, i, body_width),
            })
            .collect();
        let pages = paginate(&heights, capacity, theme::BOOK_ENTRY_GAP);
        self.anchor = clamp_anchor(self.anchor, len);
        let spreads = spread_count(&pages, per);
        let mut spread = spread_of(self.anchor, &pages, per);

        let (book_rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
        ui.add_space(theme::GAP);
        let (row, _) =
            ui.allocate_exact_size(Vec2::new(width, theme::CONTROL_HEIGHT), Sense::hover());
        // The row is built before the pages so Tab reaches it first.
        self.pages_label = pages_label(spread, per, pages.len());
        let clicked = turn_row(ui, row, &self.pages_label, spread > 0, spread + 1 < spreads);
        if let Some(turn) = keys.or(clicked) {
            let to = match turn {
                Turn::Previous => spread.saturating_sub(1),
                Turn::Next => (spread + 1).min(spreads - 1),
                Turn::First => 0,
                Turn::Last => spreads - 1,
            };
            if to != spread {
                spread = to;
                self.anchor = pages[spread * per].start;
                // The row above already says the old page numbers; lay the
                // frame out again rather than show them for a frame.
                ui.ctx().request_discard("book page turn");
            }
        }

        let first = spread * per;
        for (slot, number) in (first..(first + per).min(pages.len())).enumerate() {
            let left = book_rect.left() + slot as f32 * (page_width + theme::BOOK_GUTTER);
            let rect = Rect::from_min_size(
                egui::pos2(left, book_rect.top()),
                Vec2::new(page_width, height),
            );
            let inner = page_frame(ui, rect, head, Some(number + 1));
            self.page_body(ui, inner, number, pages[number].clone(), &heights, entries);
        }
        if per == 2 {
            let spine = Rect::from_center_size(
                egui::pos2(
                    book_rect.left() + page_width + theme::BOOK_GUTTER / 2.0,
                    book_rect.center().y,
                ),
                Vec2::new(2.0, height - 2.0 * theme::BOOK_PAGE_PADDING),
            );
            ui.painter()
                .rect_filled(spine, 1.0, theme::book_gutter(ui.visuals()));
        }
    }

    fn page_body(
        &mut self,
        ui: &mut Ui,
        inner: Rect,
        number: usize,
        range: Range<usize>,
        heights: &[f32],
        entries: &mut impl Entries,
    ) {
        let builder = UiBuilder::new()
            .id_salt(("book-page", number))
            .max_rect(inner)
            .layout(Layout::top_down(Align::Min));
        ui.scope_builder(builder, |ui| {
            // Every page body scrolls, so an entry taller than a page (it got
            // a page to itself) or a misjudged estimate never clips. Salted
            // per page: LL-G `egui-scrollarea-shared-id-salt`.
            egui::ScrollArea::vertical()
                .id_salt(format!("book-page-{number}"))
                .auto_shrink([false, false])
                .max_height(inner.height())
                .show(ui, |ui| {
                    ui.set_width(inner.width());
                    ui.spacing_mut().item_spacing.y = 0.0;
                    if range.is_empty() {
                        entries.empty(ui);
                        return;
                    }
                    for index in range.clone() {
                        if index > range.start {
                            let (gap, _) = ui.allocate_exact_size(
                                Vec2::new(inner.width(), theme::BOOK_ENTRY_GAP),
                                Sense::hover(),
                            );
                            theme::paint_ornament_rule(
                                ui,
                                gap.shrink2(Vec2::new(gap.width() / 4.0, 0.0)),
                            );
                        }
                        let painted = ui.push_id(index, |ui| entries.paint(ui, index)).response;
                        let height = painted.rect.height();
                        if (height - heights[index]).abs() > 0.5 {
                            self.painted.insert(index, height);
                            ui.ctx().request_discard("book entry height changed");
                        }
                    }
                });
        });
    }
}

/// PageDown/PageUp turn one spread, Home/End go to the ends. Only while no
/// text field has focus: a focused button must not block them (clicking
/// Next focuses it), which is why this asks `text_edit_focused` rather than
/// `egui_wants_keyboard_input`. Arrow keys stay with egui focus navigation.
fn page_keys(ui: &Ui) -> Option<Turn> {
    if ui.ctx().text_edit_focused() {
        return None;
    }
    ui.input_mut(|i| {
        [
            (Key::PageDown, Turn::Next),
            (Key::PageUp, Turn::Previous),
            (Key::Home, Turn::First),
            (Key::End, Turn::Last),
        ]
        .into_iter()
        .find(|(key, _)| i.consume_key(Modifiers::NONE, *key))
        .map(|(_, turn)| turn)
    })
}

/// The page body's width and the height entries may fill: the page minus its
/// padding, the running head and the folio.
fn body_size(ui: &Ui, page_width: f32, height: f32) -> (f32, f32) {
    let pad = theme::BOOK_PAGE_PADDING;
    let line = ui.text_style_height(&TextStyle::Small);
    (
        page_width - 2.0 * pad,
        height - 2.0 * pad - 2.0 * (line + theme::GAP),
    )
}

/// Paints one page with its running head and, when numbered, its folio, and
/// returns the body rect between them.
fn page_frame(ui: &mut Ui, rect: Rect, head: &str, folio: Option<usize>) -> Rect {
    theme::paint_book_page(ui, rect);
    let inner = rect.shrink(theme::BOOK_PAGE_PADDING);
    let line = ui.text_style_height(&TextStyle::Small);
    let head_rect = Rect::from_min_size(inner.min, Vec2::new(inner.width(), line));
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(head_rect)
            .layout(Layout::left_to_right(Align::Center)),
        |ui| ui.label(RichText::new(head).small().weak()),
    );
    if let Some(number) = folio {
        let folio_rect =
            Rect::from_min_max(egui::pos2(inner.left(), inner.bottom() - line), inner.max);
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(folio_rect)
                .layout(Layout::centered_and_justified(Direction::LeftToRight)),
            |ui| {
                ui.label(
                    RichText::new(format!("\u{2014} {number} \u{2014}"))
                        .small()
                        .weak(),
                )
            },
        );
    }
    Rect::from_min_max(
        egui::pos2(inner.left(), head_rect.bottom() + theme::GAP),
        egui::pos2(inner.right(), inner.bottom() - line - theme::GAP),
    )
}

/// "Pages 3–4 of 12", or "Page 3 of 12" when the spread shows one page.
fn pages_label(spread: usize, per: usize, total: usize) -> String {
    let first = spread * per + 1;
    let last = (first + per - 1).min(total);
    if first == last {
        format!("Page {first} of {total}")
    } else {
        format!("Pages {first}\u{2013}{last} of {total}")
    }
}

/// `‹ Previous`, the page numbers, `Next ›`. At an end the button is
/// disabled, never hidden, so the row does not shift.
fn turn_row(ui: &mut Ui, rect: Rect, label: &str, back: bool, forward: bool) -> Option<Turn> {
    let mut turn = None;
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            let job = theme::icon_label_job(ui.style(), theme::icons::CARET_LEFT, "Previous");
            let previous = ui.add_enabled(back, egui::Button::new(job));
            theme::focus_ring(ui, &previous);
            if previous.clicked() {
                turn = Some(Turn::Previous);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let job = theme::label_icon_job(ui.style(), "Next", theme::icons::CARET_RIGHT);
                let next = ui.add_enabled(forward, egui::Button::new(job));
                theme::focus_ring(ui, &next);
                if next.clicked() {
                    turn = Some(Turn::Next);
                }
                ui.with_layout(
                    Layout::centered_and_justified(Direction::LeftToRight),
                    |ui| ui.label(RichText::new(label).small().weak()),
                );
            });
        },
    );
    turn
}

/// An entry heading: its first letter as a drop cap in the page's hue, the
/// rest in the serif role, bottom-aligned on the first line.
pub struct Heading {
    cap: Arc<Galley>,
    rest: Arc<Galley>,
    /// How far the rest sits below the top of the cap.
    offset: f32,
    pub height: f32,
}

/// Laid out the same way for `estimate` and `paint`, so the measured galleys
/// are the painted ones (egui's galley cache makes the second call free).
pub fn heading(ui: &Ui, text: &str, hue: theme::Hue, width: f32) -> Heading {
    let v = ui.visuals();
    let mut chars = text.chars();
    // A drop cap is a capital even when the phrase is not ("access granted");
    // only the display changes, and `paint_heading` reports the real text.
    let first: String = chars
        .next()
        .map(char::to_uppercase)
        .into_iter()
        .flatten()
        .collect();
    let painter = ui.painter();
    let cap = painter.layout_no_wrap(first, theme::drop_cap_font(), theme::hue(v, hue));
    let font = theme::book_heading_font();
    let line = painter.fonts_mut(|f| f.row_height(&font));
    let rest = painter.layout(
        chars.as_str().to_owned(),
        font,
        v.strong_text_color(),
        (width - cap.size().x - 2.0).max(1.0),
    );
    let offset = (cap.size().y - line).max(0.0);
    Heading {
        height: cap.size().y.max(offset + rest.size().y),
        cap,
        rest,
        offset,
    }
}

pub fn paint_heading(ui: &mut Ui, text: &str, hue: theme::Hue, width: f32) {
    let heading = heading(ui, text, hue, width);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, heading.height), Sense::hover());
    // Painted as two galleys, so tell assistive tech the whole heading.
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text));
    let ink = ui.visuals().strong_text_color();
    let cap_width = heading.cap.size().x;
    ui.painter().galley(rect.min, heading.cap, ink);
    ui.painter().galley(
        rect.min + Vec2::new(cap_width + 2.0, heading.offset),
        heading.rest,
        ink,
    );
}

#[cfg(test)]
mod tests;

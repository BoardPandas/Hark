//! The Spellbook in the book view (spec §5.3): each term opens with a drop
//! cap, then "Also heard as" with its aliases, then Edit and Delete. Editing
//! happens where the entry sits, through the same `edit_term` and
//! `alias_editor` the list uses, so the two views cannot disagree.

use super::*;
use crate::ui::book::Entries;
use egui::text::{LayoutJob, TextFormat};
use egui::{Galley, Sense, Vec2};
use std::sync::Arc;

pub(super) struct SpellbookEntries<'a> {
    pub page: &'a mut SpellbookPage,
    pub entries: &'a mut Vec<SpellbookEntry>,
    pub changed: bool,
    /// Applied after the book is drawn, as the list does, so indices cannot
    /// shift under the entries still being painted this frame.
    pub delete: Option<usize>,
}

impl SpellbookEntries<'_> {
    fn is_open(&self, index: usize) -> bool {
        self.page.edit.as_ref().is_some_and(|(i, _)| *i == index)
            || self.page.expanded == Some(index)
    }

    fn is_new(&self, index: usize) -> bool {
        self.page.last_added.as_deref() == Some(self.entries[index].term.as_str())
    }
}

impl SpellbookPage {
    /// The book view. An open entry pins the spread, so it cannot turn out
    /// from under the cursor.
    pub(super) fn book_view(&mut self, ui: &mut Ui, entries: &mut Vec<SpellbookEntry>) -> bool {
        let mut book = std::mem::take(&mut self.book);
        if let Some(open) = self.edit.as_ref().map(|(i, _)| *i).or(self.expanded) {
            book.anchor = open;
        }
        let mut pages = SpellbookEntries {
            page: self,
            entries,
            changed: false,
            delete: None,
        };
        book.show(ui, "Spellbook", &mut pages);
        let (mut changed, delete) = (pages.changed, pages.delete);
        self.book = book;
        if let Some(index) = delete {
            entries.remove(index);
            self.edit = None;
            self.expanded = None;
            changed = true;
        }
        changed
    }
}

impl Entries for SpellbookEntries<'_> {
    fn len(&self) -> usize {
        self.entries.len()
    }

    fn estimate(&mut self, ui: &Ui, index: usize, width: f32) -> f32 {
        let entry = &self.entries[index];
        let mut height = if self.page.edit.as_ref().is_some_and(|(i, _)| *i == index) {
            theme::CONTROL_HEIGHT
        } else {
            heading(ui, &entry.term, width).height
        };
        if self.is_new(index) {
            height += theme::GAP + ui.text_style_height(&egui::TextStyle::Small) + 4.0;
        }
        if !entry.aliases.is_empty() {
            height += theme::GAP + aliases_line(ui, &entry.aliases, width).size().y;
        }
        if self.page.expanded == Some(index) {
            // One row per alias plus the add row; the painted height corrects
            // this guess on the next pass.
            height += (entry.aliases.len() + 1) as f32 * (theme::CONTROL_HEIGHT + theme::GAP);
        }
        height + theme::GAP + theme::CONTROL_HEIGHT
    }

    fn paint(&mut self, ui: &mut Ui, index: usize) {
        ui.spacing_mut().item_spacing.y = theme::GAP;
        let width = ui.available_width();
        if self.page.edit.as_ref().is_some_and(|(i, _)| *i == index) {
            self.changed |= self.page.edit_term(ui, self.entries, width);
        } else {
            paint_heading(ui, &self.entries[index].term, width);
        }
        if self.is_new(index) {
            theme::chip(ui, theme::Hue::Violet, None, "New");
        }
        if index < self.entries.len() && !self.entries[index].aliases.is_empty() {
            let galley = aliases_line(ui, &self.entries[index].aliases, width);
            ui.add(egui::Label::new(galley).selectable(false));
        }
        if self.page.expanded == Some(index) {
            self.changed |= self.page.alias_editor(ui, self.entries, index);
        }
        let open = self.is_open(index);
        ui.horizontal(|ui| {
            let (glyph, label) = if open {
                (theme::icons::CHECK, "Done")
            } else {
                (theme::icons::PENCIL_SIMPLE, "Edit")
            };
            let edit = ui.button(theme::icon_label_job(ui.style(), glyph, label));
            theme::focus_ring(ui, &edit);
            if edit.clicked() {
                if open {
                    // Done commits a pending term edit the way leaving the
                    // field would, then closes the entry.
                    if let Some((i, buffer)) = self.page.edit.take() {
                        self.changed |= commit_edit(self.entries, i, &buffer);
                    }
                    self.page.expanded = None;
                } else {
                    self.page.edit = Some((index, self.entries[index].term.clone()));
                    self.page.edit_needs_focus = true;
                    self.page.expanded = Some(index);
                    self.page.new_alias.clear();
                }
            }
            let delete = ui.button(theme::icon_label_job(
                ui.style(),
                theme::icons::TRASH,
                "Delete",
            ));
            theme::focus_ring(ui, &delete);
            if delete.clicked() {
                self.delete = Some(index);
            }
        });
    }

    fn empty(&mut self, ui: &mut Ui) {
        widgets::empty_state(
            ui,
            theme::icons::BOOK_OPEN,
            "This spellbook has no spells yet.",
            "Add names and terms your provider keeps missing.",
        );
    }
}

/// A term's heading: its first letter as a drop cap in the Spellbook's hue,
/// the rest in the serif role, both bottom-aligned on the first line.
struct Heading {
    cap: Arc<Galley>,
    rest: Arc<Galley>,
    /// How far the rest sits below the top of the cap.
    offset: f32,
    height: f32,
}

/// Laid out the same way for `estimate` and `paint`, so the measured galleys
/// are the painted ones (egui's galley cache makes the second call free).
fn heading(ui: &Ui, term: &str, width: f32) -> Heading {
    let v = ui.visuals();
    let mut chars = term.chars();
    let first = chars.next().map(String::from).unwrap_or_default();
    let painter = ui.painter();
    let cap = painter.layout_no_wrap(
        first,
        theme::drop_cap_font(),
        theme::hue(v, theme::Hue::Violet),
    );
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

fn paint_heading(ui: &mut Ui, term: &str, width: f32) {
    let heading = heading(ui, term, width);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, heading.height), Sense::hover());
    // Painted as two galleys, so tell assistive tech the whole term.
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, term));
    let text = ui.visuals().strong_text_color();
    let cap_width = heading.cap.size().x;
    ui.painter().galley(rect.min, heading.cap, text);
    ui.painter().galley(
        rect.min + Vec2::new(cap_width + 2.0, heading.offset),
        heading.rest,
        text,
    );
}

/// "Also heard as: a, b", the aliases in monospace as in the list.
fn aliases_line(ui: &Ui, aliases: &[String], width: f32) -> Arc<Galley> {
    let small = egui::TextStyle::Small.resolve(ui.style());
    let weak = ui.visuals().weak_text_color();
    let mut job = LayoutJob::default();
    job.wrap.max_width = width;
    job.append(
        "Also heard as: ",
        0.0,
        TextFormat::simple(small.clone(), weak),
    );
    let mono = egui::FontId::new(small.size, egui::FontFamily::Monospace);
    for (i, alias) in aliases.iter().enumerate() {
        if i > 0 {
            job.append(", ", 0.0, TextFormat::simple(small.clone(), weak));
        }
        job.append(alias, 0.0, TextFormat::simple(mono.clone(), weak));
    }
    ui.painter().layout_job(job)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(term: &str, aliases: &[&str]) -> SpellbookEntry {
        SpellbookEntry {
            term: term.to_owned(),
            aliases: aliases.iter().map(|a| a.to_string()).collect(),
        }
    }

    /// Spec AC13: adding from History while the book is open lands on the
    /// spread holding the new term, whether it was appended or merged.
    #[test]
    fn a_history_handoff_add_anchors_the_book_on_the_new_entry() {
        let mut page = SpellbookPage::new();
        let mut entries: Vec<SpellbookEntry> =
            (0..30).map(|i| entry(&format!("Term{i}"), &[])).collect();
        page.prime_add("al drazi".to_owned());
        page.add = "Eldrazi".to_owned();
        assert!(page.submit_add(&mut entries));
        assert_eq!(page.book.anchor, 30);
        assert_eq!(entries[30].aliases, vec!["al drazi".to_owned()]);

        // The same term again only merges an alias into entry 30.
        page.book.anchor = 0;
        page.prime_add("el drazzy".to_owned());
        page.add = "Eldrazi".to_owned();
        assert!(page.submit_add(&mut entries));
        assert_eq!(page.book.anchor, 30);
    }

    /// The painted page carries the drop cap in the Spellbook's hue, the
    /// aliases line, and the New chip on the entry just added.
    #[test]
    fn the_book_paints_drop_caps_aliases_and_the_new_chip() {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        ctx.data_mut(|d| d.insert_persisted(view_id(), "book".to_owned()));
        let mut page = SpellbookPage::new();
        let mut entries = vec![entry("Eldrazi", &["al drazi"]), entry("Kozilek", &[])];
        page.last_added = Some("Kozilek".to_owned());
        let mut violet = egui::Color32::TRANSPARENT;
        let mut texts = Vec::new();
        for _ in 0..2 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(1000.0, 700.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    violet = theme::hue(ui.visuals(), theme::Hue::Violet);
                    assert!(!page.show(ui, &mut entries));
                },
            );
            output.textures_delta.clear();
            texts = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(t) => Some((
                        t.galley.text().to_owned(),
                        t.galley.job.sections.first().map(|s| s.format.color),
                    )),
                    _ => None,
                })
                .collect();
        }
        assert!(texts.iter().any(|(t, c)| t == "E" && *c == Some(violet)));
        assert!(texts.iter().any(|(t, _)| t.starts_with("Also heard as: ")));
        assert!(texts.iter().any(|(t, _)| t == "New"));
        assert!(texts.iter().any(|(t, _)| t.contains("Page 1 of 1")));
    }
}

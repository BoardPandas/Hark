//! Invocations in the book view (spec §5.4): each phrase opens with a drop
//! cap, then its scope, its alternates, the text it types written out in
//! full, any reason it cannot fire, and Edit and Delete. Editing lays the
//! book flat: the open `Draft` spans the whole spread, and its Save, Cancel
//! and Delete go through the same `apply_outcome` as the list.

use super::*;
use crate::ui::book::{self as pages, Entries};
use egui::TextStyle;

struct InvocationEntries<'a> {
    entries: &'a [Invocation],
    /// Applied after the book is drawn, as the list does.
    edit: Option<usize>,
    delete: Option<usize>,
}

impl InvocationsPage {
    pub(super) fn book_view(&mut self, ui: &mut Ui, invocations: &mut Invocations) -> bool {
        let mut book = std::mem::take(&mut self.book);
        if let Some(draft) = &mut self.draft {
            // Explicit Save only, never on lost focus: each commit restarts
            // the pipeline (editor.rs). The book adds no other commit path.
            let mut outcome = Outcome::Open;
            book.show_flat(ui, "Invocations", |ui| {
                outcome = draft.show(ui, &invocations.entries);
            });
            self.book = book;
            return self.apply_outcome(&mut invocations.entries, outcome);
        }
        let mut pages = InvocationEntries {
            entries: &invocations.entries,
            edit: None,
            delete: None,
        };
        book.show(ui, "Invocations", &mut pages);
        let (edit, delete) = (pages.edit, pages.delete);
        self.book = book;
        if let Some(index) = edit {
            self.draft = Some(Draft::existing(index, &invocations.entries[index]));
            self.book.anchor = index;
        }
        if let Some(index) = delete {
            invocations.entries.remove(index);
            self.book.anchor = pages::clamp_anchor(index, invocations.entries.len());
            return true;
        }
        false
    }
}

impl Entries for InvocationEntries<'_> {
    fn len(&self) -> usize {
        self.entries.len()
    }

    fn estimate(&mut self, ui: &Ui, index: usize, width: f32) -> f32 {
        let entry = &self.entries[index];
        let small = ui.text_style_height(&TextStyle::Small);
        let mut height = pages::heading(ui, &entry.phrase, theme::Hue::Rose, width).height;
        height += theme::GAP + small + 4.0;
        if !entry.aliases.is_empty() {
            height += theme::GAP + small;
        }
        let body = ui.painter().layout(
            entry.expansion.clone(),
            TextStyle::Body.resolve(ui.style()),
            ui.visuals().text_color(),
            width,
        );
        height += theme::GAP + body.size().y;
        if skip_reason(self.entries, index).is_some() {
            height += theme::GAP + small;
        }
        height + theme::GAP + theme::CONTROL_HEIGHT
    }

    fn paint(&mut self, ui: &mut Ui, index: usize) {
        ui.spacing_mut().item_spacing.y = theme::GAP;
        let entry = &self.entries[index];
        let width = ui.available_width();
        pages::paint_heading(ui, &entry.phrase, theme::Hue::Rose, width);
        theme::chip(ui, theme::Hue::Rose, None, entry.scope.label());
        if !entry.aliases.is_empty() {
            ui.label(
                RichText::new(format!("Also: {}", entry.aliases.join(", ")))
                    .small()
                    .weak(),
            );
        }
        // Written out in full, line breaks kept: the book is where the text
        // an invocation types can be read, not just previewed.
        ui.add(egui::Label::new(RichText::new(&entry.expansion)).wrap());
        if let Some(reason) = skip_reason(self.entries, index) {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    theme::icon_text(theme::icons::WARNING)
                        .small()
                        .color(theme::warning(ui.visuals())),
                );
                ui.label(
                    RichText::new(reason)
                        .small()
                        .color(theme::warning(ui.visuals())),
                );
            });
        }
        ui.horizontal(|ui| {
            let edit = ui.button(theme::icon_label_job(
                ui.style(),
                theme::icons::PENCIL_SIMPLE,
                "Edit",
            ));
            theme::focus_ring(ui, &edit);
            if edit.clicked() {
                self.edit = Some(index);
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
        empty_state(ui);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hark_config::Scope;

    fn invocation(phrase: &str, expansion: &str) -> Invocation {
        Invocation {
            phrase: phrase.to_owned(),
            aliases: Vec::new(),
            expansion: expansion.to_owned(),
            scope: Scope::Utterance,
        }
    }

    fn rows() -> Vec<Invocation> {
        vec![
            invocation("access granted", "first"),
            invocation("sign off", "second"),
            invocation("lunch order", "third"),
        ]
    }

    /// Spec AC12: both views route Save, Cancel and Delete through this one
    /// function, so equal outcomes give equal `entries`.
    #[test]
    fn apply_outcome_saves_replaces_appends_and_deletes() {
        let mut page = InvocationsPage::new();

        let mut entries = rows();
        page.draft = Some(Draft::existing(1, &entries[1]));
        assert!(page.apply_outcome(
            &mut entries,
            Outcome::Saved(invocation("sign off", "edited"), Some(1))
        ));
        assert_eq!(entries[1].expansion, "edited");
        assert_eq!((entries.len(), page.book.anchor), (3, 1));
        assert!(page.draft.is_none());

        assert!(page.apply_outcome(
            &mut entries,
            Outcome::Saved(invocation("new one", "x"), None)
        ));
        assert_eq!((entries.len(), page.book.anchor), (4, 3));

        // A row deleted elsewhere mid-edit: append rather than lose the text.
        assert!(page.apply_outcome(
            &mut entries,
            Outcome::Saved(invocation("late", "y"), Some(40))
        ));
        assert_eq!((entries.len(), page.book.anchor), (5, 4));

        assert!(page.apply_outcome(&mut entries, Outcome::Deleted(4)));
        assert_eq!((entries.len(), page.book.anchor), (4, 3));

        page.draft = Some(Draft::new_entry());
        assert!(!page.apply_outcome(&mut entries, Outcome::Cancelled));
        assert!(page.draft.is_none());
        assert!(!page.apply_outcome(&mut entries, Outcome::Open));
        assert_eq!(entries.len(), 4);
    }

    fn painted_texts(page: &mut InvocationsPage, invocations: &mut Invocations) -> Vec<String> {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        ctx.data_mut(|d| d.insert_persisted(view_id(), "book".to_owned()));
        let mut texts = Vec::new();
        for _ in 0..2 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::Vec2::new(1000.0, 760.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    assert!(!page.show(ui, invocations));
                },
            );
            output.textures_delta.clear();
            texts = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
                    _ => None,
                })
                .collect();
        }
        texts
    }

    #[test]
    fn the_book_writes_each_expansion_out_in_full() {
        let mut page = InvocationsPage::new();
        let mut invocations = Invocations {
            entries: vec![
                invocation("access granted", "Line one\nLine two"),
                invocation("granted", "one word cannot fire"),
            ],
        };
        let texts = painted_texts(&mut page, &mut invocations);
        assert!(texts.iter().any(|t| t == "Line one\nLine two"), "{texts:?}");
        assert!(texts.iter().any(|t| t == "A"), "drop cap");
        assert!(
            texts.iter().any(|t| t == Scope::Utterance.label()),
            "scope chip"
        );
        assert!(texts.iter().any(|t| t.contains("two words")), "skip reason");
        assert!(
            texts.iter().any(|t| t.contains("\u{2014} 1 \u{2014}")),
            "folio"
        );
    }

    #[test]
    fn an_open_draft_lies_flat_across_the_spread() {
        let mut page = InvocationsPage::new();
        let mut invocations = Invocations { entries: rows() };
        page.draft = Some(Draft::existing(0, &invocations.entries[0]));
        let texts = painted_texts(&mut page, &mut invocations);
        assert!(texts.iter().any(|t| t == "Edit invocation"), "{texts:?}");
        assert!(
            !texts.iter().any(|t| t.contains("\u{2014} 1 \u{2014}")),
            "a flat book has no folios"
        );
        assert!(page.draft.is_some(), "the draft stays open");
    }
}

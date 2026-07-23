//! The dictionary editor (spec §3.11): pinned add field (Enter adds),
//! inline edit, per-row delete. Every change persists immediately and
//! restarts the pipeline (bias terms are baked in at start; restart is the
//! correct-by-construction baseline). The caller owns persistence.

use crate::theme;
use crate::ui::widgets;
use egui::{Key, RichText, TextEdit, Ui};

pub struct DictionaryPage {
    add: String,
    /// In-progress row edit: (index, buffer). At most one at a time.
    edit: Option<(usize, String)>,
    edit_needs_focus: bool,
    /// Persistence failure surfaced by the caller; sticky until it clears.
    notice: Option<String>,
}

impl DictionaryPage {
    pub fn new() -> Self {
        DictionaryPage {
            add: String::new(),
            edit: None,
            edit_needs_focus: false,
            notice: None,
        }
    }

    pub fn set_notice(&mut self, notice: Option<String>) {
        self.notice = notice;
    }

    /// Render. Returns true when `terms` changed (caller persists then).
    pub fn show(&mut self, ui: &mut Ui, terms: &mut Vec<String>) -> bool {
        let mut changed = false;

        ui.label(
            RichText::new(
                "Corrections apply on this device after transcription; entries are \
                 also sent to your STT provider as accuracy hints.",
            )
            .weak(),
        );
        ui.add_space(6.0);

        // Pinned add row.
        ui.horizontal(|ui| {
            let response = ui.add(
                TextEdit::singleline(&mut self.add)
                    .hint_text("Add a term")
                    .desired_width(280.0),
            );
            let entered = response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            let clicked = ui
                .add_enabled(
                    !self.add.trim().is_empty(),
                    theme::primary_button(ui.visuals(), "Add"),
                )
                .clicked();
            if entered || clicked {
                if add_term(terms, &self.add) {
                    changed = true;
                }
                self.add.clear();
                if entered {
                    // Keep the flow: Enter adds and the field stays ready.
                    response.request_focus();
                }
            }
        });

        if let Some(error) = &self.notice {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(theme::icons::WARNING).color(theme::DANGER));
                ui.label(RichText::new(error).small());
            });
        }
        ui.add_space(8.0);

        if terms.is_empty() {
            widgets::empty_state(
                ui,
                theme::icons::BOOK_OPEN,
                "No dictionary terms yet.",
                "Add names and terms your provider keeps missing.",
            );
            return changed;
        }

        let mut delete: Option<usize> = None;
        for index in 0..terms.len() {
            ui.horizontal(|ui| {
                if let Some((edit_index, buffer)) = &mut self.edit {
                    if *edit_index == index {
                        let response = ui.add(TextEdit::singleline(buffer).desired_width(280.0));
                        if self.edit_needs_focus {
                            response.request_focus();
                            self.edit_needs_focus = false;
                        }
                        let escape = ui.input(|i| i.key_pressed(Key::Escape));
                        if escape {
                            self.edit = None;
                        } else if response.lost_focus() {
                            let (i, buffer) = self.edit.take().expect("edit is Some here");
                            if commit_edit(terms, i, &buffer) {
                                changed = true;
                            }
                        }
                        return;
                    }
                }
                // A flat, full-row button: click to edit in place.
                if ui
                    .add(
                        egui::Button::new(RichText::new(&terms[index]).monospace())
                            .fill(egui::Color32::TRANSPARENT)
                            .stroke(egui::Stroke::NONE),
                    )
                    .on_hover_text("Click to edit")
                    .clicked()
                {
                    self.edit = Some((index, terms[index].clone()));
                    self.edit_needs_focus = true;
                }
                if ui
                    .button(RichText::new(theme::icons::TRASH))
                    .on_hover_text("Delete term")
                    .clicked()
                {
                    delete = Some(index);
                }
            });
            // Nocturne fading rule under each term row.
            theme::fading_rule(ui, 6.0);
        }
        if let Some(index) = delete {
            terms.remove(index);
            self.edit = None;
            changed = true;
        }
        changed
    }
}

/// Add a trimmed, non-empty, non-duplicate term. Returns true if added.
fn add_term(terms: &mut Vec<String>, raw: &str) -> bool {
    let term = raw.trim();
    if term.is_empty() || terms.iter().any(|t| t == term) {
        return false;
    }
    terms.push(term.to_string());
    true
}

/// Commit an inline edit: trimmed and unique replaces; empty or duplicate
/// input reverts (a row is deleted with its button, never by blanking).
fn commit_edit(terms: &mut [String], index: usize, raw: &str) -> bool {
    let term = raw.trim();
    if index >= terms.len() || term.is_empty() || terms[index] == term {
        return false;
    }
    if terms.iter().any(|t| t == term) {
        return false;
    }
    terms[index] = term.to_string();
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_trims_dedupes_and_rejects_empty() {
        let mut terms = vec!["Hark".to_string()];
        assert!(add_term(&mut terms, "  Deepgram  "));
        assert_eq!(terms, ["Hark", "Deepgram"]);
        assert!(!add_term(&mut terms, "Hark"));
        assert!(!add_term(&mut terms, "   "));
        assert_eq!(terms.len(), 2);
    }

    #[test]
    fn edit_replaces_in_place_and_reverts_on_bad_input() {
        let mut terms = vec!["Hark".to_string(), "Deepgram".to_string()];
        assert!(commit_edit(&mut terms, 0, " Harken "));
        assert_eq!(terms[0], "Harken");
        // Empty reverts rather than deleting.
        assert!(!commit_edit(&mut terms, 0, "  "));
        assert_eq!(terms[0], "Harken");
        // A duplicate of another row reverts.
        assert!(!commit_edit(&mut terms, 0, "Deepgram"));
        assert_eq!(terms[0], "Harken");
        // Unchanged text is not a change.
        assert!(!commit_edit(&mut terms, 1, "Deepgram"));
        // Out-of-range never panics.
        assert!(!commit_edit(&mut terms, 9, "x"));
    }
}

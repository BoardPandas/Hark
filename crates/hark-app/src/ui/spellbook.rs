//! The spellbook editor (spec §3.11): pinned add field (Enter adds),
//! inline edit, per-row delete. Every change persists immediately and
//! restarts the pipeline (bias terms are baked in at start; restart is the
//! correct-by-construction baseline). The caller owns persistence.

use crate::theme;
use crate::ui::widgets;
use egui::text::{CCursor, CCursorRange};
use egui::{Key, RichText, TextEdit, Ui};

pub struct SpellbookPage {
    add: String,
    /// In-progress row edit: (index, buffer). At most one at a time.
    edit: Option<(usize, String)>,
    edit_needs_focus: bool,
    /// Persistence failure surfaced by the caller; sticky until it clears.
    notice: Option<String>,
    /// The transcript span this add field was primed from, when the user
    /// arrived by selecting text in History. Drives the explanatory line and
    /// the one-shot focus/select-all.
    primed_from: Option<String>,
    add_needs_focus: bool,
    /// Last term added this session, offered for one-click undo. Adding from
    /// History is a two-click gesture, so it must be a one-click mistake to
    /// unmake or people will not use it freely.
    last_added: Option<String>,
}

impl SpellbookPage {
    pub fn new() -> Self {
        SpellbookPage {
            add: String::new(),
            edit: None,
            edit_needs_focus: false,
            notice: None,
            primed_from: None,
            add_needs_focus: false,
            last_added: None,
        }
    }

    pub fn set_notice(&mut self, notice: Option<String>) {
        self.notice = notice;
    }

    /// Arrive from a History selection: the misheard span becomes the starting
    /// contents of the add field.
    ///
    /// It is a starting point, not the answer. "Al Drazi" is what the provider
    /// got *wrong*; storing it verbatim would be worse than useless. The field
    /// is focused with the text selected so the first keystroke replaces it,
    /// and only the corrected spelling is ever saved. (Keeping the misheard
    /// form as well is what aliases are for -- a later slice.)
    pub fn prime_add(&mut self, heard: String) {
        self.add = heard.clone();
        self.primed_from = Some(heard);
        self.add_needs_focus = true;
        self.last_added = None;
        self.edit = None;
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

        // Arrived from a History selection: say what the prefilled text is, so
        // nobody saves the misheard spelling by pressing Enter on autopilot.
        if let Some(heard) = &self.primed_from {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(theme::icons::LIGHTNING)
                        .small()
                        .color(theme::accent(ui.visuals())),
                );
                ui.label(
                    RichText::new(format!(
                        "Hark heard \u{201C}{heard}\u{201D}. Type the correct spelling and add it.",
                    ))
                    .small(),
                );
            });
            ui.add_space(2.0);
        }

        // Pinned add row.
        ui.horizontal(|ui| {
            let output = TextEdit::singleline(&mut self.add)
                .hint_text("Add a term")
                .desired_width(280.0)
                .show(ui);
            let response = output.response;
            if self.add_needs_focus {
                self.add_needs_focus = false;
                response.request_focus();
                // Select the whole prefill rather than parking a caret at one
                // end: the text is meant to be replaced, so the first
                // keystroke should replace it.
                let mut state = output.state;
                let all =
                    CCursorRange::two(CCursor::new(0), CCursor::new(self.add.chars().count()));
                state.cursor.set_char_range(Some(all));
                state.store(ui.ctx(), response.id);
            }
            let entered = response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            let clicked = ui
                .add_enabled(
                    !self.add.trim().is_empty(),
                    theme::primary_button(ui.visuals(), "Add"),
                )
                .clicked();
            if entered || clicked {
                let term = self.add.trim().to_string();
                if add_term(terms, &self.add) {
                    changed = true;
                    self.last_added = Some(term);
                }
                self.add.clear();
                self.primed_from = None;
                if entered {
                    // Keep the flow: Enter adds and the field stays ready.
                    response.request_focus();
                }
            }
        });

        // One-click undo for the last add, which is the whole safety net for
        // adding from History: a wrong term corrupts every later dictation
        // containing that sound, so unmaking it must be cheaper than making it.
        if let Some(added) = self.last_added.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(format!(
                        "{} Added \u{201C}{added}\u{201D}",
                        theme::icons::CHECK
                    ))
                    .small()
                    .color(theme::SUCCESS),
                );
                if ui.link(RichText::new("Undo").small()).clicked() {
                    if undo_add(terms, &added) {
                        changed = true;
                    }
                    self.last_added = None;
                }
            });
            ui.add_space(2.0);
        }

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
                "No spellbook terms yet.",
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

/// Remove a term the user just added. Matches by value rather than index
/// because the row order can move underneath the affordance (an edit, a
/// delete) between the add and the undo. Returns true if something was
/// removed; a term the user has since deleted by hand is not an error.
fn undo_add(terms: &mut Vec<String>, added: &str) -> bool {
    match terms.iter().position(|t| t == added) {
        Some(index) => {
            terms.remove(index);
            true
        }
        None => false,
    }
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
    fn undo_removes_the_added_term_and_tolerates_it_being_gone() {
        let mut terms = vec!["Hark".to_string(), "Eldrazi".to_string()];
        assert!(undo_add(&mut terms, "Eldrazi"));
        assert_eq!(terms, ["Hark"]);
        // Already removed by hand between the add and the undo: not an error,
        // and it must not take an unrelated term with it.
        assert!(!undo_add(&mut terms, "Eldrazi"));
        assert_eq!(terms, ["Hark"]);
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

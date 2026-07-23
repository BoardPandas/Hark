//! The Invocations page: trigger phrase -> canned text, listed and edited.
//!
//! Say a phrase, get a block of text you wrote, injected byte for byte.
//! Every change persists immediately and restarts the pipeline (the trigger
//! matcher is built at pipeline start); the caller owns persistence.
//!
//! The editor lives in [`editor`] so both files stay under the ~300-line
//! UI-module guardrail.

mod editor;

use crate::theme;
use crate::ui::widgets;
use editor::Draft;
use egui::{Label, RichText, Sense, Ui};
use hark_config::{Invocation, Invocations};

/// One-line preview length for the expansion column.
const PREVIEW_CHARS: usize = 80;

pub struct InvocationsPage {
    /// The open editor: a new entry, or an existing row by index. At most
    /// one at a time.
    draft: Option<Draft>,
    /// Persistence failure surfaced by the caller; sticky until it clears.
    notice: Option<String>,
}

impl InvocationsPage {
    pub fn new() -> Self {
        InvocationsPage {
            draft: None,
            notice: None,
        }
    }

    pub fn set_notice(&mut self, notice: Option<String>) {
        self.notice = notice;
    }

    /// Render. Returns true when `invocations` changed (caller persists).
    pub fn show(&mut self, ui: &mut Ui, invocations: &mut Invocations) -> bool {
        // An open editor owns the whole page: a full-screen form is easier
        // to reason about than an inline one that reflows the list under
        // the cursor while a multi-line expansion grows.
        if self.draft.is_some() {
            return self.show_editor(ui, invocations);
        }

        ui.label(
            RichText::new(
                "Say the trigger phrase and Hark types the text you wrote, exactly as \
                 you wrote it. Canned text never goes through a cleanup voice, so it \
                 cannot be reworded on the way to your cursor.",
            )
            .weak(),
        );
        ui.add_space(8.0);

        if ui
            .add(theme::primary_button(
                ui.visuals(),
                format!("{}  New invocation", theme::icons::LIGHTNING),
            ))
            .clicked()
        {
            self.draft = Some(Draft::new_entry());
        }

        if let Some(error) = &self.notice {
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(theme::icons::WARNING).color(theme::DANGER));
                ui.label(RichText::new(error).small());
            });
        }
        ui.add_space(8.0);

        if invocations.entries.is_empty() {
            empty_state(ui);
            return false;
        }
        self.list(ui, invocations)
    }

    /// The row list. Plain `ScrollArea::show`, never `show_rows`: rows are
    /// non-uniform (a warning line appears only on entries that cannot
    /// fire), and LL-G `rust/egui-show-rows-uniform-height` is exactly that
    /// -- `row_height * count` arithmetic desyncs the scrollbar and shifts
    /// rows under the cursor.
    fn list(&mut self, ui: &mut Ui, invocations: &mut Invocations) -> bool {
        let mut changed = false;
        let mut delete: Option<usize> = None;
        let mut edit: Option<usize> = None;

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (index, entry) in invocations.entries.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.set_width((ui.available_width() - 96.0).max(160.0));
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    RichText::new(theme::icons::LIGHTNING)
                                        .color(theme::accent(ui.visuals())),
                                );
                                ui.label(RichText::new(&entry.phrase).monospace());
                                ui.label(RichText::new(entry.scope.label()).small().weak());
                            });
                            let preview =
                                Label::new(RichText::new(preview(&entry.expansion)).small().weak())
                                    .sense(Sense::click());
                            if ui.add(preview).on_hover_text("Click to edit").clicked() {
                                edit = Some(index);
                            }
                            // A hand-edited config can hold entries that
                            // will never arm. Say so on the row rather than
                            // letting the user wonder why nothing happens.
                            if let Some(reason) = skip_reason(&invocations.entries, index) {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(
                                        RichText::new(theme::icons::WARNING)
                                            .small()
                                            .color(theme::WARNING),
                                    );
                                    ui.label(RichText::new(reason).small().color(theme::WARNING));
                                });
                            }
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                            if ui
                                .button(RichText::new(theme::icons::TRASH))
                                .on_hover_text("Delete invocation")
                                .clicked()
                            {
                                delete = Some(index);
                            }
                            if ui.button("Edit").clicked() {
                                edit = Some(index);
                            }
                        });
                    });
                    theme::fading_rule(ui, 8.0);
                }
            });

        if let Some(index) = edit {
            self.draft = Some(Draft::existing(index, &invocations.entries[index]));
        }
        if let Some(index) = delete {
            invocations.entries.remove(index);
            self.draft = None;
            changed = true;
        }
        changed
    }

    fn show_editor(&mut self, ui: &mut Ui, invocations: &mut Invocations) -> bool {
        let Some(draft) = &mut self.draft else {
            return false;
        };
        match draft.show(ui, &invocations.entries) {
            editor::Outcome::Open => false,
            editor::Outcome::Cancelled => {
                self.draft = None;
                false
            }
            editor::Outcome::Saved(entry, index) => {
                match index {
                    Some(i) if i < invocations.entries.len() => invocations.entries[i] = entry,
                    // A row deleted from another window mid-edit would land
                    // here; appending is better than panicking or silently
                    // discarding what the user just typed.
                    _ => invocations.entries.push(entry),
                }
                self.draft = None;
                true
            }
            editor::Outcome::Deleted(index) => {
                if index < invocations.entries.len() {
                    invocations.entries.remove(index);
                }
                self.draft = None;
                true
            }
        }
    }
}

/// The "you say -> Hark types" example, so the value is obvious before the
/// user has typed anything.
fn empty_state(ui: &mut Ui) {
    widgets::empty_state(
        ui,
        theme::icons::LIGHTNING,
        "No invocations yet.",
        "Pair a phrase you say with a block of text you'd rather not retype.",
    );
    ui.add_space(14.0);
    ui.vertical_centered(|ui| {
        egui::Grid::new("invocations-example")
            .num_columns(2)
            .spacing([18.0, 4.0])
            .show(ui, |ui| {
                ui.label(RichText::new("You say").small().weak());
                ui.label(RichText::new("Hark types").small().weak());
                ui.end_row();
                ui.label(RichText::new("\"access granted\"").monospace());
                ui.label(
                    RichText::new("You have access to the Support Forge tools\u{2026}").weak(),
                );
                ui.end_row();
            });
    });
}

/// Whitespace-flattened, char-boundary-safe one-line preview. Same shape as
/// the history row's preview; a multi-line expansion must not stretch a row.
fn preview(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(PREVIEW_CHARS) {
        Some((byte, _)) => format!("{}\u{2026}", flat[..byte].trim_end()),
        None => flat,
    }
}

/// Why the entry at `index` will never fire, or `None` when it is armed.
///
/// Mirrors `hark_dictionary::Expander::new`'s build-time gate, using the
/// dictionary's own tokenizer so the two can never disagree about what
/// counts as a word (hyphens split: "access-granted" is two words).
fn skip_reason(entries: &[Invocation], index: usize) -> Option<&'static str> {
    let entry = &entries[index];
    if hark_dictionary::phrase_word_count(&entry.phrase) < hark_dictionary::MIN_TRIGGER_WORDS {
        return Some("Won't fire: a trigger needs at least two words.");
    }
    if entry.expansion.is_empty() {
        return Some("Won't fire: this invocation has no text to type.");
    }
    // First wins, matching the expander. Only a *later* duplicate is dead.
    let key = hark_dictionary::normalized_phrase(&entry.phrase);
    let shadowed = entries[..index]
        .iter()
        .any(|e| hark_dictionary::normalized_phrase(&e.phrase) == key);
    shadowed.then_some("Won't fire: an earlier invocation already uses this trigger.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use hark_config::Scope;

    fn entries(rows: &[(&str, &str)]) -> Vec<Invocation> {
        rows.iter()
            .map(|(phrase, expansion)| Invocation {
                phrase: phrase.to_string(),
                expansion: expansion.to_string(),
                scope: Scope::Utterance,
            })
            .collect()
    }

    #[test]
    fn preview_flattens_whitespace_and_truncates_on_char_boundaries() {
        assert_eq!(preview("one\ntwo\t three"), "one two three");

        let long = "abcde ".repeat(40);
        let cut = preview(&long);
        assert!(cut.ends_with('\u{2026}'));
        assert!(cut.chars().count() <= PREVIEW_CHARS + 1);

        // Multi-byte characters never split a boundary.
        let emoji = "\u{1F3A4}".repeat(PREVIEW_CHARS + 20);
        assert_eq!(preview(&emoji).chars().count(), PREVIEW_CHARS + 1);
    }

    #[test]
    fn an_armed_entry_has_no_warning() {
        let rows = entries(&[("access granted", "the paragraph")]);
        assert_eq!(skip_reason(&rows, 0), None);
    }

    #[test]
    fn skip_reason_names_each_way_an_entry_can_be_dead() {
        let one_word = entries(&[("granted", "text")]);
        assert!(skip_reason(&one_word, 0).unwrap().contains("two words"));

        let empty = entries(&[("access granted", "")]);
        assert!(skip_reason(&empty, 0).unwrap().contains("no text"));

        // Hyphens split, so this clears the two-word gate like the expander.
        let hyphen = entries(&[("access-granted", "text")]);
        assert_eq!(skip_reason(&hyphen, 0), None);
    }

    #[test]
    fn only_the_later_duplicate_is_flagged() {
        // First wins in the expander, so the first row is the live one and
        // must not be marked dead.
        let rows = entries(&[("access granted", "first"), ("Access-Granted!", "second")]);
        assert_eq!(skip_reason(&rows, 0), None);
        assert!(skip_reason(&rows, 1).unwrap().contains("already uses"));
    }
}

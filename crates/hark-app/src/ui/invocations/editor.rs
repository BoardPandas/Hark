//! The invocation editor: trigger, scope, expansion, and a test panel.
//!
//! Commits on an explicit **Save** and never on `lost_focus` the way the
//! dictionary editor does. Focus is lost by clicking a scrollbar or
//! alt-tabbing, and every commit here runs `save_to_disk` plus a full
//! `pipeline.start()` -- hook, worker, and capture restart, including a
//! keychain read. That is far too much to fire by accident, and a
//! half-typed multi-line expansion would be written to disk.

use crate::theme;
use egui::{RichText, TextEdit, Ui};
use hark_config::{Invocation, Scope};
use hark_dictionary::{Expander, MIN_TRIGGER_WORDS};

/// What the editor did this frame.
pub enum Outcome {
    /// Still editing.
    Open,
    Cancelled,
    /// Save pressed: the entry, and the row index it replaces (`None` = new).
    Saved(Invocation, Option<usize>),
    /// Delete pressed on an existing row.
    Deleted(usize),
}

pub struct Draft {
    /// The row being edited; `None` for a new entry.
    index: Option<usize>,
    phrase: String,
    expansion: String,
    scope: Scope,
    /// "Type what you'd say" test input.
    probe: String,
    /// The preview matcher, rebuilt only when this is set -- never per
    /// frame, since building it encodes every trigger phonetically.
    preview: Option<Expander>,
    preview_dirty: bool,
}

impl Draft {
    pub fn new_entry() -> Self {
        Draft {
            index: None,
            phrase: String::new(),
            expansion: String::new(),
            // New entries default to whole-dictation: the scope that cannot
            // fire mid-sentence by accident.
            scope: Scope::Utterance,
            probe: String::new(),
            preview: None,
            preview_dirty: true,
        }
    }

    pub fn existing(index: usize, entry: &Invocation) -> Self {
        Draft {
            index: Some(index),
            phrase: entry.phrase.clone(),
            expansion: entry.expansion.clone(),
            scope: entry.scope,
            probe: String::new(),
            preview: None,
            preview_dirty: true,
        }
    }

    pub fn show(&mut self, ui: &mut Ui, entries: &[Invocation]) -> Outcome {
        let mut outcome = Outcome::Open;

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.label(
                    RichText::new(if self.index.is_some() {
                        "Edit invocation"
                    } else {
                        "New invocation"
                    })
                    .text_style(theme::subheading()),
                );
                ui.add_space(10.0);

                self.trigger_field(ui);
                let problem = self.problem(entries);
                if let Some(problem) = &problem {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(theme::icons::WARNING)
                                .small()
                                .color(theme::DANGER),
                        );
                        ui.label(RichText::new(problem).small().color(theme::DANGER));
                    });
                }
                ui.add_space(12.0);

                self.scope_field(ui);
                ui.add_space(12.0);
                self.expansion_field(ui);
                ui.add_space(12.0);
                self.test_panel(ui);
                ui.add_space(16.0);

                ui.horizontal(|ui| {
                    let saveable = problem.is_none() && !self.expansion.is_empty();
                    if ui
                        .add_enabled(saveable, egui::Button::new("Save"))
                        .clicked()
                    {
                        outcome = Outcome::Saved(
                            Invocation {
                                phrase: self.phrase.trim().to_string(),
                                expansion: self.expansion.clone(),
                                scope: self.scope,
                            },
                            self.index,
                        );
                    }
                    if ui.button("Cancel").clicked() {
                        outcome = Outcome::Cancelled;
                    }
                    if let Some(index) = self.index {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .button(RichText::new(format!("{}  Delete", theme::icons::TRASH)))
                                .clicked()
                            {
                                outcome = Outcome::Deleted(index);
                            }
                        });
                    }
                });
            });

        outcome
    }

    fn trigger_field(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("When I say").text_style(theme::subheading()));
        ui.label(
            RichText::new("The phrase you'll speak. Two words or more.")
                .small()
                .weak(),
        );
        ui.add_space(4.0);
        let response = ui.add(
            TextEdit::singleline(&mut self.phrase)
                .hint_text("access granted")
                .desired_width(320.0),
        );
        if response.changed() {
            self.preview_dirty = true;
        }
    }

    /// Two radio buttons with plain-language labels, plus the one
    /// consequence the user cannot discover on their own.
    fn scope_field(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Fire when").text_style(theme::subheading()));
        ui.add_space(4.0);
        let mut scope = self.scope;
        ui.radio_value(
            &mut scope,
            Scope::Utterance,
            "That phrase is the whole dictation",
        );
        ui.radio_value(
            &mut scope,
            Scope::Anywhere,
            "That phrase appears anywhere in the sentence",
        );
        if scope != self.scope {
            self.scope = scope;
            self.preview_dirty = true;
        }
        if self.scope == Scope::Anywhere {
            ui.add_space(2.0);
            ui.label(
                RichText::new(
                    "Anywhere also means this dictation skips your cleanup voice, so \
                     filler words around the inserted text stay as spoken.",
                )
                .small()
                .weak(),
            );
        }
    }

    fn expansion_field(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Type this").text_style(theme::subheading()));
        ui.label(
            RichText::new("Injected exactly as written, line breaks and all.")
                .small()
                .weak(),
        );
        ui.add_space(4.0);
        let response = ui.add(
            TextEdit::multiline(&mut self.expansion)
                .desired_rows(6)
                .desired_width(f32::INFINITY),
        );
        if response.changed() {
            self.preview_dirty = true;
        }
    }

    /// "Would this fire?" against the real matcher, so the answer is the
    /// truth rather than a second implementation's opinion.
    fn test_panel(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Try it").text_style(theme::subheading()));
        ui.add_space(4.0);
        let response = ui.add(
            TextEdit::singleline(&mut self.probe)
                .hint_text("Type what you'd say")
                .desired_width(320.0),
        );
        if response.changed() {
            self.preview_dirty = true;
        }
        if self.probe.trim().is_empty() {
            return;
        }

        if self.preview_dirty {
            let scope = match self.scope {
                Scope::Utterance => hark_dictionary::Scope::Utterance,
                Scope::Anywhere => hark_dictionary::Scope::Anywhere,
            };
            // Expansion stands in as non-empty so the trigger can arm while
            // the user is still writing the text it will type.
            let expansion = if self.expansion.is_empty() {
                " ".to_string()
            } else {
                self.expansion.clone()
            };
            self.preview = Some(Expander::new(&[(self.phrase.clone(), expansion, scope)]));
            self.preview_dirty = false;
        }
        let Some(preview) = &self.preview else {
            return;
        };

        ui.add_space(4.0);
        if preview.expand(&self.probe).fired.is_some() {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(theme::icons::CHECK).color(theme::SUCCESS));
                ui.label(RichText::new("Would fire").color(theme::SUCCESS));
            });
            return;
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(theme::icons::X).weak());
            ui.label(RichText::new("Would not fire").weak());
        });
        // The near-miss hint: close but rejected is the confusing case, and
        // without a number the user has no idea whether to reword or retry.
        if let Some((phrase, score)) = preview.closest(&self.probe) {
            if score > 0.7 {
                ui.label(
                    RichText::new(format!(
                        "Closest match \"{phrase}\" at {:.0}% \u{2014} not close enough to \
                         act on.",
                        score * 100.0
                    ))
                    .small()
                    .weak(),
                );
            }
        }
    }

    /// Why Save is disabled, or `None` when the trigger is usable. Mirrors
    /// the expander's build-time gate via the same tokenizer.
    fn problem(&self, entries: &[Invocation]) -> Option<String> {
        if self.phrase.trim().is_empty() {
            return Some("Enter a trigger phrase.".to_string());
        }
        if hark_dictionary::phrase_word_count(&self.phrase) < MIN_TRIGGER_WORDS {
            return Some(
                "A trigger needs at least two words \u{2014} one-word triggers fire \
                 against ordinary speech."
                    .to_string(),
            );
        }
        let key = hark_dictionary::normalized_phrase(&self.phrase);
        let clashes = entries.iter().enumerate().any(|(i, e)| {
            Some(i) != self.index && hark_dictionary::normalized_phrase(&e.phrase) == key
        });
        clashes.then(|| "Another invocation already uses this trigger.".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(phrase: &str) -> Invocation {
        Invocation {
            phrase: phrase.to_string(),
            expansion: "text".to_string(),
            scope: Scope::Utterance,
        }
    }

    fn draft(phrase: &str) -> Draft {
        let mut d = Draft::new_entry();
        d.phrase = phrase.to_string();
        d
    }

    /// Both this editor and the row warning say "at least two words" in
    /// prose. Pin the constant they are describing so the copy on two
    /// screens cannot quietly start lying if it ever changes.
    #[test]
    fn the_trigger_minimum_matches_the_copy() {
        assert_eq!(MIN_TRIGGER_WORDS, 2);
    }

    #[test]
    fn a_blank_or_one_word_trigger_blocks_save() {
        assert!(draft("").problem(&[]).unwrap().contains("Enter a trigger"));
        assert!(draft("   ").problem(&[]).is_some());
        assert!(draft("granted").problem(&[]).unwrap().contains("two words"));
    }

    #[test]
    fn a_two_word_trigger_is_accepted() {
        assert_eq!(draft("access granted").problem(&[]), None);
        // Hyphens split, matching the expander's tokenizer.
        assert_eq!(draft("access-granted").problem(&[]), None);
    }

    #[test]
    fn a_duplicate_trigger_blocks_save() {
        let existing = [entry("access granted")];
        let problem = draft("Access-Granted!").problem(&existing).unwrap();
        assert!(problem.contains("already uses"));
    }

    #[test]
    fn editing_a_row_does_not_clash_with_itself() {
        // The row being edited must not count as its own duplicate, or
        // every existing invocation would be uneditable.
        let existing = [entry("access granted"), entry("ticket closed")];
        let mut d = Draft::existing(0, &existing[0]);
        assert_eq!(d.problem(&existing), None);

        // It still clashes with a *different* row.
        d.phrase = "ticket closed".to_string();
        assert!(d.problem(&existing).is_some());
    }

    #[test]
    fn new_entries_default_to_whole_dictation_scope() {
        assert_eq!(Draft::new_entry().scope, Scope::Utterance);
    }

    #[test]
    fn existing_entries_load_their_own_values() {
        let source = Invocation {
            phrase: "ticket closed".to_string(),
            expansion: "Closing this out.\nThanks!".to_string(),
            scope: Scope::Anywhere,
        };
        let d = Draft::existing(3, &source);
        assert_eq!(d.index, Some(3));
        assert_eq!(d.phrase, "ticket closed");
        assert_eq!(d.expansion, "Closing this out.\nThanks!");
        assert_eq!(d.scope, Scope::Anywhere);
    }
}

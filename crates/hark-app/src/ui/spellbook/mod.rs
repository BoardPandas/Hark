//! The spellbook editor (spec §3.11): pinned add row (Enter adds), inline
//! term edit, per-entry alias list, per-row delete. Every change persists
//! immediately and restarts the pipeline (entries are baked in at start;
//! restart is the correct-by-construction baseline). The caller owns
//! persistence.
//!
//! Two ways an entry corrects a transcript, and the difference is the whole
//! reason aliases exist:
//!
//! - the **term** is matched phonetically, so it catches spellings nobody
//!   enumerated;
//! - an **alias** is matched exactly, for the pairs the phonetic pass cannot
//!   or should not reach.
//!
//! Aliases live behind a per-entry Advanced affordance. Per-entry, never
//! global: a global mode would tax the simple path forever, and the common
//! case is still a bare word.

mod edit;

use crate::theme;
use crate::ui::widgets;
use edit::{add_alias, add_entry, commit_edit, is_common_word, remove_alias, undo_add};
use egui::text::{CCursor, CCursorRange};
use egui::{Key, RichText, TextEdit, Ui};
use hark_config::SpellbookEntry;

pub struct SpellbookPage {
    add: String,
    /// Misheard spelling to store alongside the term being added. Filled by a
    /// History handoff; typed by hand only in advanced mode.
    add_alias: String,
    add_advanced: bool,
    /// In-progress row edit: (index, buffer). At most one at a time.
    edit: Option<(usize, String)>,
    edit_needs_focus: bool,
    /// Entry whose aliases are open for editing.
    expanded: Option<usize>,
    new_alias: String,
    /// Persistence failure surfaced by the caller; sticky until it clears.
    notice: Option<String>,
    /// The transcript span the add row was primed from, when the user arrived
    /// by selecting text in History.
    primed_from: Option<String>,
    add_needs_focus: bool,
    /// Last term added this session, offered for one-click undo.
    last_added: Option<String>,
}

impl SpellbookPage {
    pub fn new() -> Self {
        SpellbookPage {
            add: String::new(),
            add_alias: String::new(),
            add_advanced: false,
            edit: None,
            edit_needs_focus: false,
            expanded: None,
            new_alias: String::new(),
            notice: None,
            primed_from: None,
            add_needs_focus: false,
            last_added: None,
        }
    }

    pub fn set_notice(&mut self, notice: Option<String>) {
        self.notice = notice;
    }

    /// Arrive from a History selection: the misheard span becomes an **alias**
    /// and the term field waits for the correct spelling.
    ///
    /// This is what aliases were added for. Without them the handoff could only
    /// prefill the term with "Al Drazi" -- the spelling the provider got
    /// *wrong* -- and the user had to type over it, throwing the misheard form
    /// away. Now both are kept: the term is what gets injected, the alias is
    /// what gets recognised, and the user types the one thing Hark cannot know.
    pub fn prime_add(&mut self, heard: String) {
        self.add.clear();
        self.add_alias = heard.clone();
        self.add_advanced = true;
        self.primed_from = Some(heard);
        self.add_needs_focus = true;
        self.last_added = None;
        self.edit = None;
    }

    /// Render. Returns true when `entries` changed (caller persists then).
    pub fn show(&mut self, ui: &mut Ui, entries: &mut Vec<SpellbookEntry>) -> bool {
        let mut changed = false;

        ui.label(
            RichText::new(
                "Corrections apply on this device after transcription; terms are \
                 also sent to your STT provider as accuracy hints.",
            )
            .weak(),
        );
        ui.add_space(6.0);

        changed |= self.add_row(ui, entries);
        changed |= self.undo_row(ui, entries);

        if let Some(error) = &self.notice {
            ui.horizontal_wrapped(|ui| {
                ui.label(theme::icon_text(theme::icons::WARNING).color(theme::DANGER));
                ui.label(RichText::new(error).small());
            });
        }
        ui.add_space(8.0);

        if entries.is_empty() {
            widgets::empty_state(
                ui,
                theme::icons::BOOK_OPEN,
                "No spellbook terms yet.",
                "Add names and terms your provider keeps missing.",
            );
            return changed;
        }
        changed | self.entry_list(ui, entries)
    }

    /// The pinned add row, plus the Advanced alias field and its guard.
    fn add_row(&mut self, ui: &mut Ui, entries: &mut Vec<SpellbookEntry>) -> bool {
        let mut changed = false;

        // Arrived from a History selection: name what was heard, so the term
        // field reads as "type the correction" rather than "type something".
        if let Some(heard) = &self.primed_from {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    theme::icon_text(theme::icons::LIGHTNING)
                        .small()
                        .color(theme::accent(ui.visuals())),
                );
                ui.label(
                    RichText::new(format!(
                        "Hark heard \u{201C}{heard}\u{201D}. Type the correct spelling; the \
                         misheard version is kept below so it is corrected next time.",
                    ))
                    .small(),
                );
            });
            ui.add_space(2.0);
        }

        let mut submit = false;
        ui.horizontal(|ui| {
            let output = TextEdit::singleline(&mut self.add)
                .hint_text("Add a term")
                .desired_width(280.0)
                .show(ui);
            let response = output.response;
            if self.add_needs_focus {
                self.add_needs_focus = false;
                response.request_focus();
                let all =
                    CCursorRange::two(CCursor::new(0), CCursor::new(self.add.chars().count()));
                let mut state = output.state;
                state.cursor.set_char_range(Some(all));
                state.store(ui.ctx(), response.id);
            }
            if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                submit = true;
            }
            if ui
                .add_enabled(
                    !self.add.trim().is_empty(),
                    theme::primary_button(ui.visuals(), "Add"),
                )
                .clicked()
            {
                submit = true;
            }
            ui.checkbox(&mut self.add_advanced, "Advanced")
                .on_hover_text("Also correct a specific misheard spelling to this term");
        });

        if self.add_advanced {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Also correct").small().weak());
                ui.add(
                    TextEdit::singleline(&mut self.add_alias)
                        .hint_text("what Hark heard")
                        .desired_width(240.0),
                );
            });
            alias_warning(ui, &self.add_alias);
        }

        if submit {
            let term = self.add.trim().to_string();
            if add_entry(entries, &self.add, &self.add_alias) {
                changed = true;
                self.last_added = Some(term);
            }
            self.add.clear();
            self.add_alias.clear();
            self.primed_from = None;
        }
        changed
    }

    /// One-click undo for the last add: a wrong term corrupts every later
    /// dictation containing that sound, so unmaking it must be cheaper than
    /// making it.
    fn undo_row(&mut self, ui: &mut Ui, entries: &mut Vec<SpellbookEntry>) -> bool {
        let Some(added) = self.last_added.clone() else {
            return false;
        };
        let mut changed = false;
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
                changed = undo_add(entries, &added);
                self.last_added = None;
            }
        });
        ui.add_space(2.0);
        changed
    }

    /// The term rows scroll; the add row, the undo affirmation, and the notice
    /// above them stay pinned, as the toolbar does in History. Plain `show`,
    /// never `show_rows`: an open alias editor makes rows non-uniform, and
    /// LL-G `rust/egui-show-rows-uniform-height` is exactly that -- the
    /// `row_height * count` arithmetic desyncs the scrollbar and shifts rows
    /// under the cursor.
    fn entry_list(&mut self, ui: &mut Ui, entries: &mut Vec<SpellbookEntry>) -> bool {
        let mut changed = false;
        let mut delete: Option<usize> = None;
        egui::ScrollArea::vertical()
            .id_salt("spellbook-list")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for index in 0..entries.len() {
                    ui.horizontal(|ui| {
                        if let Some((edit_index, buffer)) = &mut self.edit {
                            if *edit_index == index {
                                let response =
                                    ui.add(TextEdit::singleline(buffer).desired_width(280.0));
                                if self.edit_needs_focus {
                                    response.request_focus();
                                    self.edit_needs_focus = false;
                                }
                                if ui.input(|i| i.key_pressed(Key::Escape)) {
                                    self.edit = None;
                                } else if response.lost_focus() {
                                    let (i, buffer) = self.edit.take().expect("edit is Some here");
                                    changed |= commit_edit(entries, i, &buffer);
                                }
                                return;
                            }
                        }
                        // A flat, full-row button: click to edit in place.
                        if ui
                            .add(
                                egui::Button::new(RichText::new(&entries[index].term).monospace())
                                    .fill(egui::Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::NONE),
                            )
                            .on_hover_text("Click to edit")
                            .clicked()
                        {
                            self.edit = Some((index, entries[index].term.clone()));
                            self.edit_needs_focus = true;
                        }
                        if ui
                            .button(theme::icon_text(theme::icons::TRASH))
                            .on_hover_text("Delete term")
                            .clicked()
                        {
                            delete = Some(index);
                        }
                        // The alias count is the row's only hint that an entry
                        // does more than its term suggests; without it aliases
                        // are invisible until something they caused looks wrong.
                        let count = entries[index].aliases.len();
                        let open = self.expanded == Some(index);
                        let label = match count {
                            0 => "Advanced".to_string(),
                            1 => "1 alias".to_string(),
                            n => format!("{n} aliases"),
                        };
                        if ui
                            .link(RichText::new(label).small().weak())
                            .on_hover_text("Misheard spellings corrected to this term")
                            .clicked()
                        {
                            self.expanded = (!open).then_some(index);
                            self.new_alias.clear();
                        }
                    });
                    if self.expanded == Some(index) {
                        changed |= self.alias_editor(ui, entries, index);
                    }
                    // Nocturne fading rule under each term row.
                    theme::fading_rule(ui, 6.0);
                }
            });
        if let Some(index) = delete {
            entries.remove(index);
            self.edit = None;
            self.expanded = None;
            changed = true;
        }
        changed
    }

    fn alias_editor(&mut self, ui: &mut Ui, entries: &mut [SpellbookEntry], index: usize) -> bool {
        let mut changed = false;
        let mut remove: Option<usize> = None;
        let new_alias = &mut self.new_alias;
        ui.indent(("aliases", index), |ui| {
            for (i, alias) in entries[index].aliases.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(alias).monospace().small());
                    if ui
                        .small_button(theme::icon_text(theme::icons::X))
                        .on_hover_text("Remove this spelling")
                        .clicked()
                    {
                        remove = Some(i);
                    }
                });
            }
            let mut submit = false;
            ui.horizontal(|ui| {
                let response = ui.add(
                    TextEdit::singleline(new_alias)
                        .hint_text("what Hark heard")
                        .desired_width(220.0),
                );
                if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    submit = true;
                    response.request_focus();
                }
                if ui
                    .add_enabled(!new_alias.trim().is_empty(), egui::Button::new("Add"))
                    .clicked()
                {
                    submit = true;
                }
            });
            alias_warning(ui, new_alias);
            if submit {
                changed |= add_alias(entries, index, new_alias);
                new_alias.clear();
            }
        });
        if let Some(i) = remove {
            changed |= remove_alias(entries, index, i);
        }
        changed
    }
}

/// Warn, never block, when an alias is made entirely of common words: it would
/// fire on ordinary speech. The user may still know better than the guard --
/// niche jargon really can be spelled like a common word -- so this informs the
/// decision rather than overriding it.
fn alias_warning(ui: &mut Ui, alias: &str) {
    if !is_common_word(alias) {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.label(
            theme::icon_text(theme::icons::WARNING)
                .small()
                .color(theme::WARNING),
        );
        ui.label(
            RichText::new(
                "That is an everyday word, so this would fire whenever you say it. \
                 Use a longer phrase if you can.",
            )
            .small()
            .weak(),
        );
    });
}

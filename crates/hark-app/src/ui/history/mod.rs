//! The History panel (spec §3.7/§3.11): search-as-you-type toolbar, day
//! group headers, expandable rows with copy/delete, clear-all behind a
//! confirm, and honest empty states.
//!
//! The list renders from a cached window of reader-connection queries keyed
//! on (write generation, search, loaded pages); it re-queries only when that
//! key moves, so idle frames never touch the database. Planning called for
//! `ScrollArea::show_rows`, but that requires uniform row heights and this
//! list is deliberately heterogeneous (day headers, expandable rows):
//! windowed LIMIT queries plus a "Show more" sentinel bound the work
//! instead, and the retention cap bounds the worst case.

mod row;

use crate::storage::{StorageCmd, StorageHandle};
use crate::theme;
use crate::ui::{format, widgets};
use egui::{Align, Color32, Key, Layout, Modifiers, RichText, ScrollArea, TextEdit, Ui};
use hark_store::{Entry, StoreError};
use jiff::tz::TimeZone;

const PAGE_SIZE: u32 = 100;

pub struct HistoryPage {
    search: String,
    /// Cached newest-first window (`pages * PAGE_SIZE` rows max).
    entries: Vec<Entry>,
    /// Rows matching the search (drives the count and "Show more").
    matching: u64,
    /// All stored rows (distinguishes "no dictations yet" from "no match",
    /// and gives Clear all its honest count).
    total: u64,
    /// Windows loaded; grows via "Show more", resets when the search edits.
    pages: u32,
    /// (generation, search, pages) the cache reflects.
    cache_key: Option<(u64, String, u32)>,
    /// Reader query failure; sticky until a query succeeds.
    fetch_error: Option<String>,
    /// Expanded row (entry id); at most one, Esc or re-click collapses.
    expanded: Option<i64>,
    /// Row whose "Copied" affirmation is showing (it fades to invisible).
    copied: Option<i64>,
    confirm: Option<widgets::Confirm>,
    focus_search: bool,
    tz: TimeZone,
}

impl HistoryPage {
    pub fn new() -> Self {
        HistoryPage {
            search: String::new(),
            entries: Vec::new(),
            matching: 0,
            total: 0,
            pages: 1,
            cache_key: None,
            fetch_error: None,
            expanded: None,
            copied: None,
            confirm: None,
            focus_search: false,
            tz: TimeZone::system(),
        }
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        storage: Option<&StorageHandle>,
        unavailable: Option<&str>,
        chord: &str,
    ) {
        let Some(storage) = storage else {
            widgets::empty_state(
                ui,
                theme::icons::WARNING,
                "History is unavailable.",
                unavailable.unwrap_or("The local database could not be opened."),
            );
            return;
        };
        self.refresh(storage);

        // Ctrl+F focuses search from anywhere on the page (spec §3.11).
        if ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::F)) {
            self.focus_search = true;
        }
        self.toolbar(ui);

        if let Some(error) = &self.fetch_error {
            widgets::empty_state(ui, theme::icons::WARNING, "History cannot be read.", error);
            return;
        }
        if self.total == 0 {
            widgets::empty_state(
                ui,
                theme::icons::CLOCK_COUNTER_CLOCKWISE,
                "Dictations appear here.",
                &format!("Hold {chord} and speak into any text field."),
            );
            return;
        }
        if self.matching == 0 {
            widgets::empty_state(
                ui,
                theme::icons::MAGNIFYING_GLASS,
                &format!("No matches for \"{}\".", self.search.trim()),
                "Search covers both the raw transcript and the final text.",
            );
            return;
        }

        self.list(ui, storage);

        // Esc collapses the expanded row, unless a modal owns Esc right now.
        if self.confirm.is_none()
            && self.expanded.is_some()
            && ui.input(|i| i.key_pressed(Key::Escape))
        {
            self.expanded = None;
        }
        if let Some(confirm) = &mut self.confirm {
            match confirm.show(ui, "history-clear") {
                Some(true) => {
                    storage.send(StorageCmd::ClearEntries);
                    self.expanded = None;
                    self.confirm = None;
                }
                Some(false) => self.confirm = None,
                None => {}
            }
        }
    }

    /// Re-query when the cache key moves (a write landed, the search edited,
    /// or another page was requested). One windowed query, newest first.
    fn refresh(&mut self, storage: &StorageHandle) {
        let key = (storage.generation(), self.search.clone(), self.pages);
        if self.cache_key.as_ref() == Some(&key) {
            return;
        }
        let reader = storage.reader();
        let search = Some(self.search.as_str()).filter(|s| !s.trim().is_empty());
        let fetch = || -> Result<(Vec<Entry>, u64, u64), StoreError> {
            Ok((
                reader.entries(search, self.pages * PAGE_SIZE, 0)?,
                reader.entry_count(search)?,
                reader.entry_count(None)?,
            ))
        };
        match fetch() {
            Ok((entries, matching, total)) => {
                self.entries = entries;
                self.matching = matching;
                self.total = total;
                self.fetch_error = None;
                // A deleted or pruned row cannot stay expanded.
                if let Some(id) = self.expanded {
                    if !self.entries.iter().any(|e| e.id == id) {
                        self.expanded = None;
                    }
                }
            }
            Err(e) => {
                log::error!("history query failed: {e}");
                self.fetch_error = Some(e.to_string());
            }
        }
        // Set even on failure so a broken reader is not re-queried every
        // frame; the next write (or search edit) retries naturally.
        self.cache_key = Some(key);
    }

    fn toolbar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let response = ui.add(
                TextEdit::singleline(&mut self.search)
                    .hint_text(format!(
                        "{}  Search dictations",
                        theme::icons::MAGNIFYING_GLASS
                    ))
                    .desired_width(240.0),
            );
            if self.focus_search {
                response.request_focus();
                self.focus_search = false;
            }
            if response.changed() {
                self.pages = 1;
            }

            let count = if self.search.trim().is_empty() {
                plural(self.total, "dictation")
            } else {
                plural(self.matching, "match")
            };
            ui.label(RichText::new(count).weak());

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // Quiet danger button: destructive intent named, no fill.
                let clear = egui::Button::new(RichText::new("Clear all").color(theme::DANGER))
                    .fill(Color32::TRANSPARENT);
                if ui.add_enabled(self.total > 0, clear).clicked() {
                    self.confirm = Some(widgets::Confirm::new(
                        "Clear history?",
                        format!(
                            "Deletes {} from this device. Lifetime stats are untouched.",
                            plural(self.total, "entry")
                        ),
                        format!("Delete {}", plural(self.total, "entry")),
                    ));
                }
            });
        });
        ui.add_space(4.0);
    }

    fn list(&mut self, ui: &mut Ui, storage: &StorageHandle) {
        let now_ms = jiff::Timestamp::now().as_millisecond();
        let mut action = None;
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut last_day: Option<String> = None;
                for entry in &self.entries {
                    let day = format::day_label(entry.ts_ms, now_ms, &self.tz);
                    if last_day.as_deref() != Some(day.as_str()) {
                        ui.add_space(if last_day.is_some() { 16.0 } else { 2.0 });
                        ui.label(RichText::new(&day).text_style(theme::subheading()).weak());
                        ui.add_space(2.0);
                        last_day = Some(day);
                    }
                    let expanded = self.expanded == Some(entry.id);
                    let copied = self.copied == Some(entry.id);
                    if let Some(a) = row::show(ui, entry, expanded, copied, now_ms, &self.tz) {
                        action = Some(a);
                    }
                }
                if (self.entries.len() as u64) < self.matching {
                    ui.add_space(8.0);
                    ui.vertical_centered(|ui| {
                        if ui.button("Show more").clicked() {
                            self.pages += 1;
                        }
                    });
                }
            });
        self.apply(action, ui, storage);
    }

    fn apply(&mut self, action: Option<row::Action>, ui: &Ui, storage: &StorageHandle) {
        match action {
            Some(row::Action::Toggle(id)) => {
                self.expanded = if self.expanded == Some(id) {
                    None
                } else {
                    Some(id)
                };
            }
            Some(row::Action::Copy(id)) => {
                if let Some(entry) = self.entries.iter().find(|e| e.id == id) {
                    ui.ctx().copy_text(entry.final_text.clone());
                    self.copied = Some(id);
                    // Snap the affirmation to full opacity; it fades from
                    // here (row::show drives the decay). Stale invisible
                    // state is harmless, so it is never explicitly cleared.
                    ui.ctx()
                        .animate_value_with_time(row::copied_id(id), 1.0, 0.0);
                }
            }
            Some(row::Action::Delete(id)) => {
                storage.send(StorageCmd::DeleteEntry(id));
                if self.expanded == Some(id) {
                    self.expanded = None;
                }
            }
            None => {}
        }
    }
}

/// "1 dictation" / "214 dictations" / "1 match" / "3 matches" / "5 entries".
fn plural(n: u64, noun: &str) -> String {
    let formatted = format::count(n as i64);
    if n == 1 {
        return format!("{formatted} {noun}");
    }
    match noun {
        "match" => format!("{formatted} matches"),
        "entry" => format!("{formatted} entries"),
        _ => format!("{formatted} {noun}s"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plural_handles_the_irregular_nouns() {
        assert_eq!(plural(1, "dictation"), "1 dictation");
        assert_eq!(plural(2, "dictation"), "2 dictations");
        assert_eq!(plural(1, "match"), "1 match");
        assert_eq!(plural(3, "match"), "3 matches");
        assert_eq!(plural(1, "entry"), "1 entry");
        assert_eq!(plural(1_214, "entry"), "1,214 entries");
    }
}

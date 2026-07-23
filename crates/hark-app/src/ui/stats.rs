//! The Stats panel (spec §3.7/§3.11): gated until 10 dictations (a progress
//! placeholder, never a zeroed dashboard), then 2x2 lifetime stat cards, a
//! time-saved line, the since-date, and Reset stats behind its own confirm.

use crate::storage::{StorageCmd, StorageHandle};
use crate::theme;
use crate::ui::{format, widgets};
use egui::{CornerRadius, Frame, Margin, ProgressBar, RichText, Ui};
use hark_store::Stats;
use jiff::tz::TimeZone;

/// Dictations required before numbers mean anything.
const UNLOCK_AT: i64 = 10;

pub struct StatsPage {
    stats: Option<Stats>,
    /// Write generation the cached row reflects.
    loaded: Option<u64>,
    fetch_error: Option<String>,
    confirm: Option<widgets::Confirm>,
    tz: TimeZone,
}

impl StatsPage {
    pub fn new() -> Self {
        StatsPage {
            stats: None,
            loaded: None,
            fetch_error: None,
            confirm: None,
            tz: TimeZone::system(),
        }
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        storage: Option<&StorageHandle>,
        unavailable: Option<&str>,
    ) {
        let Some(storage) = storage else {
            widgets::empty_state(
                ui,
                theme::icons::WARNING,
                "Stats are unavailable.",
                unavailable.unwrap_or("The local database could not be opened."),
            );
            return;
        };
        self.refresh(storage);

        let Some(stats) = self.stats else {
            let detail = self.fetch_error.clone();
            widgets::empty_state(
                ui,
                theme::icons::WARNING,
                "Stats cannot be read.",
                detail
                    .as_deref()
                    .unwrap_or("The local database did not answer."),
            );
            return;
        };

        if stats.dictations < UNLOCK_AT {
            gate(ui, stats.dictations);
            return;
        }

        cards(ui, &stats);
        ui.add_space(10.0);
        ui.label(format!(
            "About {} saved vs typing at 40 WPM.",
            format::duration(format::time_saved_ms(stats.words, stats.audio_ms))
        ));
        ui.label(
            RichText::new(format!(
                "Since {}.",
                format::date(stats.since_ts_ms, &self.tz)
            ))
            .small()
            .weak(),
        );

        ui.add_space(16.0);
        // Danger-outlined button; the confirm names what survives (§3.3
        // independence rule: reset never touches history entries).
        if ui.add(theme::danger_button("Reset stats")).clicked() {
            self.confirm = Some(widgets::Confirm::new(
                "Reset stats?",
                "Sets every counter to zero and restarts the since-date. \
                 Your history entries are untouched.",
                "Reset stats",
            ));
        }
        if let Some(confirm) = &mut self.confirm {
            match confirm.show(ui, "stats-reset") {
                Some(true) => {
                    storage.send(StorageCmd::ResetStats);
                    self.confirm = None;
                }
                Some(false) => self.confirm = None,
                None => {}
            }
        }
    }

    fn refresh(&mut self, storage: &StorageHandle) {
        let generation = storage.generation();
        if self.loaded == Some(generation) {
            return;
        }
        match storage.reader().stats() {
            Ok(stats) => {
                self.stats = Some(stats);
                self.fetch_error = None;
            }
            Err(e) => {
                log::error!("stats query failed: {e}");
                self.stats = None;
                self.fetch_error = Some(e.to_string());
            }
        }
        self.loaded = Some(generation);
    }
}

/// Unlock gate: progress toward the threshold, never a zeroed dashboard.
fn gate(ui: &mut Ui, dictations: i64) {
    widgets::empty_state(
        ui,
        theme::icons::CHART_BAR,
        &format!("{dictations} of {UNLOCK_AT} dictations to unlock stats"),
        "Numbers appear once there is enough signal to mean something.",
    );
    ui.add_space(10.0);
    ui.vertical_centered(|ui| {
        ui.add(
            ProgressBar::new(dictations as f32 / UNLOCK_AT as f32)
                .desired_width(220.0)
                .desired_height(6.0)
                .fill(theme::accent_fill(ui.visuals())),
        );
    });
}

/// 2x2 lifetime cards: dictations, words, speaking time, average
/// release-to-inject (derived from the migration-002 sum).
fn cards(ui: &mut Ui, stats: &Stats) {
    let gap = ui.spacing().item_spacing.x;
    let width = ((ui.available_width() - gap) / 2.0).clamp(140.0, 340.0);
    ui.horizontal(|ui| {
        card(
            ui,
            width,
            &format::count(stats.dictations),
            "Dictations",
            false,
        );
        card(ui, width, &format::count(stats.words), "Words", false);
    });
    ui.horizontal(|ui| {
        card(
            ui,
            width,
            &format::duration(stats.audio_ms),
            "Speaking time",
            false,
        );
        let (avg, mono) = match average_total_ms(stats) {
            // Latency reads in ms, mono font (§3.10 copy voice).
            Some(avg) => (format!("{avg} ms"), true),
            // An upgraded pre-002 database has no total sum yet; an average
            // of zero would be a lie, so admit there is no data.
            None => ("n/a".to_string(), false),
        };
        card(ui, width, &avg, "Avg release-to-inject", mono);
    });
}

fn card(ui: &mut Ui, width: f32, value: &str, label: &str, mono: bool) {
    Frame::default()
        .fill(ui.visuals().faint_bg_color)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(width - 28.0);
            // Big value: Inter Medium 26px, or JetBrains Mono 23px for the
            // latency figure (§3.10 copy voice).
            let value = if mono {
                RichText::new(value).monospace().size(23.0)
            } else {
                RichText::new(value)
                    .text_style(theme::subheading())
                    .size(26.0)
            };
            ui.label(value);
            ui.label(RichText::new(label).small().weak());
        });
}

/// Average release-to-inject; `None` until the total sum carries data
/// (pre-migration-002 rows contribute 0).
fn average_total_ms(stats: &Stats) -> Option<i64> {
    (stats.total_ms > 0 && stats.dictations > 0).then(|| stats.total_ms / stats.dictations)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(dictations: i64, total_ms: i64) -> Stats {
        Stats {
            dictations,
            words: 100,
            audio_ms: 60_000,
            stt_ms: 4_000,
            cleanup_ms: 1_000,
            total_ms,
            since_ts_ms: 1_000,
        }
    }

    #[test]
    fn average_derives_from_sums_and_admits_missing_data() {
        assert_eq!(average_total_ms(&stats(10, 8_000)), Some(800));
        assert_eq!(
            average_total_ms(&stats(10, 0)),
            None,
            "a pre-002 database must not claim a 0 ms average"
        );
        assert_eq!(
            average_total_ms(&stats(0, 0)),
            None,
            "never divides by zero"
        );
    }
}

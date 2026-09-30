//! Personal Insights backed by asynchronous local aggregates.
mod charts;
mod panels;

use crate::storage::{StorageCmd, StorageHandle};
use crate::theme;
use crate::ui::{format, insights_cache::InsightsCache, widgets};
use egui::{RichText, Ui};
use hark_config::Settings;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Overview,
    Voice,
    Performance,
}

pub struct StatsPage {
    cache: InsightsCache,
    days: u16,
    tab: Tab,
    confirm: Option<widgets::Confirm>,
}

impl StatsPage {
    pub fn new() -> Self {
        Self {
            cache: InsightsCache::new(),
            days: 30,
            tab: Tab::Overview,
            confirm: None,
        }
    }

    /// True opens privacy settings; opt-ins keep save/discard semantics.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        storage: Option<&StorageHandle>,
        unavailable: Option<&str>,
        settings: &Settings,
    ) -> bool {
        let Some(storage) = storage else {
            widgets::empty_state(
                ui,
                theme::icons::WARNING,
                "Insights are unavailable.",
                unavailable.unwrap_or("The local database could not be opened."),
            );
            return false;
        };
        let mut open_privacy = false;
        ui.horizontal_wrapped(|ui| {
            for (tab, label) in [
                (Tab::Overview, "Overview"),
                (Tab::Voice, "Your voice"),
                (Tab::Performance, "Performance"),
            ] {
                if theme::nav_button(ui, label, self.tab == tab).clicked() {
                    self.tab = tab;
                }
            }
            egui::ComboBox::from_id_salt("insights-period")
                .selected_text(format!("Last {} days", self.days))
                .show_ui(ui, |ui| {
                    for days in [7, 30, 90] {
                        ui.selectable_value(&mut self.days, days, format!("Last {days} days"));
                    }
                });
        });
        ui.separator();
        ui.add_space(theme::ROW_GAP);
        self.cache.refresh(
            ui.ctx(),
            storage,
            self.days,
            settings.insights.analyze_text && self.tab == Tab::Voice,
        );
        if let Some(error) = &self.cache.error {
            ui.label(RichText::new(error).color(theme::danger(ui.visuals())));
            if ui.button("Retry insights").clicked() {
                self.cache.retry();
            }
            return false;
        }
        let Some(data) = &self.cache.data else {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Reading your local insights…");
            });
            return false;
        };
        if data.lifetime.dictations < 10 {
            theme::card(ui, |ui| {
                ui.label(RichText::new("Your voice has a story.").font(theme::hero_font()));
                ui.label(format!(
                    "{} of 10 dictations. A few more will make these insights useful.",
                    data.lifetime.dictations
                ));
                ui.add(egui::ProgressBar::new(
                    data.lifetime.dictations.max(0) as f32 / 10.0,
                ));
            });
        }
        if !data.coverage.period_complete {
            ui.label(RichText::new("Partial history: earlier days may be missing. New numeric tracking builds a complete picture from now on.").small().weak());
            ui.add_space(theme::GAP);
        }
        match self.tab {
            Tab::Overview => {
                panels::overview_metrics(ui, &data.period);
                ui.add_space(theme::ROW_GAP);
                theme::card(ui, |ui| {
                    ui.label(
                        RichText::new("Your words, over time").text_style(theme::subheading()),
                    );
                    charts::daily(ui, &data.daily);
                });
                ui.add_space(theme::ROW_GAP);
                if ui.available_width() >= theme::MIN_CARD_WIDTH * 2.0 + theme::ROW_GAP {
                    ui.columns(2, |columns| {
                        theme::card(&mut columns[0], |ui| charts::activity(ui, data));
                        theme::card(&mut columns[1], |ui| {
                            open_privacy |= panels::apps(ui, data, settings)
                        });
                    });
                } else {
                    theme::card(ui, |ui| charts::activity(ui, data));
                    ui.add_space(theme::ROW_GAP);
                    theme::card(ui, |ui| open_privacy |= panels::apps(ui, data, settings));
                }
            }
            Tab::Voice => open_privacy |= panels::voice(ui, data, settings),
            Tab::Performance => panels::performance(ui, data),
        }
        ui.add_space(theme::SECTION_GAP);
        egui::CollapsingHeader::new("Lifetime totals")
            .id_salt("insights-lifetime")
            .show(ui, |ui| {
                panels::lifetime(ui, &data.lifetime);
                ui.label(
                    RichText::new(format!(
                        "Since {}. Clearing history preserves these totals.",
                        format::date(data.lifetime.since_ts_ms, &self.cache.tz)
                    ))
                    .small()
                    .weak(),
                );
            });
        ui.add_space(theme::ROW_GAP);
        ui.label(RichText::new("Numeric details stay on this device for 366 days, independently of transcript history. Lifetime totals remain until reset.").small().weak());
        ui.label(RichText::new("Dictated words exclude invocation expansion length. Pace uses clip duration, including capture padding.").small().weak());
        ui.add_space(theme::GAP);
        if ui
            .add(theme::danger_button(ui.visuals(), "Reset stats"))
            .clicked()
        {
            self.confirm = Some(widgets::Confirm::new("Reset stats?", "Clear lifetime counters and all detailed numeric insights, including saved app labels. Your transcripts, Spellbook and invocations are untouched. Word insights can still be calculated from retained history when enabled.", "Reset stats"));
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
        open_privacy
    }
}

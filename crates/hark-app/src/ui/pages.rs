//! Page routing. Every page is a real editor or panel as of CP4; each one
//! still ships honest empty, gated, and error states (a blank region is a
//! bug).

use crate::pipeline::PipelineController;
use crate::storage::StorageHandle;
use crate::theme;
use crate::ui::dictionary::DictionaryPage;
use crate::ui::history::HistoryPage;
use crate::ui::settings::{self, SettingsPage};
use crate::ui::stats::StatsPage;
use crate::update::Updater;
use hark_config::Settings;

use egui::{RichText, Ui};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    History,
    Dictionary,
    Stats,
    Settings,
}

impl Page {
    pub fn label(self) -> &'static str {
        match self {
            Page::History => "History",
            Page::Dictionary => "Dictionary",
            Page::Stats => "Stats",
            Page::Settings => "Settings",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Page::History => theme::icons::CLOCK_COUNTER_CLOCKWISE,
            Page::Dictionary => theme::icons::BOOK_OPEN,
            Page::Stats => theme::icons::CHART_BAR,
            Page::Settings => theme::icons::GEAR,
        }
    }

    fn description(self) -> &'static str {
        match self {
            Page::History => "Your dictations, newest first. Everything stays on this device.",
            Page::Dictionary => "Names and terms your STT provider keeps missing.",
            Page::Stats => "Lifetime dictation figures. They survive a history clear.",
            Page::Settings => "Provider, key, hotkey, and voice.",
        }
    }
}

/// Per-page UI state, owned by `HarkApp`, grouped so the shell signature
/// stays readable as pages accumulate.
pub struct Views {
    pub settings: SettingsPage,
    pub dictionary: DictionaryPage,
    pub history: HistoryPage,
    pub stats: StatsPage,
}

/// Content column widths (spec §3.11): narrow for forms, wider for lists.
fn max_width(page: Page) -> f32 {
    match page {
        Page::Settings => 560.0,
        _ => 720.0,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut Ui,
    page: Page,
    settings: &mut Settings,
    pipeline: &mut PipelineController,
    views: &mut Views,
    updater: &mut Updater,
    storage: Option<&StorageHandle>,
    storage_error: Option<&str>,
) {
    let column = max_width(page).min(ui.available_width());
    let pad = ((ui.available_width() - column) / 2.0).max(0.0);
    ui.horizontal_top(|ui| {
        ui.add_space(pad);
        ui.vertical(|ui| {
            ui.set_max_width(column);
            ui.heading(page.label());
            ui.label(RichText::new(page.description()).weak());
            ui.add_space(14.0);
            match page {
                Page::History => {
                    views
                        .history
                        .show(ui, storage, storage_error, &settings.hotkey.ptt_key)
                }
                Page::Dictionary => dictionary(ui, settings, pipeline, views),
                Page::Stats => views.stats.show(ui, storage, storage_error),
                Page::Settings => {
                    // Long forms need a scroll container; the sidebar and
                    // footer stay put.
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.set_max_width(column);
                            views.settings.show(ui, settings, pipeline, updater);
                        });
                }
            }
        });
    });
}

/// Dictionary edits persist immediately and restart the pipeline (bias
/// terms are baked in at start). The settings draft mirrors the change so a
/// later Save does not resurrect deleted terms.
fn dictionary(
    ui: &mut Ui,
    settings: &mut Settings,
    pipeline: &mut PipelineController,
    views: &mut Views,
) {
    if views.dictionary.show(ui, &mut settings.dictionary.terms) {
        views
            .dictionary
            .set_notice(settings::save_to_disk(settings).err());
        pipeline.start(settings, ui.ctx());
        views.settings.draft.dictionary = settings.dictionary.clone();
    }
}

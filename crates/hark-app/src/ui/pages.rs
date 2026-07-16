//! Page routing and the CP2-era placeholders that remain (history and
//! stats land at CP4). Settings and Dictionary are real editors now; every
//! page still ships an honest empty state (a blank region is a bug).

use crate::pipeline::PipelineController;
use crate::theme;
use crate::ui::dictionary::DictionaryPage;
use crate::ui::settings::{self, SettingsPage};
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

/// Content column widths (spec §3.11): narrow for forms, wider for lists.
fn max_width(page: Page) -> f32 {
    match page {
        Page::Settings => 560.0,
        _ => 720.0,
    }
}

pub fn show(
    ui: &mut Ui,
    page: Page,
    settings: &mut Settings,
    pipeline: &mut PipelineController,
    settings_page: &mut SettingsPage,
    dictionary_page: &mut DictionaryPage,
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
                Page::History => history(ui, settings),
                Page::Dictionary => {
                    dictionary(ui, settings, pipeline, settings_page, dictionary_page)
                }
                Page::Stats => stats(ui),
                Page::Settings => {
                    // Long forms need a scroll container; the sidebar and
                    // footer stay put.
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.set_max_width(column);
                            settings_page.show(ui, settings, pipeline);
                        });
                }
            }
        });
    });
}

fn history(ui: &mut Ui, settings: &Settings) {
    empty_state(
        ui,
        theme::icons::CLOCK_COUNTER_CLOCKWISE,
        "Dictations appear here.",
        &format!(
            "Hold {} and speak into any text field.",
            settings.hotkey.ptt_key
        ),
    );
}

/// Dictionary edits persist immediately and restart the pipeline (bias
/// terms are baked in at start). The settings draft mirrors the change so a
/// later Save does not resurrect deleted terms.
fn dictionary(
    ui: &mut Ui,
    settings: &mut Settings,
    pipeline: &mut PipelineController,
    settings_page: &mut SettingsPage,
    dictionary_page: &mut DictionaryPage,
) {
    if dictionary_page.show(ui, &mut settings.dictionary.terms) {
        dictionary_page.set_notice(settings::save_to_disk(settings).err());
        pipeline.start(settings, ui.ctx());
        settings_page.draft.dictionary = settings.dictionary.clone();
    }
}

fn stats(ui: &mut Ui) {
    empty_state(
        ui,
        theme::icons::CHART_BAR,
        "Stats unlock after 10 dictations.",
        "No zeroed dashboards; numbers appear once there is enough signal.",
    );
}

/// Centered empty state: icon, one-line title, weak caption. Every panel
/// ships one (state coverage rule, spec §3.11).
fn empty_state(ui: &mut Ui, icon: &str, title: &str, caption: &str) {
    ui.add_space(56.0);
    ui.vertical_centered(|ui| {
        ui.label(
            RichText::new(icon)
                .size(40.0)
                .color(ui.visuals().weak_text_color()),
        );
        ui.add_space(6.0);
        ui.label(RichText::new(title).text_style(theme::subheading()));
        ui.label(RichText::new(caption).weak());
    });
}

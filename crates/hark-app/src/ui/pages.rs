//! The four pages in their CP2 form: designed headers and honest empty
//! states. The real editors and lists land at CP3 (settings, dictionary)
//! and CP4 (history, stats); a blank region is a bug, so every page states
//! what it is and what comes next.

use crate::theme;
use hark_config::Settings;
use hark_keychain::KeyStatus;

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

pub fn show(ui: &mut Ui, page: Page, settings: &Settings, key_status: &KeyStatus) {
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
                Page::Dictionary => dictionary(ui, settings),
                Page::Stats => stats(ui),
                Page::Settings => settings_summary(ui, settings, key_status),
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

fn dictionary(ui: &mut Ui, settings: &Settings) {
    if settings.dictionary.terms.is_empty() {
        empty_state(
            ui,
            theme::icons::BOOK_OPEN,
            "No dictionary terms yet.",
            "The editor arrives with the settings form in the next update.",
        );
        return;
    }
    ui.label(
        RichText::new(
            "Corrections apply on this device after transcription; entries are also sent \
             to your STT provider as accuracy hints.",
        )
        .weak(),
    );
    ui.add_space(6.0);
    for term in &settings.dictionary.terms {
        ui.label(RichText::new(term).monospace());
    }
    ui.add_space(10.0);
    ui.label(RichText::new("Editing arrives in the next update.").weak());
}

fn stats(ui: &mut Ui) {
    empty_state(
        ui,
        theme::icons::CHART_BAR,
        "Stats unlock after 10 dictations.",
        "No zeroed dashboards; numbers appear once there is enough signal.",
    );
}

/// Read-only summary until the CP3 form lands. The key row reads the OS
/// keychain status captured at startup, never key material.
fn settings_summary(ui: &mut Ui, settings: &Settings, key_status: &KeyStatus) {
    summary_row(
        ui,
        "Provider",
        &format!(
            "{} · {}",
            settings.provider.kind.label(),
            settings.provider.resolved_model()
        ),
    );
    let (key_icon, key_color, key_text) = match key_status {
        KeyStatus::Stored => (
            theme::icons::CHECK,
            theme::SUCCESS,
            "Key stored".to_string(),
        ),
        KeyStatus::Missing => (theme::icons::KEY, theme::WARNING, "No key yet".to_string()),
        KeyStatus::Backend(detail) => (
            theme::icons::WARNING,
            theme::DANGER,
            format!("Keychain unavailable: {detail}"),
        ),
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new("Key").weak());
        ui.label(RichText::new(key_icon).color(key_color));
        ui.label(key_text);
    });
    summary_row(ui, "Hotkey", &settings.hotkey.ptt_key);
    summary_row(ui, "Voice", settings.voice.default.label());
    ui.add_space(14.0);
    ui.label(
        RichText::new(
            "The full form (provider picker, key entry, test connection) is the next \
             checkpoint. Until then, edit config.toml and store keys via the keychain.",
        )
        .weak(),
    );
}

fn summary_row(ui: &mut Ui, name: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(name).weak());
        ui.label(value);
    });
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

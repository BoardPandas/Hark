//! Page routing. Every page is a real editor or panel as of CP4; each one
//! still ships honest empty, gated, and error states (a blank region is a
//! bug).

use crate::meeting::MeetingController;
use crate::pipeline::PipelineController;
use crate::storage::StorageHandle;
use crate::ui::history::HistoryPage;
use crate::ui::home::HomePage;
use crate::ui::invocations::InvocationsPage;
use crate::ui::meetings::{MeetingsPage, PageIntent};
use crate::ui::settings::{self, SettingsPage};
use crate::ui::spellbook::SpellbookPage;
use crate::ui::stats::StatsPage;
use crate::update::Updater;
use hark_config::Settings;

use crate::theme;
use egui::{RichText, Ui};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Home,
    History,
    Meetings,
    Spellbook,
    Invocations,
    Stats,
    Settings,
}

impl Page {
    pub const ALL: [Self; 7] = [
        Self::Home,
        Self::Stats,
        Self::History,
        Self::Meetings,
        Self::Spellbook,
        Self::Invocations,
        Self::Settings,
    ];

    pub fn icon(self) -> &'static str {
        match self {
            Self::Home => theme::icons::HOME,
            Self::Stats => theme::icons::CHART_BAR,
            Self::History => theme::icons::CLOCK_COUNTER_CLOCKWISE,
            Self::Meetings => theme::icons::MICROPHONE,
            Self::Spellbook => theme::icons::BOOK_OPEN,
            Self::Invocations => theme::icons::LIGHTNING,
            Self::Settings => theme::icons::GEAR,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Page::Home => "Home",
            Page::History => "History",
            Page::Meetings => "Meetings",
            Page::Spellbook => "Spellbook",
            Page::Invocations => "Invocations",
            Page::Stats => "Insights",
            Page::Settings => "Settings",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Page::Home => "Make room for your next idea.",
            Page::History => "Your words, ready when you need them. History stays on this device.",
            Page::Meetings => "Every call, written down. Notes and recordings stay on this device.",
            Page::Spellbook => "A little context. A lot more accuracy.",
            Page::Invocations => "Say a phrase. Type exactly what you wrote.",
            Page::Stats => "A little less typing. A little more time.",
            Page::Settings => "Set it up once. Stay in your flow.",
        }
    }
}

/// Per-page UI state, owned by `HarkApp`, grouped so the shell signature
/// stays readable as pages accumulate.
pub struct Views {
    pub home: HomePage,
    pub settings: SettingsPage,
    pub spellbook: SpellbookPage,
    pub invocations: InvocationsPage,
    pub history: HistoryPage,
    pub stats: StatsPage,
    pub meetings: MeetingsPage,
}

#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut Ui,
    page: &mut Page,
    settings: &mut Settings,
    pipeline: &mut PipelineController,
    meetings: &mut MeetingController,
    views: &mut Views,
    updater: &mut Updater,
    storage: Option<&StorageHandle>,
    storage_error: Option<&str>,
) {
    let column = theme::CONTENT_WIDTH.min(ui.available_width());
    let pad = ((ui.available_width() - column) / 2.0).max(0.0);
    ui.horizontal_top(|ui| {
        ui.add_space(pad);
        ui.vertical(|ui| {
            ui.set_max_width(column);
            if *page != Page::Home {
                ui.heading(if *page == Page::Stats {
                    "Small habits. Big impact."
                } else {
                    page.label()
                });
                ui.label(RichText::new(page.description()).weak());
                ui.add_space(theme::SECTION_GAP);
            }
            match *page {
                Page::Home => {
                    if let Some(target) =
                        views
                            .home
                            .show(ui, settings, pipeline.status(), storage, storage_error)
                    {
                        *page = target;
                        if target == Page::Settings {
                            views.settings.open(settings::Section::Audio);
                        }
                    }
                }
                Page::History => {
                    // Adding from a history selection is a two-page gesture:
                    // the term is captured here and finished in the Spellbook,
                    // where the user replaces the misheard spelling with the
                    // right one. Navigating is what the user asked for; it
                    // does cost their place in the list, which is the known
                    // trade recorded in the plan.
                    if let Some(term) =
                        views
                            .history
                            .show(ui, storage, storage_error, &settings.hotkey.ptt_key)
                    {
                        views.spellbook.prime_add(term);
                        *page = Page::Spellbook;
                    }
                }
                Page::Meetings => {
                    match views
                        .meetings
                        .show(ui, meetings, settings, storage, storage_error)
                    {
                        Some(PageIntent::OpenSettings) => {
                            *page = Page::Settings;
                            views.settings.open(settings::Section::Meetings);
                        }
                        Some(PageIntent::AcknowledgeConsent) => {
                            settings.meeting.consent_acknowledged = true;
                            // Same obligation as the spellbook: the Settings
                            // draft must not resurrect the old value on Save.
                            views.settings.draft.meeting.consent_acknowledged = true;
                            if let Err(e) = settings::save_to_disk(settings) {
                                log::error!("consent acknowledgement not persisted: {e}");
                            }
                        }
                        None => {}
                    }
                }
                Page::Spellbook => spellbook(ui, settings, pipeline, views),
                Page::Invocations => invocations(ui, settings, pipeline, views),
                Page::Stats => {
                    egui::ScrollArea::vertical()
                        .id_salt("lifetime-stats")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            egui::Frame::new()
                                .inner_margin(theme::SHADOW_MARGIN)
                                .show(ui, |ui| {
                                    if views.stats.show(ui, storage, storage_error, settings) {
                                        *page = Page::Settings;
                                        views.settings.open(settings::Section::Privacy);
                                    }
                                });
                        });
                }
                Page::Settings => {
                    views
                        .settings
                        .show(ui, settings, pipeline, updater, meetings, storage);
                }
            }
        });
    });
}

/// Spellbook edits persist immediately and restart the pipeline (bias
/// terms are baked in at start). The settings draft mirrors the change so a
/// later Save does not resurrect deleted terms.
fn spellbook(
    ui: &mut Ui,
    settings: &mut Settings,
    pipeline: &mut PipelineController,
    views: &mut Views,
) {
    if views.spellbook.show(ui, &mut settings.spellbook.entries) {
        views
            .spellbook
            .set_notice(settings::save_to_disk(settings).err());
        pipeline.start(settings, ui.ctx());
        views.settings.draft.spellbook = settings.spellbook.clone();
    }
}

/// Invocation edits persist immediately and restart the pipeline (the
/// trigger matcher is built at pipeline start). Same four obligations as
/// `spellbook`, in the same order.
fn invocations(
    ui: &mut Ui,
    settings: &mut Settings,
    pipeline: &mut PipelineController,
    views: &mut Views,
) {
    if views.invocations.show(ui, &mut settings.invocations) {
        views
            .invocations
            .set_notice(settings::save_to_disk(settings).err());
        pipeline.start(settings, ui.ctx());
        // Load-bearing. The Settings page edits a *draft* copy of the whole
        // Settings struct and writes it wholesale on Save. Without this
        // line that draft still holds the pre-edit invocations, so opening
        // Settings and pressing Save would resurrect every deleted
        // invocation -- silent data loss with no error to notice.
        views.settings.draft.invocations = settings.invocations.clone();
    }
}

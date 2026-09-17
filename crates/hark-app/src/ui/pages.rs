//! Page routing. Every page is a real editor or panel as of CP4; each one
//! still ships honest empty, gated, and error states (a blank region is a
//! bug).

use crate::pipeline::PipelineController;
use crate::storage::StorageHandle;
use crate::ui::history::HistoryPage;
use crate::ui::invocations::InvocationsPage;
use crate::ui::settings::{self, SettingsPage};
use crate::ui::spellbook::SpellbookPage;
use crate::ui::stats::StatsPage;
use crate::update::Updater;
use hark_config::Settings;

use crate::theme;
use egui::{RichText, Ui};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    History,
    Spellbook,
    Invocations,
    Stats,
    Settings,
}

impl Page {
    pub fn label(self) -> &'static str {
        match self {
            Page::History => "History",
            Page::Spellbook => "Spellbook",
            Page::Invocations => "Invocations",
            Page::Stats => "Stats",
            Page::Settings => "Settings",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Page::History => "Your words, ready when you need them. History stays on this device.",
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
    pub settings: SettingsPage,
    pub spellbook: SpellbookPage,
    pub invocations: InvocationsPage,
    pub history: HistoryPage,
    pub stats: StatsPage,
}

#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut Ui,
    page: &mut Page,
    settings: &mut Settings,
    pipeline: &mut PipelineController,
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
            ui.heading(page.label());
            ui.label(RichText::new(page.description()).weak());
            ui.add_space(theme::SECTION_GAP);
            match *page {
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
                Page::Spellbook => spellbook(ui, settings, pipeline, views),
                Page::Invocations => invocations(ui, settings, pipeline, views),
                Page::Stats => {
                    egui::ScrollArea::vertical()
                        .id_salt("lifetime-stats")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            egui::Frame::new()
                                .inner_margin(theme::SHADOW_MARGIN)
                                .show(ui, |ui| views.stats.show(ui, storage, storage_error));
                        });
                }
                Page::Settings => {
                    views.settings.show(ui, settings, pipeline, updater);
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

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

    /// The page's category hue (spec §6). Settings has none and uses accent.
    pub fn hue(self) -> Option<theme::Hue> {
        match self {
            Self::Home => Some(theme::Hue::Green),
            Self::Stats => Some(theme::Hue::Blue),
            Self::History => Some(theme::Hue::Gold),
            Self::Meetings => Some(theme::Hue::Teal),
            Self::Spellbook => Some(theme::Hue::Violet),
            Self::Invocations => Some(theme::Hue::Rose),
            Self::Settings => None,
        }
    }

    /// The color the page's icon is drawn in, wherever it appears.
    pub fn ink(self, v: &egui::Visuals) -> egui::Color32 {
        self.hue()
            .map_or_else(|| theme::accent(v), |hue| theme::hue(v, hue))
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
                header(ui, *page);
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

/// The page icon in its hue, the title, a short hue rule, the description.
/// The icon and rule are decorative: the title says which page this is.
fn header(ui: &mut Ui, page: Page) {
    let ink = page.ink(ui.visuals());
    ui.horizontal(|ui| {
        ui.label(
            theme::icon_text(page.icon())
                .size(theme::PAGE_ICON_SIZE)
                .color(ink),
        );
        ui.heading(if page == Page::Stats {
            "Small habits. Big impact."
        } else {
            page.label()
        });
    });
    let (rule, _) = ui.allocate_exact_size(
        egui::vec2(theme::HEADER_RULE_WIDTH, theme::HEADER_RULE_HEIGHT),
        egui::Sense::hover(),
    );
    ui.painter().rect_filled(rule, 1.0, ink);
    ui.label(RichText::new(page.description()).weak());
    ui.add_space(theme::SECTION_GAP);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Spec AC14 (T16): six hued pages, six different hues, and Settings in
    /// accent instead.
    #[test]
    fn each_page_but_settings_has_its_own_hue() {
        let hues: Vec<theme::Hue> = Page::ALL.iter().filter_map(|page| page.hue()).collect();
        assert_eq!(hues.len(), 6);
        for (i, hue) in hues.iter().enumerate() {
            assert!(!hues[i + 1..].contains(hue), "{hue:?} is used twice");
        }
        assert_eq!(Page::Settings.hue(), None);
        let v = egui::Visuals::light();
        assert_eq!(Page::Settings.ink(&v), theme::accent(&v));
    }

    /// Every nav button draws its icon in the page's ink and its label in the
    /// widget's own text color, in every theme.
    #[test]
    fn nav_icons_wear_their_page_hue_in_every_theme() {
        for appearance in theme::Appearance::ALL {
            let ctx = egui::Context::default();
            theme::apply(&ctx);
            theme::set_appearance(&ctx, appearance);
            let mut page = Page::Home;
            let mut visuals = egui::Visuals::default();
            let mut jobs = Vec::new();
            for _ in 0..2 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1000.0, 700.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        visuals = ui.visuals().clone();
                        crate::ui::navigation::show(ui, &mut page);
                    },
                );
                output.textures_delta.clear();
                jobs = output
                    .shapes
                    .iter()
                    .filter_map(|clipped| match &clipped.shape {
                        egui::Shape::Text(t) => Some(t.galley.job.clone()),
                        _ => None,
                    })
                    .collect();
            }
            for target in Page::ALL {
                if target == Page::Meetings && !hark_pipeline::meeting::meetings_supported() {
                    continue;
                }
                let job = jobs
                    .iter()
                    .find(|job| job.text == format!("{}{}", target.icon(), target.label()))
                    .unwrap_or_else(|| {
                        panic!("{appearance:?}: no nav button for {}", target.label())
                    });
                assert_eq!(
                    job.sections[0].format.color,
                    target.ink(&visuals),
                    "{appearance:?} {}",
                    target.label()
                );
                assert_eq!(job.sections[1].format.color, egui::Color32::PLACEHOLDER);
            }
        }
    }
}

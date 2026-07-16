//! The Settings page (spec §3.6): a draft `Settings` edited in place, the
//! key section writing straight to the OS keychain, test-connection on a
//! background thread, and Save = validate -> persist TOML -> pipeline
//! restart. Failures leave the app running with the pipeline stopped and a
//! visible cause; never a silent dead state.

pub mod cleanup;
pub mod form;
pub mod get_started;
pub mod keys;
pub mod test;

use crate::pipeline::PipelineController;
use crate::theme;
use egui::{RichText, Ui};
use hark_config::Settings;

pub struct SettingsPage {
    /// The model being edited; `saved` (in `HarkApp`) only changes on Save.
    pub draft: Settings,
    bufs: form::FormBufs,
    stt_keys: keys::KeySection,
    cleanup_keys: Option<keys::KeySection>,
    test: test::TestConnection,
    get_started: get_started::GetStarted,
    /// Outcome of the last save; persists until the next one.
    save_notice: Option<Result<String, String>>,
}

impl SettingsPage {
    /// `onboarding` = no STT key resolved at startup (pipeline stopped,
    /// key-related): the Get Started card latches active.
    pub fn new(settings: &Settings, onboarding: bool) -> Self {
        SettingsPage {
            draft: settings.clone(),
            bufs: form::FormBufs::from_settings(settings),
            stt_keys: keys::KeySection::new("stt", settings.provider.kind.label()),
            cleanup_keys: None,
            test: test::TestConnection::new(),
            get_started: get_started::GetStarted::new(onboarding),
            save_notice: None,
        }
    }

    pub fn show(&mut self, ui: &mut Ui, saved: &mut Settings, pipeline: &mut PipelineController) {
        // The key section follows the draft's provider account.
        self.stt_keys.sync_account(self.draft.provider.kind.label());

        let card_view = get_started::view(
            &self.get_started,
            self.stt_keys.stored(),
            pipeline.is_running(),
            pipeline.injected_count() > 0,
        );
        if get_started::show(ui, &card_view, &self.draft.hotkey.ptt_key) {
            self.get_started.dismissed = true;
        }

        if form::provider_section(ui, &mut self.draft, &mut self.bufs) {
            self.get_started.provider_touched = true;
        }
        form::model_endpoint_section(ui, &mut self.draft, &mut self.bufs);
        // A stored or removed key takes effect now, not at the next Save:
        // the pipeline resolves keys only at start, so a key change for the
        // running provider restarts it immediately (else dictation keeps
        // failing against the old key with no sign why).
        if self.stt_keys.show(ui) && self.draft.provider.kind == saved.provider.kind {
            pipeline.start(saved, ui.ctx());
        }
        let test_finished = self.test.show(ui, &self.draft);
        form::hotkey_section(ui, &mut self.draft);
        form::voice_section(ui, &mut self.draft);
        cleanup::section(ui, &mut self.draft, &mut self.bufs, &mut self.cleanup_keys);
        form::behavior_section(ui, &mut self.draft);
        form::privacy_section(ui, &mut self.draft);

        ui.add_space(12.0);
        let save = egui::Button::new(RichText::new("Save").color(theme::ON_ACCENT))
            .fill(theme::accent_fill(ui.visuals()));
        if ui.add(save).clicked() {
            self.save(saved, pipeline, ui.ctx());
        }
        if let Some(notice) = &self.save_notice {
            let (icon, color, text) = match notice {
                Ok(t) => (theme::icons::CHECK, theme::SUCCESS, t.as_str()),
                Err(t) => (theme::icons::WARNING, theme::DANGER, t.as_str()),
            };
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(icon).color(color));
                ui.label(RichText::new(text).small());
            });
        }

        if test_finished {
            self.get_started.test_passed = self.test.stt_passed();
            // Onboarding promise (§3.11): a passing test flips the card to
            // "hold the chord and speak", so make that true by running the
            // save-restart flow the user has not learned to look for yet.
            if self.get_started.test_passed && self.get_started.active && !pipeline.is_running() {
                self.save(saved, pipeline, ui.ctx());
            }
        }
    }

    /// Validate -> persist TOML -> restart the pipeline (§3.6 Save).
    /// `Settings::save` validates before writing and writes atomically, so
    /// an invalid draft changes nothing on disk. `PipelineController::start`
    /// drops the old handle (hook, worker, capture, in order) before the new
    /// `run`, which is the restart-race ordering §5 requires.
    fn save(
        &mut self,
        saved: &mut Settings,
        pipeline: &mut PipelineController,
        ctx: &egui::Context,
    ) {
        match save_to_disk(&self.draft) {
            Ok(()) => {
                *saved = self.draft.clone();
                pipeline.start(saved, ctx);
                self.save_notice = Some(Ok(if pipeline.is_running() {
                    "Saved. Pipeline restarted.".to_string()
                } else {
                    "Saved. Pipeline stopped; see the status bar for the cause.".to_string()
                }));
            }
            Err(detail) => self.save_notice = Some(Err(detail)),
        }
    }
}

/// Persist settings to the OS config path. Shared with the dictionary
/// editor, which saves per change instead of behind a Save button.
pub(crate) fn save_to_disk(settings: &Settings) -> Result<(), String> {
    let Some(path) = hark_config::default_config_path() else {
        return Err("No OS config directory found; cannot save settings.".to_string());
    };
    settings.save(&path).map_err(|e| e.to_string())
}

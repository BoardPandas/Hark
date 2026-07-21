//! The Settings page (spec §3.6): a draft `Settings` edited in place, the
//! key section writing straight to the OS keychain, test-connection on a
//! background thread, and Save = validate -> persist TOML -> pipeline
//! restart. Failures leave the app running with the pipeline stopped and a
//! visible cause; never a silent dead state.

pub mod capture;
pub mod cleanup;
pub mod form;
pub mod get_started;
pub mod keys;
pub mod test;
pub mod updates;

use crate::pipeline::PipelineController;
use crate::theme;
use crate::update::Updater;
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
    /// Input-device names for the microphone picker, enumerated off the UI
    /// thread once at construction and re-scanned on demand (WASAPI COM must
    /// not be initialized on this thread; see `hark_audio::list_input_devices`).
    mic_devices: Vec<String>,
    /// The Windows Default *Communications* capture device, if any. Queried
    /// off the UI thread with the device list (it is a COM call), and cached
    /// for the same reason: the picker labels it so a user whose headset is
    /// the communications default can see why Hark and Teams disagree.
    comms_default: Option<String>,
    /// Cross-frame state for recording the push-to-talk shortcut.
    hotkey: capture::HotkeyCapture,
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
            mic_devices: hark_audio::list_input_devices(),
            comms_default: hark_audio::communications_default_device(),
            hotkey: capture::HotkeyCapture::new(),
        }
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        saved: &mut Settings,
        pipeline: &mut PipelineController,
        updater: &mut Updater,
    ) {
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
        // Recording a shortcut installs its own keyboard hook; only one may run
        // at a time, so the pipeline's push-to-talk hook stands down while
        // recording and resumes (with the still-saved chord) when it ends.
        let ctx = ui.ctx().clone();
        match form::hotkey_section(ui, &mut self.draft, &mut self.hotkey, &ctx) {
            capture::CaptureTransition::Started => pipeline.stop(),
            capture::CaptureTransition::Ended => pipeline.start(saved, &ctx),
            capture::CaptureTransition::None => {}
        }
        // The capture stream runs continuously while the pipeline is up, so
        // the meter is live here without starting anything extra.
        let level = pipeline.level_meter().map(|m| m.level());
        if form::mic_section(
            ui,
            &mut self.draft,
            &self.mic_devices,
            level,
            self.comms_default.as_deref(),
        ) {
            self.mic_devices = hark_audio::list_input_devices();
            self.comms_default = hark_audio::communications_default_device();
        }
        form::voice_section(ui, &mut self.draft);
        cleanup::section(ui, &mut self.draft, &mut self.bufs, &mut self.cleanup_keys);
        form::behavior_section(ui, &mut self.draft);
        form::privacy_section(ui, &mut self.draft);
        updates::section(ui, updater, &mut self.draft);

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

    /// Surface the outcome of a save that happened outside the form (a
    /// tray voice change while the window may be hidden): the notice waits
    /// under the Save button for the next time the page is seen.
    pub(crate) fn set_save_notice(&mut self, notice: Result<String, String>) {
        self.save_notice = Some(notice);
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
                // Bring the OS startup entry in line with the just-saved
                // toggle. Best-effort (logs on failure); never blocks the save.
                crate::app::reconcile_autostart(self.draft.startup.launch_at_login);
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

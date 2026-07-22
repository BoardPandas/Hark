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
pub mod local;
pub mod test;
pub mod updates;

use crate::model_download::ModelDownload;
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
    cleanup_test: cleanup::CleanupTest,
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
    /// On-device model download state. Lives here rather than in `HarkApp`
    /// because nothing outside Settings drives it.
    download: ModelDownload,
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
            cleanup_test: cleanup::CleanupTest::new(),
            test: test::TestConnection::new(),
            get_started: get_started::GetStarted::new(onboarding),
            save_notice: None,
            mic_devices: hark_audio::list_input_devices(),
            comms_default: hark_audio::communications_default_device(),
            hotkey: capture::HotkeyCapture::new(),
            // An unknown model id in config falls back to the catalog default
            // rather than failing construction; the section shows what it is.
            download: ModelDownload::new(
                hark_local_stt::find(&settings.local_stt.model)
                    .unwrap_or(&hark_local_stt::PARAKEET_V3_INT8),
            ),
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
        local::section(ui, &mut self.draft, &mut self.download);
        form::voice_section(ui, &mut self.draft);
        cleanup::section(
            ui,
            &mut self.draft,
            &mut self.bufs,
            &mut self.cleanup_keys,
            &mut self.cleanup_test,
        );
        form::behavior_section(ui, &mut self.draft);
        form::privacy_section(ui, &mut self.draft);
        updates::section(ui, updater, &mut self.draft);
        // Save lives in the sticky bar above the footer (`unsaved_bar`), not
        // at the end of a long scrolling form where a changed field can push
        // it out of sight.
        ui.add_space(12.0);

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
    /// in the sticky bar for the next time the page is seen.
    pub(crate) fn set_save_notice(&mut self, notice: Result<String, String>) {
        self.save_notice = Some(notice);
    }

    /// The sticky bar pinned above the status footer, drawn by the shell so
    /// the settings scroll area cannot hide it. Present only when there is
    /// something to act on: unsaved edits, or the outcome of the last save.
    pub(crate) fn unsaved_bar(
        &mut self,
        ui: &mut Ui,
        saved: &mut Settings,
        pipeline: &mut PipelineController,
    ) {
        let dirty = self.draft != *saved;
        if !dirty && self.save_notice.is_none() {
            return;
        }

        // A failed save outranks "Unsaved changes": the draft is still dirty
        // either way, and the reason it stayed that way is the only new
        // information on the bar. Resolved before the panel so the left side
        // holds no borrow of `self` while the buttons need it mutably.
        let (icon, color, text) = match (&self.save_notice, dirty) {
            (Some(Err(t)), _) => (theme::icons::WARNING, theme::DANGER, t.clone()),
            (_, true) => (
                theme::icons::WARNING,
                theme::WARNING,
                "Unsaved changes".to_string(),
            ),
            (Some(Ok(t)), false) => (theme::icons::CHECK, theme::SUCCESS, t.clone()),
            // Guarded by the early return above.
            (None, false) => return,
        };

        let window_fill = ui.visuals().window_fill;
        egui::Panel::bottom("settings_save_bar")
            .resizable(false)
            .show_separator_line(true)
            .frame(
                egui::Frame::default()
                    .fill(window_fill)
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show(ui, |ui| {
                // Sides, not horizontal + right_to_left: the message truncates
                // into whatever the buttons leave, never over them.
                egui::Sides::new().height(24.0).show(
                    ui,
                    |ui| {
                        ui.label(RichText::new(icon).color(color));
                        ui.add(egui::Label::new(RichText::new(text).small()).truncate());
                    },
                    |ui| {
                        if dirty {
                            let save =
                                egui::Button::new(RichText::new("Save").color(theme::ON_ACCENT))
                                    .fill(theme::accent_fill(ui.visuals()));
                            if ui.add(save).clicked() {
                                let ctx = ui.ctx().clone();
                                self.save(saved, pipeline, &ctx);
                            }
                            if ui.button("Discard").clicked() {
                                self.discard(saved);
                            }
                        } else if ui.button("Dismiss").clicked() {
                            self.save_notice = None;
                        }
                    },
                );
            });
    }

    /// Throw the draft away and re-seed every text buffer from the saved
    /// model; a stale buffer would write its old value straight back into
    /// the draft on the next frame.
    fn discard(&mut self, saved: &Settings) {
        self.draft = saved.clone();
        self.bufs = form::FormBufs::from_settings(saved);
        self.save_notice = None;
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

//! The Settings page (spec §3.6): a draft `Settings` edited in place, the
//! key section writing straight to the OS keychain, test-connection on a
//! background thread, and Save = validate -> persist TOML -> pipeline
//! restart. Failures leave the app running with the pipeline stopped and a
//! visible cause; never a silent dead state.

pub mod capture;
pub mod cleanup;
pub mod form;
pub mod get_started;
pub mod hotkey;
pub mod keys;
pub mod local;
mod onboarding;
mod preferences;
mod sections;
pub mod test;
pub mod updates;
pub use sections::Section;

use crate::model_download::ModelDownload;
use crate::pipeline::PipelineController;
use crate::theme;
use crate::update::Updater;
use egui::{RichText, Ui};
use hark_config::Settings;

pub struct SettingsPage {
    section: Section,
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
    /// Session-scoped click tally on the Voice heading; unlocks the hidden
    /// voices once it crosses the threshold. Intentionally not persisted.
    voice_secret_clicks: u32,
    /// Consumed by the app root, which shares the tray's clean shutdown path.
    close_requested: bool,
}

impl SettingsPage {
    /// `onboarding` = no STT key resolved at startup (pipeline stopped,
    /// key-related): the Get Started card latches active.
    pub fn new(settings: &Settings, onboarding: bool) -> Self {
        SettingsPage {
            section: Section::default(),
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
            voice_secret_clicks: 0,
            close_requested: false,
        }
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        saved: &mut Settings,
        pipeline: &mut PipelineController,
        updater: &mut Updater,
    ) {
        if self.section != Section::Audio {
            self.leave(pipeline);
        }
        self.stt_keys.sync_account(self.draft.provider.kind.label());
        if self.onboarding(ui, saved, pipeline) {
            return;
        }
        if ui.available_width() < theme::SETTINGS_BREAKPOINT {
            ui.horizontal_wrapped(|ui| self.navigation(ui, pipeline));
            ui.add_space(theme::SECTION_GAP);
            self.section_body(ui, saved, pipeline, updater);
        } else {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(theme::SETTINGS_NAV_WIDTH);
                    self.navigation(ui, pipeline);
                });
                ui.add_space(theme::SECTION_GAP);
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    self.section_body(ui, saved, pipeline, updater);
                });
            });
        }
    }

    /// Completion polling is independent of the visible Settings section.
    pub(crate) fn poll(&mut self) {
        self.download.poll();
        self.cleanup_test.poll();
        self.test.poll();
    }

    pub(crate) fn take_close_request(&mut self) -> bool {
        std::mem::take(&mut self.close_requested)
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
            (Some(Err(t)), _) => (
                theme::icons::WARNING,
                theme::danger(ui.visuals()),
                t.clone(),
            ),
            (_, true) => (
                theme::icons::WARNING,
                theme::warning(ui.visuals()),
                "Unsaved changes".to_string(),
            ),
            (Some(Ok(t)), false) => (theme::icons::CHECK, theme::success(ui.visuals()), t.clone()),
            // Guarded by the early return above.
            (None, false) => return,
        };

        let bar_fill = theme::surface(ui.visuals());
        egui::Panel::bottom("settings_save_bar")
            .resizable(false)
            .show_separator_line(true)
            .frame(
                egui::Frame::default()
                    .fill(bar_fill)
                    .inner_margin(egui::Margin::symmetric(16, 9)),
            )
            .show(ui, |ui| {
                // Sides, not horizontal + right_to_left: the message truncates
                // into whatever the buttons leave, never over them.
                egui::Sides::new().height(24.0).show(
                    ui,
                    |ui| {
                        ui.label(theme::icon_text(icon).color(color));
                        ui.add(egui::Label::new(RichText::new(text).small()).truncate());
                    },
                    |ui| {
                        if dirty {
                            if ui
                                .add(theme::primary_button(ui.visuals(), "Save changes"))
                                .clicked()
                            {
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
                    "Saved. Dictation is ready.".to_string()
                } else {
                    "Saved. Pipeline stopped; see the status bar for the cause.".to_string()
                }));
            }
            Err(detail) => self.save_notice = Some(Err(detail)),
        }
    }
}

/// Persist settings to the OS config path. Shared with the spellbook
/// editor, which saves per change instead of behind a Save button.
pub(crate) fn save_to_disk(settings: &Settings) -> Result<(), String> {
    let Some(path) = hark_config::default_config_path() else {
        return Err("No OS config directory found; cannot save settings.".to_string());
    };
    settings.save(&path).map_err(|e| e.to_string())
}

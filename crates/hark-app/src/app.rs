//! The application root: owns the settings, the pipeline controller, the
//! storage handle, and the current page. `logic` drains pipeline events (it
//! also runs while the window is hidden whenever `request_repaint` fires);
//! `ui` renders the shell. All of this stays on the main thread; the
//! pipeline and storage workers never do.

use crate::pipeline::{PipelineController, PipelineStatus};
use crate::ui::dictionary::DictionaryPage;
use crate::ui::history::HistoryPage;
use crate::ui::settings::SettingsPage;
use crate::ui::stats::StatsPage;
use crate::ui::{pages, settings, shell};
use crate::{storage, theme, tray};
use hark_config::{Settings, VoiceName};

pub struct HarkApp {
    /// The persisted model; only a settings Save (or dictionary edit)
    /// changes it. The in-progress form draft lives in `views.settings`.
    settings: Settings,
    /// Declared before `storage` on purpose: fields drop in order, so the
    /// pipeline (and its event pump, which holds a storage sender) is gone
    /// before `StorageHandle::drop` joins the worker to flush final writes.
    pipeline: PipelineController,
    storage: Option<storage::StorageHandle>,
    /// Why storage is off, surfaced by the history/stats error states.
    storage_error: Option<String>,
    /// Created lazily on the first `logic` call (event loop running, main
    /// thread: the macOS requirement). `None` before that, and also after
    /// a failed attempt (`tray_failed` stops retries).
    tray: Option<tray::Tray>,
    tray_failed: bool,
    /// Set by the tray's Quit: lets the close request through instead of
    /// hiding the window.
    quitting: bool,
    page: pages::Page,
    views: pages::Views,
}

impl HarkApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);

        let (settings, load_error) = load_settings();
        let (storage, storage_error) = open_storage(&cc.egui_ctx);
        let mut pipeline = PipelineController::new(storage.as_ref().map(|s| s.sender()));
        match load_error {
            None => pipeline.start(&settings, &cc.egui_ctx),
            // A broken config file must be visible, not silently defaulted
            // over: the app opens with the pipeline stopped and the cause
            // in the footer.
            Some(detail) => pipeline.mark_stopped(detail),
        }

        // No STT key at startup = guided onboarding (spec §3.11): the
        // Get Started card latches active on the settings page.
        let onboarding = matches!(
            pipeline.status(),
            PipelineStatus::Stopped {
                key_related: true,
                ..
            }
        );
        let views = pages::Views {
            settings: SettingsPage::new(&settings, onboarding),
            dictionary: DictionaryPage::new(),
            history: HistoryPage::new(),
            stats: StatsPage::new(),
        };

        // Window-first onboarding (spec §3.11): land on History when
        // dictation is live, on Settings when it needs attention.
        let page = if pipeline.is_running() {
            pages::Page::History
        } else {
            pages::Page::Settings
        };

        // The window starts hidden (main.rs) and shows only when it has
        // something to say: onboarding or a stopped pipeline. A running
        // pipeline keeps the app in the tray, the daemon shape it is meant
        // to have.
        if !pipeline.is_running() {
            show_window(&cc.egui_ctx);
        }
        // A hidden window is not guaranteed a natural first frame; one
        // explicit repaint makes `logic` run (creating the tray, flushing
        // the visibility command above) even if the window never shows.
        cc.egui_ctx.request_repaint();

        HarkApp {
            settings,
            pipeline,
            storage,
            storage_error,
            tray: None,
            tray_failed: false,
            quitting: false,
            page,
            views,
        }
    }

    /// Create the tray on the first callback: by then the event loop runs
    /// and we are on the main thread (macOS hard requirement, invisible on
    /// a Windows-only dev loop; right by construction). One attempt only.
    fn ensure_tray(&mut self, ctx: &egui::Context) {
        if self.tray.is_some() || self.tray_failed {
            return;
        }
        match tray::Tray::create(
            ctx,
            self.pipeline.status(),
            &self.settings.hotkey.ptt_key,
            self.settings.voice.default,
        ) {
            Ok(tray) => self.tray = Some(tray),
            Err(e) => {
                self.tray_failed = true;
                log::error!("tray creation failed: {e}");
                // Without a tray there is no way back to a hidden window,
                // so show it; close falls through to quit (handle_close).
                show_window(ctx);
            }
        }
    }

    fn handle_tray_actions(&mut self, ctx: &egui::Context) {
        let actions = match &self.tray {
            Some(tray) => tray.take_actions(),
            None => return,
        };
        for action in actions {
            match action {
                tray::TrayAction::SelectVoice(voice) => self.select_voice(voice, ctx),
                tray::TrayAction::OpenSettings => {
                    self.page = pages::Page::Settings;
                    show_window(ctx);
                }
                tray::TrayAction::ShowWindow => show_window(ctx),
                tray::TrayAction::Quit => {
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }

    /// A tray voice pick behaves like a dictionary edit: persist
    /// immediately, restart the pipeline (voices bake in at start), and
    /// mirror the settings draft so a later Save does not resurrect the
    /// old voice.
    fn select_voice(&mut self, voice: VoiceName, ctx: &egui::Context) {
        self.settings.voice.default = voice;
        self.views.settings.draft.voice.default = voice;
        if let Err(e) = settings::save_to_disk(&self.settings) {
            log::error!("tray voice change not persisted: {e}");
            self.views.settings.set_save_notice(Err(format!(
                "Voice changed for this session, but saving failed: {e}"
            )));
        }
        self.pipeline.start(&self.settings, ctx);
        if let Some(tray) = &mut self.tray {
            // Unconditional: native check items toggle themselves, so even
            // re-clicking the current voice needs its checkmark restored.
            tray.set_voice(voice);
        }
    }

    /// Close = hide once the tray exists (Quit lives in the tray menu).
    /// With no tray, or after Quit, the close request passes through and
    /// `run_native` returns.
    fn handle_close(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }
        if self.quitting || self.tray.is_none() {
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
    }

    /// While the chord is held, register the recording overlay viewport;
    /// otherwise leave it unregistered so egui tears the window down. Called
    /// from `logic`, which runs even while the main window is hidden, so the
    /// overlay works during normal tray-daemon operation. The overlay only
    /// exists while a meter does (i.e. the pipeline is live), so a Recording
    /// status without one is impossible in practice and simply shows nothing.
    fn show_recording_overlay(&mut self, ctx: &egui::Context) {
        if !matches!(self.pipeline.status(), PipelineStatus::Recording) {
            return;
        }
        if let Some(meter) = self.pipeline.level_meter() {
            let monitor = ctx.input(|i| i.viewport().monitor_size);
            crate::overlay::show(ctx, meter, monitor);
        }
    }
}

fn show_window(ctx: &egui::Context) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
}

/// Load settings from the OS config dir. A missing file is defaults (first
/// run); a missing config dir is defaults with a warning (headless CI); a
/// file that exists but fails to parse or validate is an error the UI must
/// surface.
fn load_settings() -> (Settings, Option<String>) {
    let Some(path) = hark_config::default_config_path() else {
        log::warn!("no OS config dir found; using default settings");
        return (Settings::default(), None);
    };
    match Settings::load(&path) {
        Ok(s) => {
            log::info!("settings: {} (missing file = defaults)", path.display());
            (s, None)
        }
        Err(e) => (
            Settings::default(),
            Some(format!("cannot load {}: {e}", path.display())),
        ),
    }
}

/// Open the history database and start the storage worker. Failure disables
/// history/stats for the session (with the cause on both panels) but never
/// dictation itself.
fn open_storage(ctx: &egui::Context) -> (Option<storage::StorageHandle>, Option<String>) {
    let Some(dir) = hark_config::default_data_dir() else {
        let detail = "No OS data directory found; history and stats are disabled.".to_string();
        log::warn!("{detail}");
        return (None, Some(detail));
    };
    let path = dir.join("hark.db");
    match storage::spawn(&path, ctx.clone()) {
        Ok(handle) => {
            log::info!("history database: {}", path.display());
            (Some(handle), None)
        }
        Err(e) => {
            let detail = format!("Cannot open the history database: {e}");
            log::error!("{detail}");
            (None, Some(detail))
        }
    }
}

impl eframe::App for HarkApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ensure_tray(ctx);
        self.pipeline.drain_events();
        self.handle_tray_actions(ctx);
        self.handle_close(ctx);
        self.show_recording_overlay(ctx);
        if let Some(tray) = &mut self.tray {
            tray.apply(
                self.pipeline.status(),
                &self.settings.hotkey.ptt_key,
                self.settings.voice.default,
            );
        }
    }

    // clear_color is transparent so the recording overlay's borderless
    // viewport shows only its pill, not an opaque window fill. The main
    // window is opaque and fully painted by its panels, so a transparent
    // clear is invisible there.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Color32::TRANSPARENT.to_normalized_gamma_f32()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        shell::show(
            ui,
            &mut self.page,
            &mut self.settings,
            &mut self.pipeline,
            &mut self.views,
            self.storage.as_ref(),
            self.storage_error.as_deref(),
        );
    }
}

// Clean shutdown is structural: when `run_native` returns (tray Quit, or a
// window close while trayless), `HarkApp` drops field by field.
// `PipelineController` drops the `PipelineHandle` (hook, worker, capture
// stop in order; the event pump follows), then `StorageHandle` joins the
// storage worker so the last history write commits before the process
// exits. The tray pumps park on global channels and die with the process;
// dropping `Tray` removes the OS icon.

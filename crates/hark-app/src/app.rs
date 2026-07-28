//! The application root: owns the settings, the pipeline controller, the
//! storage handle, and the current page. `logic` drains pipeline events (it
//! also runs while the window is hidden whenever `request_repaint` fires);
//! `ui` renders the shell. All of this stays on the main thread; the
//! pipeline and storage workers never do.

use crate::pipeline::{PipelineController, PipelineStatus};
use crate::ui::history::HistoryPage;
use crate::ui::invocations::InvocationsPage;
use crate::ui::settings::SettingsPage;
use crate::ui::spellbook::SpellbookPage;
use crate::ui::stats::StatsPage;
use crate::ui::{pages, settings, shell};
use crate::update::Updater;
use crate::{storage, theme, tray, window_state};
use hark_config::{Settings, VoiceName};
use std::sync::mpsc::{self, Receiver};

pub struct HarkApp {
    /// The persisted model; only a settings Save (or spellbook edit)
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
    /// Update check/self-update state, shared by the startup banner and the
    /// Settings section.
    updater: Updater,
    /// Declared before `activations` on purpose (same reasoning as
    /// `pipeline`/`storage`): fields drop in order, so the listener thread is
    /// stopped and joined before the channel it sends on goes away.
    _activation_listener: Option<hark_single_instance::ActivationListener>,
    /// Activation requests from later launches (Start menu, shortcut, taskbar)
    /// that found this instance already running. The listener thread only
    /// sends and wakes the loop; showing the window happens here, on the main
    /// thread. `None` if the listener could not start, which costs activation
    /// and nothing else.
    activations: Option<Receiver<()>>,
    /// Where the window opens: restored on the first `logic` pass, tracked
    /// from the root viewport, persisted by `save`.
    window: window_state::Memory,
    /// Set when startup found something worth showing the window for
    /// (onboarding, a stopped pipeline); consumed by the first `logic` pass.
    show_at_startup: bool,
}

impl HarkApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);

        let (settings, load_error) = load_settings();
        reconcile_autostart(settings.startup.launch_at_login);
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
            spellbook: SpellbookPage::new(),
            invocations: InvocationsPage::new(),
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
        // to have. Showing waits for the first `logic` pass so the restored
        // geometry is applied first — viewport commands run in the order they
        // were queued, and made-visible-then-resized is a visible flash.
        let show_at_startup = !pipeline.is_running();
        // A hidden window is not guaranteed a natural first frame; one
        // explicit repaint makes `logic` run (creating the tray, placing and
        // showing the window) even if the window never shows.
        wake_ui(&cc.egui_ctx);

        // Opt-in (default on): one background check at startup surfaces a
        // banner if a newer release exists. Runs on a worker thread.
        let mut updater = Updater::new();
        if settings.updates.check_on_startup {
            updater.start_check(&cc.egui_ctx);
        }

        // Same shape as the tray and pipeline pumps: a thread that owns the
        // blocking wait, a channel into the UI, and one repaint per event so a
        // hidden, idle window actually wakes up to drain it.
        let (activations, listener) = start_activation_listener(&cc.egui_ctx);

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
            updater,
            _activation_listener: listener,
            activations,
            window: window_state::Memory::load(cc.storage),
            show_at_startup,
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

    /// A tray voice pick behaves like a spellbook edit: persist
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

    /// A second launch asked us to surface. Land on History: someone who
    /// re-opens a running dictation app wants to see what it captured, and it
    /// matches where a healthy startup already lands.
    fn handle_activations(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.activations else { return };
        // Drain fully: several impatient clicks collapse into one show, and
        // leaving any queued would re-show the window on later frames.
        if rx.try_iter().count() == 0 {
            return;
        }
        log::info!("activated by a second launch; showing the window");
        self.page = pages::Page::History;
        show_window(ctx);
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

/// Start the activation listener, or log why not. Failure is not fatal: Hark
/// keeps running and only loses the ability to be re-opened from a shortcut,
/// so a hard error here would trade a minor annoyance for a dead app.
fn start_activation_listener(
    ctx: &egui::Context,
) -> (
    Option<Receiver<()>>,
    Option<hark_single_instance::ActivationListener>,
) {
    let (tx, rx) = mpsc::channel();
    let ctx = ctx.clone();
    match hark_single_instance::listen(move || {
        // Send first, then wake: the loop must find the event already queued
        // when it runs, or the repaint drains nothing and the request is lost
        // until something else happens to wake the app.
        if tx.send(()).is_ok() {
            wake_ui(&ctx);
        }
    }) {
        Ok(listener) => (Some(rx), Some(listener)),
        Err(e) => {
            log::warn!("activation listener not started ({e}); a second launch will exit quietly");
            (None, None)
        }
    }
}

/// Wake the UI from a worker thread. Every background thread that sends into
/// the UI must call this, never `Context::request_repaint()`.
///
/// `request_repaint()` is `request_repaint_of(viewport_id())`, and
/// `viewport_id()` is *whichever viewport is mid-pass right now* — meaningless
/// from another thread, and during a dictation actively wrong: the recording
/// pill is a second viewport repainting at ~60 fps, so the pipeline event that
/// ENDS a dictation asks for a repaint of the pill. eframe then discards it as
/// outdated (the pill's pass counter has already moved on), and the request is
/// gone. Nothing wakes the main window, and only a root pass drains pipeline
/// events, updates the tray, and unregisters the overlay — which is how a
/// finished dictation left the pill on screen and the tray red until a tray
/// click forced a pass. Only the root viewport runs `App::logic`, so the root
/// is what "wake the UI" means everywhere in this crate.
pub(crate) fn wake_ui(ctx: &egui::Context) {
    ctx.request_repaint_of(egui::ViewportId::ROOT);
}

fn show_window(ctx: &egui::Context) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
}

/// Bring the OS launch-at-login entry in line with the setting. Best-effort:
/// a registry failure is logged (label only) and never blocks startup or a
/// Save. No-op off Windows (`hark-autostart`).
pub(crate) fn reconcile_autostart(enabled: bool) {
    if let Err(e) = hark_autostart::reconcile(enabled) {
        log::warn!("could not update launch-at-login (enabled={enabled}): {e}");
    }
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
        // `logic` runs for the root viewport only (the recording overlay paints
        // through its own callback), so this is the one place that sees the
        // main window's geometry and nothing else's.
        self.window.tick(ctx);
        if std::mem::take(&mut self.show_at_startup) {
            show_window(ctx);
        }
        self.ensure_tray(ctx);
        self.pipeline.drain_events();
        self.updater.poll();
        self.handle_tray_actions(ctx);
        self.handle_activations(ctx);
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

    /// Called by eframe's auto-save and once more on a clean exit. It can fire
    /// during ANY viewport's frame, which is exactly why the geometry written
    /// here is the cached root one rather than whatever is painting.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.window.save(storage);
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
            &mut self.updater,
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

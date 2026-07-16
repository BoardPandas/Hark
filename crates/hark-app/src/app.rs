//! The application root: owns the settings, the pipeline controller, and
//! the current page. `logic` drains pipeline events (it also runs while the
//! window is hidden whenever `request_repaint` fires); `ui` renders the
//! shell. All of this stays on the main thread; the pipeline never does.

use crate::pipeline::PipelineController;
use crate::theme;
use crate::ui::{pages, shell};
use hark_config::Settings;
use hark_keychain::KeyStatus;

pub struct HarkApp {
    settings: Settings,
    pipeline: PipelineController,
    page: pages::Page,
    /// Keychain status captured at startup (and on future settings saves);
    /// never re-read per frame, since it hits the OS keychain.
    key_status: KeyStatus,
}

impl HarkApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);

        let (settings, load_error) = load_settings();
        let mut pipeline = PipelineController::new();
        match load_error {
            None => pipeline.start(&settings, &cc.egui_ctx),
            // A broken config file must be visible, not silently defaulted
            // over: the app opens with the pipeline stopped and the cause
            // in the footer.
            Some(detail) => pipeline.mark_stopped(detail),
        }

        let key_status = hark_keychain::key_status(settings.provider.kind.label());
        // Window-first onboarding (spec §3.11): land on History when
        // dictation is live, on Settings when it needs attention.
        let page = if pipeline.is_running() {
            pages::Page::History
        } else {
            pages::Page::Settings
        };
        HarkApp {
            settings,
            pipeline,
            page,
            key_status,
        }
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

impl eframe::App for HarkApp {
    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.pipeline.drain_events();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        shell::show(
            ui,
            &mut self.page,
            self.pipeline.status(),
            &self.settings,
            &self.key_status,
        );
    }
}

// Clean shutdown is structural: when `run_native` returns, `HarkApp` drops,
// `PipelineController` drops the `PipelineHandle`, and its Drop stops the
// hook, worker, and capture stream in order. Close = quit until the tray
// lands (CP5).

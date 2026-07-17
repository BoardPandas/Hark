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
use crate::ui::{pages, shell};
use crate::{storage, theme};
use hark_config::Settings;

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
        HarkApp {
            settings,
            pipeline,
            storage,
            storage_error,
            page,
            views,
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
    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.pipeline.drain_events();
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

// Clean shutdown is structural: when `run_native` returns, `HarkApp` drops
// field by field. `PipelineController` drops the `PipelineHandle` (hook,
// worker, capture stop in order; the event pump follows), then
// `StorageHandle` joins the storage worker so the last history write commits
// before the process exits. Close = quit until the tray lands (CP5).

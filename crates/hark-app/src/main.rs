//! Hark: push-to-talk dictation for the desktop.
//!
//! The main thread owns the eframe event loop (and, from CP5, the tray);
//! the dictation pipeline runs on worker threads behind a channel. Debug
//! builds keep a console for logs; release builds are windowless (any
//! future console child process must set CREATE_NO_WINDOW).
//!
//! Logging hygiene, unchanged from hark-cli: lengths, counts, millis, and
//! config labels only. Key material, raw audio, and transcript text are
//! structurally absent from every log call site.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod overlay;
mod pipeline;
mod storage;
mod theme;
mod tray;
mod ui;
mod update;

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // The autostart entry launches Hark with `--hidden` (hark-autostart). The
    // window already starts hidden into the tray, so this is informational
    // today; it keeps the launch intent explicit and the stored Run command
    // stable if a manual launch is ever made to show the window.
    let launched_hidden = std::env::args().any(|a| a == hark_autostart::HIDDEN_FLAG);
    log::info!("startup: launched_hidden={launched_hidden}");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Hark")
            .with_inner_size([960.0, 640.0])
            .with_min_inner_size([720.0, 480.0])
            // Launch hidden: when a key resolves the app lives in the tray
            // (CP5); `HarkApp::new` shows the window only when it needs
            // attention (onboarding, config error, stopped pipeline).
            .with_visible(false),
        ..Default::default()
    };
    eframe::run_native(
        "Hark",
        options,
        Box::new(|cc| Ok(Box::new(app::HarkApp::new(cc)))),
    )
}

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
mod model_download;
mod overlay;
mod pipeline;
mod storage;
mod theme;
mod tray;
mod ui;
mod update;

use std::time::{Duration, Instant};

/// When the updater relaunches us it spawns the new process before the outgoing
/// one has released its single-instance lock, so the lock is briefly still held
/// at startup. Poll for it up to this long before giving up, rather than losing
/// the race and exiting — which left the user with no running Hark after
/// "Download & install".
const RELAUNCH_LOCK_WAIT: Duration = Duration::from_secs(5);
const RELAUNCH_LOCK_POLL: Duration = Duration::from_millis(100);

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // The autostart entry launches Hark with `--hidden` (hark-autostart). The
    // window already starts hidden into the tray, so this is informational
    // today; it keeps the launch intent explicit and the stored Run command
    // stable if a manual launch is ever made to show the window.
    let launched_hidden = std::env::args().any(|a| a == hark_autostart::HIDDEN_FLAG);
    // The updater relaunches us with this flag; it means an outgoing instance is
    // still shutting down and holding the lock, so wait for it (see below).
    let relaunched = std::env::args().any(|a| a == hark_update::RELAUNCHED_FLAG);
    log::info!("startup: launched_hidden={launched_hidden} relaunched={relaunched}");

    // Bound to a named variable, not `_`: dropping the guard releases the lock,
    // and `let _ =` would do that on this very line. It must live to the end of
    // main, past run_native.
    let _instance_guard = match acquire_instance(relaunched) {
        Ok(Some(guard)) => Some(guard),
        Ok(None) => {
            // Autostart plus a manual launch is the common way here. Exiting
            // quietly leaves the running instance untouched in the tray.
            log::info!("startup: another Hark instance is already running; exiting");
            return Ok(());
        }
        // Fail open: a guard that can block startup is worse than the double
        // instance it exists to prevent.
        Err(e) => {
            log::warn!("startup: single-instance check failed ({e}); starting anyway");
            None
        }
    };

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

/// Claim the single-instance lock. A normal launch tries once and reports the
/// lock taken immediately (`Ok(None)`), so autostart-plus-manual-launch exits at
/// once. A launch relaunched by the updater instead polls: the outgoing instance
/// is mid-shutdown and still holds the lock for a few hundred milliseconds, so
/// racing it and exiting is exactly the "app never comes back" bug. Waiting lets
/// the old process finish releasing the OS lock, then we claim it and start.
fn acquire_instance(
    relaunched: bool,
) -> Result<Option<hark_single_instance::InstanceGuard>, hark_single_instance::Error> {
    let guard = hark_single_instance::acquire()?;
    if guard.is_some() || !relaunched {
        return Ok(guard);
    }
    log::info!(
        "startup: relaunched after update; waiting for the previous instance to release the lock"
    );
    let deadline = Instant::now() + RELAUNCH_LOCK_WAIT;
    while Instant::now() < deadline {
        std::thread::sleep(RELAUNCH_LOCK_POLL);
        if let Some(guard) = hark_single_instance::acquire()? {
            return Ok(Some(guard));
        }
    }
    log::warn!(
        "startup: previous instance still holds the lock after the relaunch grace period; exiting"
    );
    Ok(None)
}

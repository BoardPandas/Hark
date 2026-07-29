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
mod window_state;

use std::time::{Duration, Instant};

/// When the updater relaunches us it spawns the new process before the outgoing
/// one has released its single-instance lock, so the lock is briefly still held
/// at startup. Poll for it up to this long before giving up, rather than losing
/// the race and exiting — which left the user with no running Hark after
/// "Download & install".
const RELAUNCH_LOCK_WAIT: Duration = Duration::from_secs(5);
const RELAUNCH_LOCK_POLL: Duration = Duration::from_millis(100);

/// Rotate the log once it passes this; one backup is kept, so the logs cost at
/// most twice this on disk.
const LOG_MAX_BYTES: u64 = 2 * 1024 * 1024;

fn main() -> eframe::Result {
    init_logging();

    // The autostart entry launches Hark with `--hidden` (hark-autostart). The
    // window already starts hidden into the tray, so this is informational
    // today; it keeps the launch intent explicit and the stored Run command
    // stable if a manual launch is ever made to show the window.
    let launched_hidden = std::env::args().any(|a| a == hark_autostart::HIDDEN_FLAG);
    // The updater relaunches us with this flag; it means an outgoing instance is
    // still shutting down and holding the lock, so wait for it (see below).
    let relaunched = std::env::args().any(|a| a == hark_update::RELAUNCHED_FLAG);
    // The version first, and on its own line: a log that does not say which
    // build wrote it cannot answer "is this the fix, or the bug it replaced?",
    // which is the first question every report about a shipped build raises.
    log::info!("Hark {}", env!("CARGO_PKG_VERSION"));
    log::info!("startup: launched_hidden={launched_hidden} relaunched={relaunched}");

    // Bound to a named variable, not `_`: dropping the guard releases the lock,
    // and `let _ =` would do that on this very line. It must live to the end of
    // main, past run_native.
    let _instance_guard = match acquire_instance(relaunched) {
        Ok(Some(guard)) => Some(guard),
        Ok(None) => {
            // Autostart plus a manual launch is the common way here. Before
            // exiting, ask the running instance to show itself: a user who
            // clicked the Start menu entry or the shortcut is asking to *see*
            // Hark, and an app that answers a launch with nothing at all reads
            // as broken. The two exceptions are launches that never wanted a
            // window in the first place: the updater's relaunch (a silent
            // handover) and the autostart entry's `--hidden`.
            let activate = !relaunched && !launched_hidden;
            log::info!("startup: another Hark instance is already running (activate={activate})");
            if activate {
                if let Err(e) = hark_single_instance::signal_existing() {
                    log::warn!("startup: could not activate the running instance ({e})");
                }
            }
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
            // First-launch geometry only: a remembered size and position are
            // applied on the first frame instead (`window_state`).
            .with_inner_size(window_state::DEFAULT_SIZE)
            .with_min_inner_size(window_state::MIN_SIZE)
            // Launch hidden: when a key resolves the app lives in the tray
            // (CP5); `HarkApp::new` shows the window only when it needs
            // attention (onboarding, config error, stopped pipeline).
            .with_visible(false)
            // The glow backend builds ONE GL framebuffer config from the main
            // viewport and shares it with every child viewport. It only asks
            // for an alpha-capable (transparent) framebuffer when this main
            // viewport requests transparency — so without this flag the
            // recording overlay's transparent clear composites onto an opaque
            // buffer and renders as an opaque black box around the pill. The
            // main window itself stays opaque: its panels fully paint it (see
            // `HarkApp::clear_color`), so a transparent clear is invisible here.
            .with_transparent(true),
        // eframe's own window persistence is unusable for an app with a second
        // viewport. Its auto-save runs at the end of ANY viewport's paint and
        // stores *that* viewport's geometry under the root window's key — so
        // every dictation that outlived the auto-save interval saved the 160x40
        // recording pill as "the Hark window", and the next launch restored a
        // window barely bigger than its own title bar (`min_inner_size` does not
        // rescue it: eframe only clamps a restored size to 64x64). We keep the
        // geometry ourselves instead, from the root viewport only
        // (`window_state`), and eframe still persists egui memory as before.
        persist_window: false,
        ..Default::default()
    };
    eframe::run_native(
        "Hark",
        options,
        Box::new(|cc| Ok(Box::new(app::HarkApp::new(cc)))),
    )
}

/// Send logs somewhere a user can actually find them.
///
/// A release build is `windows_subsystem = "windows"`: it has no console, so
/// env_logger's stderr goes nowhere and every log line the app writes is lost —
/// which is exactly what turns a "sometimes it gets stuck" report into a
/// guessing game. Release builds therefore log to a file next to the history
/// database; debug builds keep the console. The log is safe to share by
/// construction: this codebase logs lengths, counts, millis, and config labels
/// only — never key material, audio, or transcript text.
fn init_logging() {
    let mut builder =
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"));
    // Debug keeps stderr so `cargo run` still prints; release has no console.
    if !cfg!(debug_assertions) {
        if let Some(file) = open_log_file() {
            builder.target(env_logger::Target::Pipe(Box::new(file)));
        }
    }
    builder.init();
    if let Some(path) = log_path() {
        log::info!("log file: {}", path.display());
    }
}

fn log_path() -> Option<std::path::PathBuf> {
    hark_config::default_data_dir().map(|dir| dir.join("hark.log"))
}

/// Open the log for appending, rotating first if the old one has grown past
/// [`LOG_MAX_BYTES`]. Every failure here returns `None` and leaves logging on
/// stderr: not being able to write a log must never stop the app starting.
fn open_log_file() -> Option<std::fs::File> {
    let path = log_path()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).ok()?;
    }
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > LOG_MAX_BYTES) {
        // One backup, replaced each rotation. A failed rename just means this
        // run keeps appending to a slightly oversized log.
        let _ = std::fs::rename(&path, path.with_extension("log.1"));
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok()
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

//! Hark dev binary: load config -> resolve key -> run the pipeline -> block
//! on Ctrl+C. Thin wiring only; the future tray/egui `hark-app` binary
//! replaces this `main` and calls the same `hark_pipeline::run` from its
//! event loop.
//!
//! Logging hygiene: this binary and every crate below it log lengths,
//! counts, and millis. Key material, raw audio, and transcript text are
//! structurally absent from all log call sites (grep-verified per the
//! acceptance criteria).

use std::process::ExitCode;
use std::sync::mpsc;

fn main() -> ExitCode {
    // Default to info so the pipeline's latency lines are visible in dev;
    // RUST_LOG overrides.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // 1. Settings: OS config dir, defaults when absent.
    let settings = match hark_config::default_config_path() {
        Some(path) => match hark_config::Settings::load(&path) {
            Ok(s) => {
                log::info!("settings: {} (missing file = defaults)", path.display());
                s
            }
            Err(e) => {
                eprintln!("hark: cannot load settings from {}: {e}", path.display());
                return ExitCode::from(2);
            }
        },
        None => {
            log::warn!("no OS config dir found; using default settings");
            hark_config::Settings::default()
        }
    };

    // 2. BYOK key: HARK_STT_KEY env override, then the OS keychain.
    let provider_label = settings.provider.kind.label();
    let api_key = match hark_keychain::resolve_key(provider_label) {
        Ok(k) => k,
        Err(e) => {
            // The error text is actionable and never carries key material.
            eprintln!("hark: {e}");
            return ExitCode::from(3);
        }
    };

    // 3. The pipeline: capture + hook + worker threads.
    let handle = match hark_pipeline::run(&settings, api_key) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("hark: cannot start: {e}");
            return ExitCode::from(4);
        }
    };

    println!(
        "hark ready: hold {} and speak; release to inject. Ctrl+C to quit.",
        settings.hotkey.ptt_key
    );

    // 4. Park until Ctrl+C. The channel wakes exactly once; the handler
    //    itself does no work (signal-safety).
    let (quit_tx, quit_rx) = mpsc::channel::<()>();
    if let Err(e) = ctrlc::set_handler(move || {
        let _ = quit_tx.send(());
    }) {
        eprintln!("hark: cannot install Ctrl+C handler: {e}");
        return ExitCode::from(4);
    }
    let _ = quit_rx.recv();

    // 5. Clean shutdown: the handle's Drop stops hook -> worker -> capture.
    println!("hark: shutting down");
    drop(handle);
    ExitCode::SUCCESS
}

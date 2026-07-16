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
use std::str::FromStr;
use std::sync::mpsc;

/// Hand-rolled parse of the one supported flag: `--voice <name>` (or
/// `--voice=<name>`), overriding `voice.default` for this run. No clap for
/// one flag. Unknown arguments and invalid voice names return the message
/// to print before exiting (the voice parse error lists the valid names).
fn voice_override(args: &[String]) -> Result<Option<hark_voice::Voice>, String> {
    let mut voice = None;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--voice" {
            let value = args
                .get(i + 1)
                .ok_or_else(|| "--voice requires a value".to_string())?;
            voice = Some(hark_voice::Voice::from_str(value).map_err(|e| e.to_string())?);
            i += 2;
        } else if let Some(value) = args[i].strip_prefix("--voice=") {
            voice = Some(hark_voice::Voice::from_str(value).map_err(|e| e.to_string())?);
            i += 1;
        } else {
            return Err(format!(
                "unknown argument {:?} (usage: hark-cli [--voice <name>])",
                args[i]
            ));
        }
    }
    Ok(voice)
}

/// Map the CLI/voice enum back onto the config-side name for the override.
fn to_voice_name(voice: hark_voice::Voice) -> hark_config::VoiceName {
    match voice {
        hark_voice::Voice::Verbatim => hark_config::VoiceName::Verbatim,
        hark_voice::Voice::Clean => hark_config::VoiceName::Clean,
        hark_voice::Voice::Professional => hark_config::VoiceName::Professional,
        hark_voice::Voice::Casual => hark_config::VoiceName::Casual,
        hark_voice::Voice::Custom => hark_config::VoiceName::Custom,
    }
}

fn main() -> ExitCode {
    // Default to info so the pipeline's latency lines are visible in dev;
    // RUST_LOG overrides.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let override_voice = match voice_override(&args) {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("hark: {msg}");
            return ExitCode::from(2);
        }
    };

    // 1. Settings: OS config dir, defaults when absent.
    let mut settings = match hark_config::default_config_path() {
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
    if let Some(voice) = override_voice {
        // Same rule Settings::validate applies to the config default.
        if voice == hark_voice::Voice::Custom && settings.voice.custom_prompt.trim().is_empty() {
            eprintln!("hark: --voice custom requires voice.custom_prompt in config.toml");
            return ExitCode::from(2);
        }
        log::info!("voice override for this run: {}", voice.name());
        settings.voice.default = to_voice_name(voice);
    }

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

    // 3. The pipeline: capture + hook + worker threads. The CLI has no UI,
    //    so the advisory event receiver is dropped on the spot; the worker's
    //    best-effort sends tolerate that by contract.
    let (events_tx, _) = mpsc::channel();
    let handle = match hark_pipeline::run(&settings, api_key, events_tx) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn no_args_means_no_override() {
        assert_eq!(voice_override(&[]).unwrap(), None);
    }

    #[test]
    fn both_flag_forms_parse() {
        assert_eq!(
            voice_override(&args(&["--voice", "professional"])).unwrap(),
            Some(hark_voice::Voice::Professional)
        );
        assert_eq!(
            voice_override(&args(&["--voice=verbatim"])).unwrap(),
            Some(hark_voice::Voice::Verbatim)
        );
    }

    #[test]
    fn invalid_voice_error_lists_the_valid_names() {
        let msg = voice_override(&args(&["--voice", "shouty"])).unwrap_err();
        for name in hark_voice::Voice::NAMES {
            assert!(msg.contains(name), "must list {name}: {msg}");
        }
    }

    #[test]
    fn missing_value_and_unknown_args_are_rejected() {
        assert!(voice_override(&args(&["--voice"])).is_err());
        assert!(voice_override(&args(&["--frobnicate"])).is_err());
    }
}

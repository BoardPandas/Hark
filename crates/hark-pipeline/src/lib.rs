//! Hark dictation pipeline: the worker-thread state machine that glues
//! audio capture, push-to-talk edges, cloud STT, and injection.
//!
//! `run(settings, api_key)` spawns the three worker threads (audio capture,
//! keyboard hook, pipeline worker) and returns a handle; the calling thread
//! stays free. In `hark-cli` the caller just parks on Ctrl+C; the future
//! tray/egui binary will call the same `run` from its main-thread event
//! loop, preserving the UI-on-main-thread invariant unchanged.

mod retry;
mod state;
mod worker;

pub use retry::should_retry;
pub use state::{advance, Action, Event, PipelineState};

use hark_audio::WindowParams;
use hark_config::Settings;
use hark_inject::{InjectSettings, Strategy};
use hark_stt::{ProviderConfig, SttError};
use std::sync::mpsc;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PipelineError {
    #[error(transparent)]
    Hotkey(#[from] hark_hotkey::HotkeyError),
    #[error(transparent)]
    Capture(#[from] hark_audio::CaptureError),
    #[error(transparent)]
    Stt(#[from] SttError),
    #[error("invalid configuration: {0}")]
    Config(String),
}

/// The running pipeline. Dropping it shuts everything down in order: the
/// hotkey listener stops (closing the event channel), the worker drains and
/// exits, and the capture stream stops with its handle.
pub struct PipelineHandle {
    // Declaration order is load-bearing for Drop: listener first (closes the
    // channel), then the worker join, then capture.
    listener: Option<hark_hotkey::ListenerHandle>,
    worker: Option<std::thread::JoinHandle<()>>,
    _capture: hark_audio::CaptureHandle,
}

impl Drop for PipelineHandle {
    fn drop(&mut self) {
        // Stop the hook thread; its exit drops the sender, which ends the
        // worker's receive loop.
        drop(self.listener.take());
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
        // _capture drops last, stopping the audio stream.
    }
}

/// Map the settings enum onto the STT crate's provider taxonomy: OpenAI and
/// Groq (and any custom endpoint) share the OpenAI-compatible contract.
fn provider_config(settings: &Settings, api_key: String) -> Result<ProviderConfig, PipelineError> {
    let kind = match settings.provider.kind {
        hark_config::ProviderKind::Deepgram => hark_stt::ProviderKind::Deepgram,
        hark_config::ProviderKind::Openai
        | hark_config::ProviderKind::Groq
        | hark_config::ProviderKind::OpenaiCompatible => hark_stt::ProviderKind::OpenAiCompatible,
    };
    // Settings::load validated this; guard again for hand-built Settings.
    let base_url = settings.provider.resolved_base_url().ok_or_else(|| {
        PipelineError::Config("openai-compatible provider requires a base_url".to_string())
    })?;
    Ok(ProviderConfig {
        kind,
        label: settings.provider.kind.label().to_string(),
        base_url,
        model: settings.provider.resolved_model(),
        api_key,
        bias_terms: settings.dictionary.terms.clone(),
    })
}

fn window_params(audio: &hark_config::Audio) -> WindowParams {
    WindowParams {
        preroll_ms: audio.preroll_ms,
        tail_ms: audio.tail_ms,
        max_hold_s: audio.max_hold_s,
        min_speech_ms: audio.min_speech_ms,
        silence_rms: audio.silence_rms,
    }
}

fn inject_settings(inject: &hark_config::Inject) -> InjectSettings {
    InjectSettings {
        strategy: match inject.strategy {
            hark_config::InjectStrategy::Clipboard => Strategy::Clipboard,
            hark_config::InjectStrategy::Type => Strategy::Type,
        },
        set_paste_delay_ms: inject.set_paste_delay_ms,
        paste_restore_delay_ms: inject.paste_restore_delay_ms,
        clipboard_retries: inject.clipboard_retries,
    }
}

/// Build and start the whole pipeline: shared HTTP client, provider adapter,
/// continuous audio capture, the keyboard hook, and the worker thread.
/// Blocks only until the pieces are up (the pre-warm request runs on the
/// worker thread so startup stays fast).
pub fn run(settings: &Settings, api_key: String) -> Result<PipelineHandle, PipelineError> {
    let chord = hark_hotkey::PttChord::parse(&settings.hotkey.ptt_key)
        .map_err(hark_hotkey::HotkeyError::from)?;
    let provider_cfg = provider_config(settings, api_key)?;
    let prewarm_url = provider_cfg.base_url.clone();

    let client = hark_stt::shared_client()?;
    let provider = hark_stt::build(&provider_cfg, client.clone())?;

    let window = window_params(&settings.audio);
    let (capture, consumer) = hark_audio::start(hark_audio::window::ring_seconds(&window))?;
    let sample_rate = capture.sample_rate();
    log::info!(
        "capture live at {sample_rate} Hz; ptt chord: {chord}; provider: {}",
        provider_cfg.label
    );

    let (ptt_tx, ptt_rx) = mpsc::channel();
    let listener = hark_hotkey::spawn_listener(chord, ptt_tx)?;

    let w = worker::Worker {
        consumer,
        sample_rate,
        window,
        inject: inject_settings(&settings.inject),
        provider,
        corrector: hark_dictionary::Corrector::new(&settings.dictionary.terms),
        prewarm_url,
        client,
    };
    let worker = std::thread::Builder::new()
        .name("hark-pipeline-worker".to_string())
        .spawn(move || worker::run(w, ptt_rx))
        .expect("spawning the worker thread cannot fail");

    Ok(PipelineHandle {
        listener: Some(listener),
        worker: Some(worker),
        _capture: capture,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings_from(toml: &str) -> Settings {
        Settings::from_toml(toml).unwrap()
    }

    #[test]
    fn default_settings_map_to_deepgram_nova3() {
        let cfg = provider_config(&settings_from(""), "KEY".to_string()).unwrap();
        assert_eq!(cfg.kind, hark_stt::ProviderKind::Deepgram);
        assert_eq!(cfg.label, "deepgram");
        assert_eq!(cfg.base_url, "https://api.deepgram.com");
        assert_eq!(cfg.model, "nova-3");
        assert_eq!(cfg.api_key, "KEY");
        assert!(cfg.bias_terms.is_empty());
    }

    #[test]
    fn groq_and_openai_share_the_openai_compatible_adapter() {
        for (kind, url, model) in [
            (
                "groq",
                "https://api.groq.com/openai/v1",
                "whisper-large-v3-turbo",
            ),
            (
                "openai",
                "https://api.openai.com/v1",
                "gpt-4o-mini-transcribe",
            ),
        ] {
            let cfg = provider_config(
                &settings_from(&format!("[provider]\nkind = \"{kind}\"")),
                "K".to_string(),
            )
            .unwrap();
            assert_eq!(cfg.kind, hark_stt::ProviderKind::OpenAiCompatible);
            assert_eq!(cfg.label, kind);
            assert_eq!(cfg.base_url, url);
            assert_eq!(cfg.model, model);
        }
    }

    #[test]
    fn dictionary_terms_feed_provider_bias_terms() {
        let s = settings_from("[dictionary]\nterms = [\"Hark\", \"Levenshtein\"]");
        let cfg = provider_config(&s, "K".to_string()).unwrap();
        assert_eq!(cfg.bias_terms, vec!["Hark", "Levenshtein"]);
    }

    #[test]
    fn audio_settings_map_to_window_params() {
        let s = settings_from("[audio]\npreroll_ms = 200\ntail_ms = 100\nmax_hold_s = 30");
        let w = window_params(&s.audio);
        assert_eq!(w.preroll_ms, 200);
        assert_eq!(w.tail_ms, 100);
        assert_eq!(w.max_hold_s, 30);
        assert_eq!(w.min_speech_ms, 250); // default carried through
    }

    #[test]
    fn inject_settings_map_including_strategy() {
        let s = settings_from("[inject]\nstrategy = \"type\"\nclipboard_retries = 3");
        let i = inject_settings(&s.inject);
        assert_eq!(i.strategy, Strategy::Type);
        assert_eq!(i.clipboard_retries, 3);
        assert_eq!(i.set_paste_delay_ms, 50); // default carried through
    }
}

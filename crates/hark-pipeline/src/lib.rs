//! Hark dictation pipeline: the worker-thread state machine that glues
//! audio capture, push-to-talk edges, cloud STT, and injection.
//!
//! `run(settings, api_key, events)` spawns the three worker threads (audio
//! capture, keyboard hook, pipeline worker) and returns a handle; the
//! calling thread stays free. The `events` sender carries advisory
//! [`PipelineEvent`]s toward the UI: sends are non-blocking and a
//! disconnected receiver is ignored, so callers that do not care (hark-cli)
//! may drop the receiver immediately.

mod events;
mod local;
mod retry;
mod state;
mod worker;

pub use events::{DictationRecord, FailStage, PipelineEvent};
pub use hark_audio::LevelMeter;
pub use retry::should_retry;
pub use state::{advance, Action, Event, PipelineState};

use hark_audio::WindowParams;
use hark_config::Settings;
use hark_inject::{InjectSettings, Strategy};
use hark_stt::{ProviderConfig, SttError};
use std::sync::mpsc;
use std::sync::Arc;
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
    /// Live mic-level meter, cloned from the capture handle. Advisory UI data
    /// (the recording overlay's audio-reactive pulse); reading it never
    /// touches the audio path.
    level: Arc<LevelMeter>,
    _capture: hark_audio::CaptureHandle,
}

impl PipelineHandle {
    /// The live input-level meter for UI feedback. Cheap to clone.
    pub fn level_meter(&self) -> Arc<LevelMeter> {
        self.level.clone()
    }
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
///
/// Public because the settings "Test connection" flow builds the exact same
/// config the pipeline would run with; a passing test therefore validates
/// the real path, not a parallel reimplementation.
pub fn provider_config(
    settings: &Settings,
    api_key: String,
) -> Result<ProviderConfig, PipelineError> {
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

/// Map the config-side voice name onto the voice crate's enum (same parallel
/// -enums pattern as `provider_config` for the STT kinds).
fn effective_voice(name: hark_config::VoiceName) -> hark_voice::Voice {
    match name {
        hark_config::VoiceName::Verbatim => hark_voice::Voice::Verbatim,
        hark_config::VoiceName::Clean => hark_voice::Voice::Clean,
        hark_config::VoiceName::Professional => hark_voice::Voice::Professional,
        hark_config::VoiceName::Casual => hark_voice::Voice::Casual,
        hark_config::VoiceName::Custom => hark_voice::Voice::Custom,
    }
}

/// Build the optional cleanup step. Fail-open at every layer (unresolvable
/// provider, missing key, adapter build failure): warn once and return None,
/// which the worker treats as Verbatim. The pipeline must start regardless;
/// STT keeps working when the optional feature cannot.
fn build_cleanup(
    settings: &Settings,
    client: reqwest::blocking::Client,
    stt_api_key: &str,
) -> Option<worker::CleanupPlan> {
    use hark_config::{CleanupKeySource, CleanupResolution};

    let resolved = match hark_config::resolve_cleanup_provider(
        &settings.provider,
        &settings.voice,
        settings.voice.default,
    ) {
        CleanupResolution::Verbatim => return None,
        CleanupResolution::VerbatimWithWarning { reason } => {
            log::warn!("cleanup disabled: {reason}");
            return None;
        }
        CleanupResolution::Resolved(r) => r,
    };

    let api_key = match &resolved.key_source {
        // The "share one provider+key" case: no second keychain read.
        CleanupKeySource::ReuseSttKey => stt_api_key.to_string(),
        CleanupKeySource::Account(account) => {
            match hark_keychain::resolve_key_for(hark_keychain::CLEANUP_ENV_OVERRIDE, account) {
                Ok(k) => k,
                Err(e) => {
                    log::warn!("cleanup key unavailable ({e}); running verbatim");
                    return None;
                }
            }
        }
    };

    let voice = effective_voice(settings.voice.default);
    let config = hark_voice::openai_compatible::CleanupConfig {
        label: resolved.kind.label().to_string(),
        base_url: resolved.base_url.clone(),
        model: resolved.model.clone(),
        api_key,
        temperature: resolved.temperature,
        reasoning_effort: resolved.reasoning_effort.clone(),
        voice,
        custom_prompt: settings.voice.custom_prompt.clone(),
        dictionary_terms: settings.dictionary.terms.clone(),
    };
    let adapter = match hark_voice::openai_compatible::OpenAiCompatibleChat::new(&config, client) {
        Ok(a) => a,
        Err(e) => {
            log::warn!("cleanup adapter build failed ({e}); running verbatim");
            return None;
        }
    };

    // Second pre-warm only when the cleanup endpoint differs from the STT
    // one (base-URL comparison; a same-host false positive just costs one
    // harmless extra GET).
    let prewarm_url = (settings.provider.resolved_base_url().as_deref()
        != Some(resolved.base_url.as_str()))
    .then(|| resolved.base_url.clone());

    log::info!(
        "cleanup: voice={} provider={} model={}, gate: skip below {} words, \
         reject above {}x input words",
        voice.name(),
        resolved.kind.label(),
        resolved.model,
        settings.voice.skip_below_words,
        settings.voice.max_expansion_ratio
    );
    Some(worker::CleanupPlan {
        cleaner: Box::new(adapter),
        voice,
        model: resolved.model,
        skip_below_words: settings.voice.skip_below_words,
        max_expansion_ratio: settings.voice.max_expansion_ratio,
        prewarm_url,
    })
}

/// Build the on-device plan, or `None` when local STT is off or unusable.
///
/// Fail-open like `build_cleanup`: an unknown model id or a missing data
/// directory warns and degrades to cloud-only rather than refusing to start.
/// The one exception is primary mode, where degrading silently would leave
/// the user with no engine at all — so that case logs an error, and the
/// worker's policy surfaces it on the first dictation.
fn build_local(settings: &Settings) -> Option<local::LocalPlan> {
    let cfg = &settings.local_stt;
    if !cfg.mode.uses_local() {
        return None;
    }
    if !hark_local_stt::LocalEngine::is_available() {
        log::warn!(
            "local STT is set to {} but this build has no on-device engine; using cloud only",
            cfg.mode.label()
        );
        return None;
    }
    let (spec, dir) = match hark_local_stt::resolve(&cfg.model) {
        Ok(pair) => pair,
        Err(e) => {
            log::error!("local STT unavailable: {e}");
            return None;
        }
    };
    let plan = local::LocalPlan::new(cfg.mode, spec, dir, cfg.threads);
    if !plan.is_ready() {
        log::warn!(
            "local model {} is not downloaded yet; mode={}",
            spec.id,
            cfg.mode.label()
        );
    }
    Some(plan)
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
pub fn run(
    settings: &Settings,
    api_key: String,
    events: mpsc::Sender<PipelineEvent>,
) -> Result<PipelineHandle, PipelineError> {
    let chord = hark_hotkey::PttChord::parse(&settings.hotkey.ptt_key)
        .map_err(hark_hotkey::HotkeyError::from)?;
    let client = hark_stt::shared_client()?;
    // Before provider_config consumes the key: the inherit path reuses it.
    let cleanup = build_cleanup(settings, client.clone(), &api_key);
    let provider_cfg = provider_config(settings, api_key)?;
    let prewarm_url = provider_cfg.base_url.clone();

    let local = build_local(settings);
    let mode = settings.local_stt.mode;

    // Primary mode never contacts a provider, so it must not require a key
    // (that is the whole point of "I don't want to use a cloud model").
    let provider = if mode.uses_cloud() {
        // A ready local fallback earns the cloud a *shorter* leash: the full
        // 15 s timeout plus on-device decoding would make a rescued dictation
        // slower than no fallback at all.
        let fallback_armed = mode == hark_config::LocalMode::Fallback
            && local.as_ref().is_some_and(|p| p.is_ready());
        let stt_client = if fallback_armed {
            log::info!(
                "on-device fallback armed; cloud requests capped at {} ms",
                settings.local_stt.fallback_after_ms
            );
            hark_stt::client_with_timeout(settings.local_stt.fallback_after_ms)?
        } else {
            client.clone()
        };
        Some(hark_stt::build(&provider_cfg, stt_client)?)
    } else {
        log::info!("local STT is primary; no cloud provider will be contacted");
        None
    };

    let window = window_params(&settings.audio);
    let (capture, consumer) = hark_audio::start(
        hark_audio::window::ring_seconds(&window),
        settings.audio.input_device.clone(),
    )?;
    let sample_rate = capture.sample_rate();
    let level = capture.level_meter();
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
        cloud_label: provider_cfg.label.clone(),
        local,
        corrector: hark_dictionary::Corrector::new(&settings.dictionary.terms),
        cleanup,
        prewarm_url,
        client,
        stt_model: provider_cfg.model.clone(),
        events,
    };
    let worker = std::thread::Builder::new()
        .name("hark-pipeline-worker".to_string())
        .spawn(move || worker::run(w, ptt_rx))
        .expect("spawning the worker thread cannot fail");

    Ok(PipelineHandle {
        listener: Some(listener),
        worker: Some(worker),
        level,
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

    // --- cleanup plan wiring. Only paths that never read the real OS
    // keychain are exercised here: Verbatim, warn-and-degrade, STT key
    // reuse, and one Account-path test that the HARK_CLEANUP_KEY env
    // override satisfies before the keyring would be touched. ---

    fn client() -> reqwest::blocking::Client {
        hark_stt::shared_client().unwrap()
    }

    #[test]
    fn verbatim_voice_never_constructs_a_cleaner() {
        let s = settings_from("[provider]\nkind = \"openai\"\n[voice]\ndefault = \"verbatim\"");
        assert!(build_cleanup(&s, client(), "STT-KEY").is_none());
    }

    #[test]
    fn default_deepgram_config_degrades_to_no_cleaner() {
        // Out-of-the-box config: Deepgram STT + Clean voice. Warn, no plan,
        // and crucially no error: the pipeline must start.
        assert!(build_cleanup(&settings_from(""), client(), "STT-KEY").is_none());
    }

    #[test]
    fn openai_stt_inherits_into_a_cleanup_plan_without_keychain() {
        let s = settings_from(
            "[provider]\nkind = \"openai\"\n[voice]\nskip_below_words = 3\n[dictionary]\nterms = [\"Hark\"]",
        );
        let plan = build_cleanup(&s, client(), "STT-KEY").expect("inherit path builds a plan");
        assert_eq!(plan.voice, hark_voice::Voice::Clean);
        assert_eq!(plan.model, "gpt-5-nano");
        assert_eq!(plan.skip_below_words, 3);
        assert_eq!(plan.cleaner.label(), "openai");
        // Same endpoint as STT: no second pre-warm needed.
        assert_eq!(plan.prewarm_url, None);
    }

    #[test]
    fn explicit_provider_with_env_key_prewarns_the_differing_host() {
        // Deepgram STT + explicit groq cleanup: the Account path resolves
        // via HARK_CLEANUP_KEY before any keyring read, and the differing
        // endpoint gets its own pre-warm URL.
        std::env::set_var("HARK_CLEANUP_KEY", "env-test-key");
        let s = settings_from("[voice.provider]\nkind = \"groq\"");
        let plan = build_cleanup(&s, client(), "STT-KEY");
        std::env::remove_var("HARK_CLEANUP_KEY");
        let plan = plan.expect("env-keyed explicit provider builds a plan");
        assert_eq!(plan.model, "llama-3.1-8b-instant");
        assert_eq!(plan.cleaner.label(), "groq");
        assert_eq!(
            plan.prewarm_url.as_deref(),
            Some("https://api.groq.com/openai/v1")
        );
    }
}

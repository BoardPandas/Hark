//! The worker's parts, built from settings before the threads start: the
//! invocation expander, the cleanup plan, the on-device plan, the streaming
//! adapter, and the window and injection parameters.

use crate::{local, worker};
use hark_audio::WindowParams;
use hark_config::Settings;
use hark_inject::{InjectSettings, Strategy};
use hark_stt::ProviderConfig;

/// Build the invocation expander from the configured triggers (same
/// parallel-enums pattern as `provider_config` for the STT kinds).
///
/// The matcher is a **derived artifact**: it is rebuilt from the TOML on
/// every pipeline start and never cached anywhere, so the config file stays
/// the single owner (LL-G `architecture/transient-cache-without-drift`).
/// Entries that cannot arm are skipped by the expander itself; only the
/// counts are logged, never a phrase or an expansion.
pub(crate) fn build_expander(invocations: &hark_config::Invocations) -> hark_spellbook::Expander {
    let entries: Vec<(String, Vec<String>, String, hark_spellbook::Scope)> = invocations
        .entries
        .iter()
        .map(|i| {
            let scope = match i.scope {
                hark_config::Scope::Utterance => hark_spellbook::Scope::Utterance,
                hark_config::Scope::Anywhere => hark_spellbook::Scope::Anywhere,
            };
            (
                i.phrase.clone(),
                i.aliases.clone(),
                i.expansion.clone(),
                scope,
            )
        })
        .collect();
    let expander = hark_spellbook::Expander::with_aliases(&entries);
    if expander.skipped() > 0 {
        log::warn!(
            "{} invocation(s) will not fire (need {}+ words, a non-empty expansion, \
             and a trigger no earlier entry already uses); {} armed",
            expander.skipped(),
            hark_spellbook::MIN_TRIGGER_WORDS,
            expander.armed()
        );
    } else if expander.armed() > 0 {
        log::info!("{} invocation(s) armed", expander.armed());
    }
    if expander.skipped_aliases() > 0 {
        log::warn!(
            "{} invocation alternate(s) ignored (need {}+ words and a phrase no earlier \
             trigger or alternate already uses)",
            expander.skipped_aliases(),
            hark_spellbook::MIN_TRIGGER_WORDS,
        );
    }
    expander
}

/// Map the config-side voice name onto the voice crate's enum (same parallel
/// -enums pattern as `provider_config` for the STT kinds).
fn effective_voice(name: hark_config::VoiceName) -> hark_voice::Voice {
    match name {
        hark_config::VoiceName::Verbatim => hark_voice::Voice::Verbatim,
        hark_config::VoiceName::Grammar => hark_voice::Voice::Grammar,
        hark_config::VoiceName::Clean => hark_voice::Voice::Clean,
        hark_config::VoiceName::Professional => hark_voice::Voice::Professional,
        hark_config::VoiceName::Casual => hark_voice::Voice::Casual,
        hark_config::VoiceName::Notes => hark_voice::Voice::Notes,
        hark_config::VoiceName::Concise => hark_voice::Voice::Concise,
        hark_config::VoiceName::Direct => hark_voice::Voice::Direct,
        hark_config::VoiceName::Plain => hark_voice::Voice::Plain,
        hark_config::VoiceName::Pirate => hark_voice::Voice::Pirate,
        hark_config::VoiceName::Custom => hark_voice::Voice::Custom,
    }
}

/// Build the optional cleanup step. Fail-open at every layer (unresolvable
/// provider, missing key, adapter build failure): warn once and return None,
/// which the worker treats as Verbatim. The pipeline must start regardless;
/// STT keeps working when the optional feature cannot.
pub(crate) fn build_cleanup(
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
        spellbook_terms: settings.spellbook.terms(),
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
pub(crate) fn build_local(settings: &Settings) -> Option<local::LocalPlan> {
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

/// The streaming adapter for this run, if the provider has one. Gated on the
/// same `uses_cloud` as the batch adapter: primary local mode promises no
/// provider is contacted, and a live session opened on key-down would break
/// that promise before a single sample was sent. So `build` is not even
/// called there, whichever live provider is configured.
pub(crate) fn live_adapter(
    mode: hark_config::LocalMode,
    cfg: &ProviderConfig,
    build: impl FnOnce(&ProviderConfig) -> Option<Box<dyn hark_stt::LiveStt>>,
) -> Option<Box<dyn hark_stt::LiveStt>> {
    mode.uses_cloud().then(|| build(cfg)).flatten()
}

pub(crate) fn window_params(audio: &hark_config::Audio) -> WindowParams {
    WindowParams {
        preroll_ms: audio.preroll_ms,
        tail_ms: audio.tail_ms,
        max_hold_s: audio.max_hold_s,
        min_speech_ms: audio.min_speech_ms,
        silence_rms: audio.silence_rms,
    }
}

pub(crate) fn inject_settings(inject: &hark_config::Inject) -> InjectSettings {
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

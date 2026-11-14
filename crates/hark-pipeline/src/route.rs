//! Which adapter and model the STT settings select, and the provider config
//! each path runs with: the dictation's stream, its batch fallback, and a
//! meeting's live chunks.

use crate::PipelineError;
use hark_config::Settings;
use hark_stt::ProviderConfig;

/// Whether this build links the WebSocket adapters (hark-stt's `live`).
pub(crate) const LIVE_BUILD: bool = cfg!(feature = "live");

/// Which adapter and model the STT settings select. Pure and silent, so the
/// status line can name the model a dictation really uses on every frame.
///
/// OpenAI with Live transcribe on streams to the realtime adapter. A build
/// without the WebSocket client runs what the toggle being off would run (the
/// pinned batch model, else gpt-transcribe) rather than failing every
/// dictation; [`provider_config`] is where that gets logged.
pub fn stt_route(provider: &hark_config::Provider) -> (hark_stt::ProviderKind, String) {
    match provider.live_transcribe_model() {
        Some(model) if LIVE_BUILD => (hark_stt::ProviderKind::OpenAiLive, model),
        _ => batch_stt_route(provider),
    }
}

/// The adapter and model that take a finished clip: what the dictation falls
/// back to when the stream did not carry it, and what every meeting chunk
/// uses. The same as [`stt_route`] except for OpenAI with Live transcribe,
/// where it is what the toggle being off would run — the pinned batch model,
/// else gpt-transcribe — and never the realtime model.
pub fn batch_stt_route(provider: &hark_config::Provider) -> (hark_stt::ProviderKind, String) {
    if !provider.uses_live_transcribe() {
        return batch_route(provider);
    }
    batch_route(&hark_config::Provider {
        live: false,
        model: provider
            .model
            .clone()
            .filter(|m| !m.starts_with("gpt-live-")),
        ..provider.clone()
    })
}

fn batch_route(provider: &hark_config::Provider) -> (hark_stt::ProviderKind, String) {
    let kind = match provider.kind {
        hark_config::ProviderKind::Deepgram => hark_stt::ProviderKind::Deepgram,
        hark_config::ProviderKind::Gemini => hark_stt::ProviderKind::GeminiLive,
        // Same endpoint, two contracts: the model decides which, so this arm is
        // guarded on the model rather than split across the provider kinds.
        hark_config::ProviderKind::Openai | hark_config::ProviderKind::OpenaiCompatible
            if provider.uses_keywords_biasing() =>
        {
            hark_stt::ProviderKind::OpenAiTranscribe
        }
        hark_config::ProviderKind::Openai | hark_config::ProviderKind::OpenaiCompatible => {
            hark_stt::ProviderKind::OpenAiCompatible
        }
    };
    (kind, provider.resolved_model())
}

/// Map the settings enum onto the STT crate's provider taxonomy (see
/// [`stt_route`]); OpenAI and any custom endpoint share the OpenAI-compatible
/// contract.
///
/// Public because the settings "Test connection" flow builds the exact same
/// config the pipeline would run with; a passing test therefore validates
/// the real path, not a parallel reimplementation.
pub fn provider_config(
    settings: &Settings,
    api_key: String,
) -> Result<ProviderConfig, PipelineError> {
    let (kind, model) = stt_route(&settings.provider);
    if settings.provider.uses_live_transcribe() && !LIVE_BUILD {
        log::warn!(
            "Live transcribe is on, but this build has no WebSocket client; \
             OpenAI dictation uses batch {model} instead"
        );
    }
    // Settings::load validated this; guard again for hand-built Settings.
    let base_url = settings.provider.resolved_base_url().ok_or_else(|| {
        PipelineError::Config("openai-compatible provider requires a base_url".to_string())
    })?;
    Ok(ProviderConfig {
        kind,
        label: settings.provider.kind.label().to_string(),
        base_url,
        model,
        api_key,
        bias_terms: settings.spellbook.terms(),
        // No settings path selects the batch fused adapter: the Interactions
        // API prototype is driven by its spike, not by the app. Gemini Live
        // does its fusing through `live_mode` instead.
        cleanup_instruction: None,
        live_mode: match settings.provider.live_mode {
            hark_config::LiveMode::Verbatim => hark_stt::gemini_live::TranscribeMode::Verbatim,
            hark_config::LiveMode::Smart => hark_stt::gemini_live::TranscribeMode::Smart,
        },
    })
}

/// The config for the batch adapter, on the [`batch_stt_route`]. For a
/// provider that streams, that adapter only ever runs when the stream did not
/// carry the dictation. `cfg` is the [`provider_config`] for `provider`: the
/// key, label, base URL and spellbook terms are kept, only the adapter, model
/// and transcript mode can differ.
///
/// OpenAI Live falls back to REST gpt-transcribe (or the pinned batch model),
/// not to a replay over the realtime socket. That replay would reuse the very
/// endpoint and `session.update` that just failed, so a proxy that blocks
/// WebSocket upgrades, a realtime outage, a key without access to the model,
/// or one spellbook term the API rejects would lose the dictation outright
/// unless on-device backup happened to be set up.
///
/// Gemini Live in SMART mode can strand a turn outright (see
/// `TranscribeMode::Smart`): a spoken edit command that leaves nothing makes
/// the server go silent until the budget runs out. Replaying the clip in SMART
/// repeats the exact request that just hung, and in one user's log it hung
/// again six times out of seven. The replay asks for the literal transcript
/// instead. It arrives with `cleaned: None`, so the user's cleanup voice still
/// polishes it, exactly as it would any verbatim dictation.
pub(crate) fn fallback_config(
    provider: &hark_config::Provider,
    cfg: &ProviderConfig,
) -> ProviderConfig {
    let (kind, model) = batch_stt_route(provider);
    ProviderConfig {
        kind,
        model,
        live_mode: hark_stt::gemini_live::TranscribeMode::Verbatim,
        ..cfg.clone()
    }
}

/// The config a meeting's live-transcript chunks run with: the batch route,
/// whatever Live transcribe says. A 20-30 s chunk is a finished clip with no
/// hold to overlap, so the realtime model would bill about 3.8x
/// gpt-transcribe's rate for no latency gain.
pub(crate) fn meeting_chunk_config(
    settings: &Settings,
    api_key: String,
) -> Result<ProviderConfig, PipelineError> {
    let cfg = provider_config(settings, api_key)?;
    Ok(fallback_config(&settings.provider, &cfg))
}

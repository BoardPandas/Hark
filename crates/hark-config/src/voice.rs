//! The `[voice]` settings section and cleanup-provider resolution.
//!
//! Cleanup is optional and fail-open: resolution never hard-errors. A
//! non-Verbatim voice with no resolvable provider degrades to Verbatim with a
//! warning marker the pipeline logs once at startup; the default config
//! (Deepgram STT + Clean voice) must keep working out of the box, so a hard
//! error here would break `Settings::load` on a missing file.

use crate::{ConfigError, Provider, ProviderKind};
use serde::{Deserialize, Serialize};

/// Which voice rewrites the transcript before injection. `Verbatim` never
/// makes a cleanup call at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VoiceName {
    Verbatim,
    Clean,
    Professional,
    Casual,
    Custom,
}

impl VoiceName {
    /// The kebab-case config/display label (matches the serde encoding).
    pub fn label(self) -> &'static str {
        match self {
            VoiceName::Verbatim => "verbatim",
            VoiceName::Clean => "clean",
            VoiceName::Professional => "professional",
            VoiceName::Casual => "casual",
            VoiceName::Custom => "custom",
        }
    }
}

/// The `[voice]` section.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Voice {
    /// The default voice for every dictation; `hark-cli --voice` overrides
    /// it per run.
    pub default: VoiceName,
    /// The user's system prompt for `default = "custom"`; validated
    /// non-empty in that case.
    pub custom_prompt: String,
    /// Corrected transcripts with fewer words than this skip the cleanup
    /// call (STT always runs). 0 disables the gate.
    pub skip_below_words: u32,
    /// Explicit cleanup provider; omit to inherit from an openai/groq STT
    /// provider (see `resolve_cleanup_provider`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<VoiceProvider>,
}

impl Default for Voice {
    fn default() -> Self {
        Voice {
            default: VoiceName::Clean,
            custom_prompt: String::new(),
            skip_below_words: 5,
            provider: None,
        }
    }
}

/// The optional `[voice.provider]` table. `kind` is required when the table
/// is present; everything else defaults per kind.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceProvider {
    /// openai | groq | openai-compatible. Deepgram has no chat product and
    /// is rejected at validation.
    pub kind: ProviderKind,
    /// Defaults per kind; required (validated) for `openai-compatible`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Defaults per kind (provisional research pins; live re-verification
    /// deferred to Phase 4 with the BYOK UI).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Serialized into the request only when present; the openai preset
    /// leaves it unset because the GPT-5 family rejects any non-default
    /// temperature. Groq preset: 0.2.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// OpenAI GPT-5 family only; openai preset "minimal", others unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    /// Keychain account override for the edge where two distinct
    /// openai-compatible endpoints would otherwise share one slot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_account: Option<String>,
}

/// Per-kind default chat model (§2.4 of the Phase 3 plan; provisional).
fn default_chat_model(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Openai => "gpt-5-nano",
        ProviderKind::Groq => "llama-3.1-8b-instant",
        // Validation guarantees explicit config for openai-compatible and
        // rejects deepgram outright; empty keeps the function total.
        ProviderKind::OpenaiCompatible | ProviderKind::Deepgram => "",
    }
}

fn preset_temperature(kind: ProviderKind) -> Option<f32> {
    match kind {
        ProviderKind::Groq => Some(0.2),
        _ => None,
    }
}

fn preset_reasoning_effort(kind: ProviderKind) -> Option<&'static str> {
    match kind {
        ProviderKind::Openai => Some("minimal"),
        _ => None,
    }
}

impl VoiceProvider {
    /// The effective base URL: explicit value, else the kind's default.
    /// Chat and STT share base URLs on openai/groq.
    pub fn resolved_base_url(&self) -> Option<String> {
        if let Some(url) = &self.base_url {
            return Some(url.clone());
        }
        match self.kind {
            ProviderKind::Openai => Some("https://api.openai.com/v1".to_string()),
            ProviderKind::Groq => Some("https://api.groq.com/openai/v1".to_string()),
            ProviderKind::OpenaiCompatible | ProviderKind::Deepgram => None,
        }
    }

    /// The effective model: explicit value, else the kind's default.
    pub fn resolved_model(&self) -> String {
        self.model
            .clone()
            .unwrap_or_else(|| default_chat_model(self.kind).to_string())
    }

    /// Explicit temperature wins; otherwise the kind's preset.
    pub fn resolved_temperature(&self) -> Option<f32> {
        self.temperature.or_else(|| preset_temperature(self.kind))
    }

    /// Explicit effort wins; otherwise the kind's preset.
    pub fn resolved_reasoning_effort(&self) -> Option<String> {
        self.reasoning_effort
            .clone()
            .or_else(|| preset_reasoning_effort(self.kind).map(str::to_string))
    }
}

pub(crate) fn validate(voice: &Voice) -> Result<(), ConfigError> {
    if voice.default == VoiceName::Custom && voice.custom_prompt.trim().is_empty() {
        return Err(ConfigError::Invalid(
            "voice.default = \"custom\" requires a non-empty voice.custom_prompt".to_string(),
        ));
    }
    if let Some(p) = &voice.provider {
        if p.kind == ProviderKind::Deepgram {
            return Err(ConfigError::Invalid(
                "voice.provider.kind = \"deepgram\" is invalid: Deepgram has no \
                 chat-completions product"
                    .to_string(),
            ));
        }
        if p.kind == ProviderKind::OpenaiCompatible && p.base_url.is_none() {
            return Err(ConfigError::Invalid(
                "voice.provider.kind = \"openai-compatible\" requires an explicit \
                 voice.provider.base_url"
                    .to_string(),
            ));
        }
    }
    Ok(())
}

/// Where the pipeline gets the cleanup API key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CleanupKeySource {
    /// Inherit path: reuse the already-resolved STT key; the keychain is
    /// never read a second time.
    ReuseSttKey,
    /// Resolve via the cleanup env override or the OS keychain, under this
    /// account name (the provider label, unless `key_account` overrides it).
    Account(String),
}

/// A fully-resolved cleanup provider: everything the pipeline needs to build
/// the adapter except the key itself. Debug is safe: no key material here.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedCleanupProvider {
    pub kind: ProviderKind,
    pub base_url: String,
    pub model: String,
    pub temperature: Option<f32>,
    pub reasoning_effort: Option<String>,
    pub key_source: CleanupKeySource,
}

/// The outcome of cleanup-provider resolution at pipeline build.
#[derive(Debug, Clone, PartialEq)]
pub enum CleanupResolution {
    /// The effective voice is Verbatim: no provider needed, no call made.
    Verbatim,
    /// A usable cleanup provider.
    Resolved(ResolvedCleanupProvider),
    /// A non-Verbatim voice with no resolvable provider: the pipeline logs
    /// `reason` once at startup and runs Verbatim.
    VerbatimWithWarning { reason: String },
}

/// Resolve the cleanup provider. Pure: unit-testable without keychain or
/// network. Rules, in order:
///
/// 1. Effective voice Verbatim: no provider, no warning.
/// 2. Explicit `[voice.provider]` wins; its key account is `key_account`
///    when set, else the kind's label.
/// 3. Absent: an openai/groq STT provider is inherited (same kind, same
///    resolved base_url, the kind's default chat model and presets) and the
///    already-resolved STT key is reused. An openai-compatible STT endpoint
///    is NOT inherited: speaking /audio/transcriptions does not imply
///    /chat/completions.
/// 4. Otherwise degrade to Verbatim with a warning.
pub fn resolve_cleanup_provider(
    stt: &Provider,
    voice: &Voice,
    effective_voice: VoiceName,
) -> CleanupResolution {
    if effective_voice == VoiceName::Verbatim {
        return CleanupResolution::Verbatim;
    }
    if let Some(p) = &voice.provider {
        // Validation already rejected deepgram and base_url-less
        // openai-compatible tables; degrade defensively if it was skipped.
        let Some(base_url) = p.resolved_base_url() else {
            return CleanupResolution::VerbatimWithWarning {
                reason: format!(
                    "voice.provider \"{}\" has no usable base_url; running verbatim",
                    p.kind.label()
                ),
            };
        };
        let account = p
            .key_account
            .clone()
            .unwrap_or_else(|| p.kind.label().to_string());
        return CleanupResolution::Resolved(ResolvedCleanupProvider {
            kind: p.kind,
            base_url,
            model: p.resolved_model(),
            temperature: p.resolved_temperature(),
            reasoning_effort: p.resolved_reasoning_effort(),
            key_source: CleanupKeySource::Account(account),
        });
    }
    match stt.kind {
        ProviderKind::Openai | ProviderKind::Groq => {
            CleanupResolution::Resolved(ResolvedCleanupProvider {
                kind: stt.kind,
                base_url: stt
                    .resolved_base_url()
                    .expect("openai/groq always resolve a base URL"),
                model: default_chat_model(stt.kind).to_string(),
                temperature: preset_temperature(stt.kind),
                reasoning_effort: preset_reasoning_effort(stt.kind).map(str::to_string),
                key_source: CleanupKeySource::ReuseSttKey,
            })
        }
        ProviderKind::Deepgram | ProviderKind::OpenaiCompatible => {
            CleanupResolution::VerbatimWithWarning {
                reason: format!(
                    "a non-verbatim voice is configured but the \"{}\" STT provider cannot \
                     be inherited for chat cleanup and no [voice.provider] is set; \
                     running verbatim",
                    stt.kind.label()
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Settings;

    fn resolve(s: &Settings) -> CleanupResolution {
        resolve_cleanup_provider(&s.provider, &s.voice, s.voice.default)
    }

    #[test]
    fn empty_toml_yields_clean_voice_with_no_provider_table() {
        let s = Settings::from_toml("").expect("empty TOML parses");
        assert_eq!(s.voice.default, VoiceName::Clean);
        assert_eq!(s.voice.skip_below_words, 5);
        assert!(s.voice.custom_prompt.is_empty());
        assert!(s.voice.provider.is_none());
    }

    #[test]
    fn default_config_degrades_to_verbatim_with_warning() {
        // Default config is Deepgram STT + Clean voice: must load fine and
        // resolve to a warning, never a hard error.
        let s = Settings::from_toml("").expect("defaults load");
        match resolve(&s) {
            CleanupResolution::VerbatimWithWarning { reason } => {
                assert!(reason.contains("deepgram"));
                assert!(reason.contains("running verbatim"));
            }
            other => panic!("expected VerbatimWithWarning, got {other:?}"),
        }
    }

    #[test]
    fn openai_stt_is_inherited_with_presets_and_key_reuse() {
        let s = Settings::from_toml("[provider]\nkind = \"openai\"").expect("parses");
        match resolve(&s) {
            CleanupResolution::Resolved(p) => {
                assert_eq!(p.kind, ProviderKind::Openai);
                assert_eq!(p.base_url, "https://api.openai.com/v1");
                assert_eq!(p.model, "gpt-5-nano");
                assert_eq!(p.temperature, None, "GPT-5 family: temperature stays unset");
                assert_eq!(p.reasoning_effort.as_deref(), Some("minimal"));
                assert_eq!(p.key_source, CleanupKeySource::ReuseSttKey);
            }
            other => panic!("expected Resolved, got {other:?}"),
        }
    }

    #[test]
    fn groq_stt_is_inherited_with_groq_presets() {
        let s = Settings::from_toml("[provider]\nkind = \"groq\"").expect("parses");
        match resolve(&s) {
            CleanupResolution::Resolved(p) => {
                assert_eq!(p.kind, ProviderKind::Groq);
                assert_eq!(p.base_url, "https://api.groq.com/openai/v1");
                assert_eq!(p.model, "llama-3.1-8b-instant");
                assert_eq!(p.temperature, Some(0.2));
                assert_eq!(p.reasoning_effort, None);
                assert_eq!(p.key_source, CleanupKeySource::ReuseSttKey);
            }
            other => panic!("expected Resolved, got {other:?}"),
        }
    }

    #[test]
    fn inherit_carries_a_custom_stt_base_url() {
        let s = Settings::from_toml(
            "[provider]\nkind = \"openai\"\nbase_url = \"http://localhost:9000/v1\"",
        )
        .expect("parses");
        match resolve(&s) {
            CleanupResolution::Resolved(p) => {
                assert_eq!(p.base_url, "http://localhost:9000/v1");
            }
            other => panic!("expected Resolved, got {other:?}"),
        }
    }

    #[test]
    fn openai_compatible_stt_is_not_inherited() {
        // Speaking /audio/transcriptions does not imply /chat/completions.
        let s = Settings::from_toml(
            "[provider]\nkind = \"openai-compatible\"\nbase_url = \"http://localhost:8000/v1\"",
        )
        .expect("parses");
        match resolve(&s) {
            CleanupResolution::VerbatimWithWarning { reason } => {
                assert!(reason.contains("openai-compatible"));
            }
            other => panic!("expected VerbatimWithWarning, got {other:?}"),
        }
    }

    #[test]
    fn verbatim_voice_never_resolves_a_provider() {
        // Even with a perfectly good explicit provider table.
        let s = Settings::from_toml(
            r#"
            [provider]
            kind = "openai"

            [voice]
            default = "verbatim"

            [voice.provider]
            kind = "groq"
            "#,
        )
        .expect("parses");
        assert_eq!(resolve(&s), CleanupResolution::Verbatim);
    }

    #[test]
    fn explicit_provider_table_wins_over_inherit() {
        let s = Settings::from_toml(
            r#"
            [provider]
            kind = "groq"

            [voice.provider]
            kind = "openai"
            "#,
        )
        .expect("parses");
        match resolve(&s) {
            CleanupResolution::Resolved(p) => {
                assert_eq!(p.kind, ProviderKind::Openai);
                assert_eq!(p.model, "gpt-5-nano");
                // Explicit table resolves its own key; no STT reuse.
                assert_eq!(
                    p.key_source,
                    CleanupKeySource::Account("openai".to_string())
                );
            }
            other => panic!("expected Resolved, got {other:?}"),
        }
    }

    #[test]
    fn explicit_values_override_kind_presets() {
        let s = Settings::from_toml(
            r#"
            [voice.provider]
            kind = "openai"
            model = "gpt-4.1-mini"
            temperature = 0.5
            "#,
        )
        .expect("parses");
        match resolve(&s) {
            CleanupResolution::Resolved(p) => {
                assert_eq!(p.model, "gpt-4.1-mini");
                assert_eq!(p.temperature, Some(0.5));
                // Unset fields still take the kind's preset.
                assert_eq!(p.reasoning_effort.as_deref(), Some("minimal"));
            }
            other => panic!("expected Resolved, got {other:?}"),
        }
    }

    #[test]
    fn key_account_override_is_honored() {
        let s = Settings::from_toml(
            r#"
            [voice.provider]
            kind = "openai-compatible"
            base_url = "http://localhost:8000/v1"
            key_account = "my-alt-endpoint"
            "#,
        )
        .expect("parses");
        match resolve(&s) {
            CleanupResolution::Resolved(p) => {
                assert_eq!(
                    p.key_source,
                    CleanupKeySource::Account("my-alt-endpoint".to_string())
                );
                assert_eq!(p.base_url, "http://localhost:8000/v1");
                assert_eq!(p.model, "", "openai-compatible has no default model");
            }
            other => panic!("expected Resolved, got {other:?}"),
        }
    }

    #[test]
    fn cleanup_key_accounts_match_stable_provider_labels() {
        // The keychain account derives from these labels (same slots the
        // Phase 4 UI paste field will write); changing one orphans keys.
        for (kind, label) in [
            (ProviderKind::Openai, "openai"),
            (ProviderKind::Groq, "groq"),
            (ProviderKind::OpenaiCompatible, "openai-compatible"),
        ] {
            let voice = Voice {
                provider: Some(VoiceProvider {
                    kind,
                    base_url: Some("http://localhost:8000/v1".to_string()),
                    model: None,
                    temperature: None,
                    reasoning_effort: None,
                    key_account: None,
                }),
                ..Voice::default()
            };
            match resolve_cleanup_provider(&Provider::default(), &voice, VoiceName::Clean) {
                CleanupResolution::Resolved(p) => {
                    assert_eq!(p.key_source, CleanupKeySource::Account(label.to_string()));
                }
                other => panic!("expected Resolved for {label}, got {other:?}"),
            }
        }
    }

    #[test]
    fn deepgram_as_cleanup_provider_is_rejected() {
        let err = Settings::from_toml("[voice.provider]\nkind = \"deepgram\"")
            .expect_err("deepgram cleanup must be rejected");
        assert!(matches!(err, ConfigError::Invalid(_)));
        assert!(err.to_string().contains("deepgram"));
    }

    #[test]
    fn custom_voice_requires_a_prompt() {
        let err = Settings::from_toml("[voice]\ndefault = \"custom\"")
            .expect_err("custom without prompt must be rejected");
        assert!(matches!(err, ConfigError::Invalid(_)));

        let blank = Settings::from_toml("[voice]\ndefault = \"custom\"\ncustom_prompt = \"  \"")
            .expect_err("blank prompt must be rejected");
        assert!(matches!(blank, ConfigError::Invalid(_)));

        let s = Settings::from_toml(
            "[voice]\ndefault = \"custom\"\ncustom_prompt = \"Rewrite as a pirate.\"",
        )
        .expect("custom with prompt is valid");
        assert_eq!(s.voice.default, VoiceName::Custom);
    }

    #[test]
    fn openai_compatible_cleanup_requires_base_url() {
        let err = Settings::from_toml("[voice.provider]\nkind = \"openai-compatible\"")
            .expect_err("missing base_url must be rejected");
        assert!(matches!(err, ConfigError::Invalid(_)));
    }

    #[test]
    fn voice_section_parses_all_keys() {
        let s = Settings::from_toml(
            r#"
            [voice]
            default = "professional"
            skip_below_words = 3

            [voice.provider]
            kind = "groq"
            model = "openai/gpt-oss-20b"
            reasoning_effort = "low"
            "#,
        )
        .expect("parses");
        assert_eq!(s.voice.default, VoiceName::Professional);
        assert_eq!(s.voice.skip_below_words, 3);
        match resolve(&s) {
            CleanupResolution::Resolved(p) => {
                assert_eq!(p.model, "openai/gpt-oss-20b");
                assert_eq!(p.reasoning_effort.as_deref(), Some("low"));
                assert_eq!(p.temperature, Some(0.2), "groq preset still applies");
            }
            other => panic!("expected Resolved, got {other:?}"),
        }
    }
}

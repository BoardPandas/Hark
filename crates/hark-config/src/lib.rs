//! Hark settings: `Settings` struct + TOML loader with defaults. The SQLite
//! half of the planned `hark-store` is deferred to Phase 4; this crate is a
//! deliberately-minimal stand-in for the settings half.
//!
//! API keys never live in TOML: they come from the OS keychain or the
//! `HARK_STT_KEY` env override via `hark-keychain`.

use serde::Deserialize;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("cannot read config file {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot parse config file: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("invalid config: {0}")]
    Invalid(String),
}

/// Which provider preset the user picked. `OpenaiCompatible` is the escape
/// hatch for any other endpoint speaking the multipart
/// `/audio/transcriptions` contract; it requires an explicit `base_url`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderKind {
    Deepgram,
    Openai,
    Groq,
    OpenaiCompatible,
}

impl ProviderKind {
    /// Short label for reports, errors, and the keychain account name.
    pub fn label(self) -> &'static str {
        match self {
            ProviderKind::Deepgram => "deepgram",
            ProviderKind::Openai => "openai",
            ProviderKind::Groq => "groq",
            ProviderKind::OpenaiCompatible => "openai-compatible",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Provider {
    pub kind: ProviderKind,
    /// Defaults per kind; required (validated) for `openai-compatible`.
    pub base_url: Option<String>,
    /// Defaults per kind.
    pub model: Option<String>,
}

impl Default for Provider {
    fn default() -> Self {
        Provider {
            kind: ProviderKind::Deepgram,
            base_url: None,
            model: None,
        }
    }
}

impl Provider {
    /// The effective base URL: explicit value, else the kind's default.
    pub fn resolved_base_url(&self) -> Option<String> {
        if let Some(url) = &self.base_url {
            return Some(url.clone());
        }
        match self.kind {
            ProviderKind::Deepgram => Some("https://api.deepgram.com".to_string()),
            ProviderKind::Openai => Some("https://api.openai.com/v1".to_string()),
            ProviderKind::Groq => Some("https://api.groq.com/openai/v1".to_string()),
            ProviderKind::OpenaiCompatible => None,
        }
    }

    /// The effective model: explicit value, else the kind's default.
    /// The spike verdict (2026-07-16) made Deepgram nova-3 the app default.
    pub fn resolved_model(&self) -> String {
        if let Some(model) = &self.model {
            return model.clone();
        }
        match self.kind {
            ProviderKind::Deepgram => "nova-3",
            ProviderKind::Openai => "gpt-4o-mini-transcribe",
            ProviderKind::Groq => "whisper-large-v3-turbo",
            // Validation guarantees openai-compatible configs are explicit;
            // an empty model is still a valid request for some servers.
            ProviderKind::OpenaiCompatible => "",
        }
        .to_string()
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Hotkey {
    /// "+"-separated chord; hold ALL keys to record, release any to stop.
    /// Parsed by `hark-hotkey` at startup (unknown names error there).
    pub ptt_key: String,
}

impl Default for Hotkey {
    fn default() -> Self {
        Hotkey {
            // Confirmed with the user 2026-07-16: chord, not a single key.
            ptt_key: "LCtrl+LWin".to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Audio {
    /// Audio kept from before the chord registers (catches led-in words).
    pub preroll_ms: u32,
    /// Audio kept after release (catches trailing word endings). Adds
    /// directly to release-to-inject latency; 150 ms confirmed 2026-07-16.
    pub tail_ms: u32,
    /// Max hold; on exceed, transcribe what we have (ring sized from this).
    pub max_hold_s: u32,
    /// Holds shorter than this are dropped without a network request.
    pub min_speech_ms: u32,
    /// Clips whose RMS is below this are dropped without a network request.
    pub silence_rms: f32,
}

impl Default for Audio {
    fn default() -> Self {
        Audio {
            preroll_ms: 300,
            tail_ms: 150,
            max_hold_s: 120,
            min_speech_ms: 250,
            silence_rms: 0.01,
        }
    }
}

/// How transcribed text reaches the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InjectStrategy {
    /// Clipboard stash -> set -> Ctrl+V -> restore (fast, default).
    Clipboard,
    /// Character-by-character typing (slower; paste-hostile fields).
    Type,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Inject {
    pub strategy: InjectStrategy,
    /// Delay between clipboard set and the synthesized paste. No OS-level
    /// guarantee exists; ~30-50 ms is the community rule of thumb, tuned on
    /// real hardware (spec §8).
    pub set_paste_delay_ms: u64,
    /// Delay between the synthesized paste and restoring the old clipboard.
    pub paste_restore_delay_ms: u64,
    /// Bounded retries when another process holds the clipboard open.
    pub clipboard_retries: u32,
}

impl Default for Inject {
    fn default() -> Self {
        Inject {
            strategy: InjectStrategy::Clipboard,
            set_paste_delay_ms: 50,
            paste_restore_delay_ms: 50,
            clipboard_retries: 8,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Dictionary {
    /// Canonical terms: phonetic post-correction targets and the source for
    /// provider biasing. The alias keeps pre-Phase-2 config files working.
    #[serde(alias = "bias_terms")]
    pub terms: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Settings {
    pub provider: Provider,
    pub hotkey: Hotkey,
    pub audio: Audio,
    pub inject: Inject,
    pub dictionary: Dictionary,
}

impl Settings {
    /// Parse settings from TOML text. Unknown keys are tolerated (forward
    /// compatibility); missing keys take defaults; the result is validated.
    pub fn from_toml(text: &str) -> Result<Settings, ConfigError> {
        let settings: Settings = toml::from_str(text)?;
        settings.validate()?;
        Ok(settings)
    }

    /// Load settings from a file. A missing file is not an error: it yields
    /// the defaults (first-run onboarding is Phase 4).
    pub fn load(path: &Path) -> Result<Settings, ConfigError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Settings::from_toml(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(e) => Err(ConfigError::Io {
                path: path.display().to_string(),
                source: e,
            }),
        }
    }

    fn validate(&self) -> Result<(), ConfigError> {
        if self.provider.kind == ProviderKind::OpenaiCompatible && self.provider.base_url.is_none()
        {
            return Err(ConfigError::Invalid(
                "provider.kind = \"openai-compatible\" requires an explicit provider.base_url"
                    .to_string(),
            ));
        }
        if self.hotkey.ptt_key.trim().is_empty() {
            return Err(ConfigError::Invalid(
                "hotkey.ptt_key must not be empty".to_string(),
            ));
        }
        if self.audio.max_hold_s == 0 {
            return Err(ConfigError::Invalid(
                "audio.max_hold_s must be at least 1".to_string(),
            ));
        }
        Ok(())
    }
}

/// The per-user config file location. `None` when the OS gives us no home
/// (headless CI); callers fall back to defaults.
pub fn default_config_path() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("hark").join("config.toml"))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(|h| {
            PathBuf::from(h)
                .join("Library")
                .join("Application Support")
                .join("hark")
                .join("config.toml")
        })
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        std::env::var_os("HOME").map(|h| {
            PathBuf::from(h)
                .join(".config")
                .join("hark")
                .join("config.toml")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_toml_yields_all_defaults() {
        let s = Settings::from_toml("").expect("empty TOML parses");
        assert_eq!(s.provider.kind, ProviderKind::Deepgram);
        assert_eq!(
            s.provider.resolved_base_url().as_deref(),
            Some("https://api.deepgram.com")
        );
        assert_eq!(s.provider.resolved_model(), "nova-3");
        assert_eq!(s.hotkey.ptt_key, "LCtrl+LWin");
        assert_eq!(s.audio.preroll_ms, 300);
        assert_eq!(s.audio.tail_ms, 150);
        assert_eq!(s.audio.max_hold_s, 120);
        assert_eq!(s.inject.strategy, InjectStrategy::Clipboard);
        assert!(s.dictionary.terms.is_empty());
    }

    #[test]
    fn explicit_values_override_defaults() {
        let s = Settings::from_toml(
            r#"
            [provider]
            kind = "groq"
            model = "whisper-large-v3"

            [hotkey]
            ptt_key = "RCtrl"

            [audio]
            tail_ms = 200

            [inject]
            strategy = "type"

            [dictionary]
            terms = ["Hark", "Levenshtein"]
            "#,
        )
        .expect("valid TOML parses");
        assert_eq!(s.provider.kind, ProviderKind::Groq);
        assert_eq!(
            s.provider.resolved_base_url().as_deref(),
            Some("https://api.groq.com/openai/v1")
        );
        assert_eq!(s.provider.resolved_model(), "whisper-large-v3");
        assert_eq!(s.hotkey.ptt_key, "RCtrl");
        assert_eq!(s.audio.tail_ms, 200);
        // Untouched keys keep their defaults.
        assert_eq!(s.audio.preroll_ms, 300);
        assert_eq!(s.inject.strategy, InjectStrategy::Type);
        assert_eq!(s.dictionary.terms, vec!["Hark", "Levenshtein"]);
    }

    #[test]
    fn legacy_bias_terms_key_still_parses_via_alias() {
        // Pre-Phase-2 config files used `bias_terms`; the serde alias must
        // keep them loading forever.
        let s = Settings::from_toml("[dictionary]\nbias_terms = [\"Modero\"]")
            .expect("legacy key parses");
        assert_eq!(s.dictionary.terms, vec!["Modero"]);
    }

    #[test]
    fn unknown_keys_are_tolerated() {
        let s = Settings::from_toml(
            r#"
            future_top_level = "ignored"

            [provider]
            kind = "openai"
            future_key = 42

            [some_future_section]
            x = 1
            "#,
        )
        .expect("unknown keys must not fail the parse");
        assert_eq!(s.provider.kind, ProviderKind::Openai);
        assert_eq!(s.provider.resolved_model(), "gpt-4o-mini-transcribe");
    }

    #[test]
    fn openai_compatible_requires_base_url() {
        let err = Settings::from_toml("[provider]\nkind = \"openai-compatible\"")
            .expect_err("missing base_url must be rejected");
        assert!(matches!(err, ConfigError::Invalid(_)));

        let s = Settings::from_toml(
            "[provider]\nkind = \"openai-compatible\"\nbase_url = \"http://localhost:8000/v1\"",
        )
        .expect("explicit base_url is accepted");
        assert_eq!(
            s.provider.resolved_base_url().as_deref(),
            Some("http://localhost:8000/v1")
        );
    }

    #[test]
    fn malformed_toml_is_a_parse_error() {
        let err = Settings::from_toml("[provider\nkind=").expect_err("bad TOML must fail");
        assert!(matches!(err, ConfigError::Parse(_)));
    }

    #[test]
    fn empty_ptt_key_is_invalid() {
        let err = Settings::from_toml("[hotkey]\nptt_key = \"  \"")
            .expect_err("blank chord must be rejected");
        assert!(matches!(err, ConfigError::Invalid(_)));
    }

    #[test]
    fn missing_file_loads_defaults() {
        let s = Settings::load(Path::new("Z:/definitely/not/here/config.toml"))
            .expect("missing file falls back to defaults");
        assert_eq!(s.provider.kind, ProviderKind::Deepgram);
    }

    #[test]
    fn provider_labels_are_stable() {
        // The keychain account name derives from these; changing one silently
        // orphans stored keys.
        assert_eq!(ProviderKind::Deepgram.label(), "deepgram");
        assert_eq!(ProviderKind::Openai.label(), "openai");
        assert_eq!(ProviderKind::Groq.label(), "groq");
        assert_eq!(ProviderKind::OpenaiCompatible.label(), "openai-compatible");
    }
}

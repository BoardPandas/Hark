//! Hark settings: `Settings` struct + TOML loader/saver with defaults.
//! SQLite storage lives in `hark-store`; this crate owns the TOML half.
//!
//! API keys never live in TOML: they come from the OS keychain or the
//! `HARK_STT_KEY` / `HARK_CLEANUP_KEY` env overrides via `hark-keychain`.
//!
//! Saving serializes the full `Settings` model back to disk: unknown keys a
//! user hand-added are dropped. Acceptable while the schema is additive
//! (Phase 4 spec §5); the struct is the source of truth.

mod voice;

pub use voice::{
    resolve_cleanup_provider, CleanupKeySource, CleanupResolution, ResolvedCleanupProvider, Voice,
    VoiceName, VoiceProvider,
};

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Current config schema version, stamped into every saved file. Bump only
/// on a breaking schema change; the first bump must ship the
/// backup-then-migrate flow (BP `versioned-config-migration-backup`:
/// back up as `config.toml.v{version}.bak`, map fields explicitly, persist).
pub const CONFIG_VERSION: u32 = 1;

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
    #[error("cannot serialize config: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error("cannot write config file {path}: {source}")]
    Write {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid config: {0}")]
    Invalid(String),
}

/// Which provider preset the user picked. `OpenaiCompatible` is the escape
/// hatch for any other endpoint speaking the multipart
/// `/audio/transcriptions` contract; it requires an explicit `base_url`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Provider {
    pub kind: ProviderKind,
    /// Defaults per kind; required (validated) for `openai-compatible`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Defaults per kind.
    #[serde(skip_serializing_if = "Option::is_none")]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InjectStrategy {
    /// Clipboard stash -> set -> Ctrl+V -> restore (fast, default).
    Clipboard,
    /// Character-by-character typing (slower; paste-hostile fields).
    Type,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Dictionary {
    /// Canonical terms: phonetic post-correction targets and the source for
    /// provider biasing. The alias keeps pre-Phase-2 config files working.
    #[serde(alias = "bias_terms")]
    pub terms: Vec<String>,
}

/// The `[history]` section: local capture + retention. Content capture and
/// numeric stats are deliberately independent: with `capture = false` no
/// dictation text is persisted, but the counters in the stats row still
/// tick (they carry no content).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct History {
    /// `false`: no dictation content is stored; counters still tick.
    pub capture: bool,
    /// Keep at most this many newest entries (>= 1).
    pub max_entries: u32,
    /// Delete entries older than this many days (>= 1).
    pub max_age_days: u32,
}

impl Default for History {
    fn default() -> Self {
        History {
            // Retention defaults confirmed with the user 2026-07-16:
            // 1,000 entries or 90 days, whichever prunes first.
            capture: true,
            max_entries: 1_000,
            max_age_days: 90,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Config schema stamp; fresh installs write [`CONFIG_VERSION`]. Files
    /// predating the stamp deserialize to the current value (the schema has
    /// only ever been additive, so they are current-generation files).
    pub version: u32,
    pub provider: Provider,
    pub hotkey: Hotkey,
    pub audio: Audio,
    pub inject: Inject,
    pub dictionary: Dictionary,
    pub voice: Voice,
    pub history: History,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            version: CONFIG_VERSION,
            provider: Provider::default(),
            hotkey: Hotkey::default(),
            audio: Audio::default(),
            inject: Inject::default(),
            dictionary: Dictionary::default(),
            voice: Voice::default(),
            history: History::default(),
        }
    }
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

    /// Serialize to TOML text. The version stamp always serializes, so
    /// every saved file is self-describing.
    pub fn to_toml(&self) -> Result<String, ConfigError> {
        Ok(toml::to_string_pretty(self)?)
    }

    /// Validate, then write to `path`: parent dirs created, content written
    /// to a sibling temp file and renamed into place, so a crash mid-save
    /// can never leave a truncated config behind.
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        self.validate()?;
        let text = self.to_toml()?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| ConfigError::Write {
                path: dir.display().to_string(),
                source: e,
            })?;
        }
        let tmp = path.with_extension("toml.tmp");
        let write_err = |source: std::io::Error| ConfigError::Write {
            path: path.display().to_string(),
            source,
        };
        std::fs::write(&tmp, text).map_err(write_err)?;
        std::fs::rename(&tmp, path).map_err(|e| {
            // Best-effort cleanup; the temp file is harmless if it stays.
            let _ = std::fs::remove_file(&tmp);
            write_err(e)
        })
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
        if self.history.max_entries == 0 {
            return Err(ConfigError::Invalid(
                "history.max_entries must be at least 1".to_string(),
            ));
        }
        if self.history.max_age_days == 0 {
            return Err(ConfigError::Invalid(
                "history.max_age_days must be at least 1".to_string(),
            ));
        }
        voice::validate(&self.voice)?;
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

/// The per-user data directory (the history database lives here). `None`
/// when the OS gives us no home (headless CI); callers decide their
/// fallback. On Windows this coincides with the config dir (%APPDATA%\hark).
pub fn default_data_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("hark"))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(|h| {
            PathBuf::from(h)
                .join("Library")
                .join("Application Support")
                .join("hark")
        })
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        if let Some(x) = std::env::var_os("XDG_DATA_HOME") {
            return Some(PathBuf::from(x).join("hark"));
        }
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share").join("hark"))
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
    fn version_defaults_to_current_and_explicit_value_parses() {
        let s = Settings::from_toml("").expect("empty TOML parses");
        assert_eq!(
            s.version, CONFIG_VERSION,
            "pre-stamp files are current-generation files"
        );

        let s = Settings::from_toml("version = 1").expect("explicit stamp parses");
        assert_eq!(s.version, 1);
    }

    #[test]
    fn history_defaults_match_the_confirmed_retention() {
        let s = Settings::from_toml("").expect("empty TOML parses");
        assert!(s.history.capture);
        assert_eq!(s.history.max_entries, 1_000);
        assert_eq!(s.history.max_age_days, 90);
    }

    #[test]
    fn history_section_parses_and_zero_caps_are_rejected() {
        let s =
            Settings::from_toml("[history]\ncapture = false\nmax_entries = 50\nmax_age_days = 7")
                .expect("history section parses");
        assert!(!s.history.capture);
        assert_eq!(s.history.max_entries, 50);
        assert_eq!(s.history.max_age_days, 7);

        for bad in ["[history]\nmax_entries = 0", "[history]\nmax_age_days = 0"] {
            let err = Settings::from_toml(bad).expect_err("zero caps must be rejected");
            assert!(matches!(err, ConfigError::Invalid(_)), "{bad}");
        }
    }

    #[test]
    fn to_toml_stamps_the_version_and_omits_none_fields() {
        let text = Settings::default().to_toml().expect("defaults serialize");
        assert!(
            text.contains("version = 1"),
            "saved files are self-describing: {text}"
        );
        assert!(
            !text.contains("base_url"),
            "unset options must not serialize as nulls or empties: {text}"
        );
        // What we write must load back.
        Settings::from_toml(&text).expect("serialized defaults re-parse");
    }

    #[test]
    fn save_then_load_round_trips_non_default_settings() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nested").join("config.toml");

        let mut s = Settings::default();
        s.provider.kind = ProviderKind::Groq;
        s.provider.model = Some("whisper-large-v3".to_string());
        s.hotkey.ptt_key = "RCtrl".to_string();
        s.dictionary.terms = vec!["Hark".to_string(), "Modero".to_string()];
        s.history.capture = false;
        s.history.max_entries = 250;
        s.voice.default = VoiceName::Professional;
        s.voice.provider = Some(VoiceProvider {
            kind: ProviderKind::Openai,
            base_url: None,
            model: Some("gpt-5-nano".to_string()),
            temperature: None,
            reasoning_effort: Some("minimal".to_string()),
            key_account: None,
        });

        s.save(&path).expect("save creates parent dirs and writes");
        assert!(
            !path.with_extension("toml.tmp").exists(),
            "temp file is renamed away"
        );

        let loaded = Settings::load(&path).expect("saved file loads");
        assert_eq!(loaded.version, CONFIG_VERSION);
        assert_eq!(loaded.provider.kind, ProviderKind::Groq);
        assert_eq!(loaded.provider.model.as_deref(), Some("whisper-large-v3"));
        assert_eq!(loaded.provider.base_url, None);
        assert_eq!(loaded.hotkey.ptt_key, "RCtrl");
        assert_eq!(loaded.dictionary.terms, vec!["Hark", "Modero"]);
        assert!(!loaded.history.capture);
        assert_eq!(loaded.history.max_entries, 250);
        assert_eq!(loaded.voice.default, VoiceName::Professional);
        let p = loaded.voice.provider.expect("provider table round-trips");
        assert_eq!(p.kind, ProviderKind::Openai);
        assert_eq!(p.model.as_deref(), Some("gpt-5-nano"));
        assert_eq!(p.temperature, None);
        assert_eq!(p.reasoning_effort.as_deref(), Some("minimal"));

        // Untouched sections keep their defaults through the round trip.
        assert_eq!(loaded.audio.preroll_ms, 300);
        assert_eq!(loaded.inject.strategy, InjectStrategy::Clipboard);
    }

    #[test]
    fn saving_over_an_existing_file_replaces_it() {
        // The UI saves repeatedly over the same path; the rename step must
        // replace the existing config on Windows as well as Unix.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");

        let mut s = Settings::default();
        s.hotkey.ptt_key = "RCtrl".to_string();
        s.save(&path).expect("first save");

        s.hotkey.ptt_key = "LAlt".to_string();
        s.save(&path).expect("second save over the existing file");

        let loaded = Settings::load(&path).expect("loads");
        assert_eq!(loaded.hotkey.ptt_key, "LAlt");
    }

    #[test]
    fn save_validates_before_writing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");

        let mut s = Settings::default();
        s.history.max_entries = 0;
        let err = s.save(&path).expect_err("invalid settings must not save");
        assert!(matches!(err, ConfigError::Invalid(_)));
        assert!(!path.exists(), "nothing may be written on a failed save");
    }

    #[test]
    fn legacy_bias_terms_saves_back_as_terms() {
        // A config loaded through the legacy alias serializes under the
        // canonical key, quietly upgrading the file on the next save.
        let s = Settings::from_toml("[dictionary]\nbias_terms = [\"Modero\"]")
            .expect("legacy key parses");
        let text = s.to_toml().expect("serializes");
        assert!(text.contains("terms"), "{text}");
        assert!(!text.contains("bias_terms"), "{text}");
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

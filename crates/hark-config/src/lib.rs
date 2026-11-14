//! Hark settings: `Settings` struct + TOML loader/saver with defaults.
//! SQLite storage lives in `hark-store`; this crate owns the TOML half.
//!
//! API keys never live in TOML: they come from the OS keychain or the
//! `HARK_STT_KEY` / `HARK_CLEANUP_KEY` env overrides via `hark-keychain`.
//!
//! Saving serializes the full `Settings` model back to disk: unknown keys a
//! user hand-added are dropped. Acceptable while the schema is additive
//! (Phase 4 spec §5); the struct is the source of truth.

mod audio;
mod general;
mod groq_migration;
mod history;
mod hotkey;
mod inject;
mod insights;
mod invocations;
mod load_save;
mod local;
mod meeting;
mod output;
mod provider;
mod spellbook;
mod startup;
mod updates;
mod voice;

pub use audio::Audio;
pub use general::General;
pub use history::History;
pub use hotkey::Hotkey;
pub use inject::{Inject, InjectStrategy};
pub use insights::Insights;
pub use invocations::{Invocation, Invocations, Scope};
pub use local::{LocalMode, LocalStt, DEFAULT_FALLBACK_AFTER_MS, DEFAULT_MODEL};
pub use meeting::{AutoDetect, FinalPass, Meeting, SystemSource};
pub use output::Output;
pub use provider::{LiveMode, Provider, ProviderKind, OPENAI_LIVE_MODEL};
pub use spellbook::{Spellbook, SpellbookEntry};
pub use startup::Startup;
pub use updates::Updates;
pub use voice::{
    resolve_cleanup_provider, CleanupKeySource, CleanupResolution, ResolvedCleanupProvider, Voice,
    VoiceName, VoiceProvider,
};

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

/// Current config schema version, stamped into every saved file.
///
/// Bumped to 2 in 0.29.0, when `[spellbook] terms = [...]` became
/// `[[spellbook.entries]]` with aliases. That bump ships the flow BP
/// `versioned-config-migration-backup` prescribes and this comment has
/// promised since version 1: back up as `config.toml.v{version}.bak` *before*
/// any mutation, map fields explicitly, stamp the new version, persist
/// immediately. Retired fields stay deserializable so old files still parse
/// for the migration to read.
///
/// Bumped to 3 in 0.50.3, when the meeting auto-stop default fell from 60 s to
/// 15 s (see [`meeting::migrate`]).
/// Schema 4 adds the optional meeting toggle (unbound for existing users).
/// Schema 5 adds an explicit Gemini final-pass choice and its model; existing
/// final-pass choices stay unchanged. Both use additive defaults, with the
/// same versioned backup and immediate persistence as earlier migrations.
/// Schema 6 adds opt-in meeting echo cancellation, off for existing users.
/// Schema 7 retires the Groq preset: each `kind = "groq"` table becomes the
/// `openai-compatible` endpoint it always was, with Groq's URL and models
/// written out. That one is a fix-up of the TOML tree rather than a field
/// mapping, because an unknown kind fails the whole parse (see
/// `groq_migration`).
pub const CONFIG_VERSION: u32 = 7;

/// The first schema without the Groq preset; older files get the fix-up.
const GROQ_RETIRED_IN: u32 = 7;

/// Follow-ups a load's schema migration leaves for the caller. Config cannot
/// reach the keychain, so it reports what it converted instead.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Migrations {
    /// `[provider]` was the retired Groq preset and now reads
    /// `openai-compatible`. Its key is still under the old account; see
    /// [`Migrations::key_copy`].
    pub stt_from_groq: bool,
    /// `[voice.provider]` was Groq, or cleanup was inherited from a Groq STT
    /// provider and is now written out as an explicit table.
    pub cleanup_from_groq: bool,
}

impl Migrations {
    /// The keychain copy this load needs, as `(from, to)` account names. The
    /// account is the kind's label, so changing the STT kind moved its slot.
    /// The caller copies only into an empty slot. A second load of the
    /// migrated file reports nothing here, so a copy that failed is retried
    /// through [`Provider::groq_key_carry`], which asks the settings instead.
    pub fn key_copy(&self) -> Option<(&'static str, &'static str)> {
        self.stt_from_groq.then_some((
            groq_migration::GROQ_KEY_ACCOUNT,
            ProviderKind::OpenaiCompatible.label(),
        ))
    }
}

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// Renamed from `dictionary` in 0.26.0. The alias loads existing config
    /// files unchanged; the next Save rewrites the section under its new name,
    /// so the migration is one-way and needs no separate step.
    #[serde(alias = "dictionary")]
    pub spellbook: Spellbook,
    pub voice: Voice,
    pub history: History,
    pub insights: Insights,
    pub updates: Updates,
    pub startup: Startup,
    pub general: General,
    pub local_stt: LocalStt,
    /// Text shaping applied just before injection (see [`Output`]).
    pub output: Output,
    /// Meeting transcription ("Hark Meetings"), independent of dictation.
    /// Must stay before `invocations`: see the comment on that field.
    pub meeting: Meeting,
    /// Last on purpose: this is the only section holding a TOML
    /// array-of-tables, and those must follow every scalar key.
    pub invocations: Invocations,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            version: CONFIG_VERSION,
            provider: Provider::default(),
            hotkey: Hotkey::default(),
            audio: Audio::default(),
            inject: Inject::default(),
            spellbook: Spellbook::default(),
            voice: Voice::default(),
            history: History::default(),
            insights: Insights::default(),
            updates: Updates::default(),
            startup: Startup::default(),
            general: General::default(),
            local_stt: LocalStt::default(),
            output: Output::default(),
            meeting: Meeting::default(),
            invocations: Invocations::default(),
        }
    }
}

impl Settings {
    /// Validate the optional meeting binding against push-to-talk. Exposed
    /// for immediate settings feedback and hand-built runtime settings.
    pub fn validate_meeting_shortcut(&self) -> Result<(), ConfigError> {
        let Some(meeting) = self.meeting.toggle_key.as_deref() else {
            return Ok(());
        };
        let meeting = hark_hotkey::PttChord::parse(meeting)
            .map_err(|e| ConfigError::Invalid(format!("meeting.toggle_key: {e}")))?;
        let ptt = hark_hotkey::PttChord::parse(&self.hotkey.ptt_key)
            .map_err(|e| ConfigError::Invalid(format!("hotkey.ptt_key: {e}")))?;
        if meeting.conflicts_with(&ptt) {
            return Err(ConfigError::Invalid(
                "meeting.toggle_key conflicts with push-to-talk; neither shortcut may contain all keys of the other".into(),
            ));
        }
        Ok(())
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
        local::validate(&self.local_stt)?;
        self.validate_meeting_shortcut()?;
        if self.meeting.gemini_model.trim().is_empty() {
            return Err(ConfigError::Invalid(
                "meeting.gemini_model must not be empty".into(),
            ));
        }
        Ok(())
    }
}

/// The per-user config *directory*. `None` when the OS gives us no home
/// (headless CI); callers fall back to defaults.
///
/// On Linux this is the XDG config home, which is a different place from
/// [`default_data_dir`] — the freedesktop spec separates configuration a user
/// might sync or hand-edit from state a program owns. Windows and macOS put
/// both under one roof, which is their convention.
pub fn default_config_dir() -> Option<PathBuf> {
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
        // XDG_CONFIG_HOME wins when set and absolute. The spec says a relative
        // value is invalid and must be treated as unset, which matters: joining
        // a relative path would put the config wherever Hark happened to be
        // started from, so it would appear empty on the next launch.
        if let Some(x) = std::env::var_os("XDG_CONFIG_HOME") {
            let x = PathBuf::from(x);
            if x.is_absolute() {
                return Some(x.join("hark"));
            }
        }
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config").join("hark"))
    }
}

/// The per-user config file location. `None` when the OS gives us no home
/// (headless CI); callers fall back to defaults.
pub fn default_config_path() -> Option<PathBuf> {
    default_config_dir().map(|d| d.join("config.toml"))
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
        // Same absolute-path rule as XDG_CONFIG_HOME above: a relative value
        // is invalid per the spec, and honouring one would scatter the history
        // database across every directory Hark was ever launched from.
        if let Some(x) = std::env::var_os("XDG_DATA_HOME") {
            let x = PathBuf::from(x);
            if x.is_absolute() {
                return Some(x.join("hark"));
            }
        }
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share").join("hark"))
    }
}

/// Where downloaded on-device model weights live: `<data_dir>/models/<model>`.
/// `None` when the OS gives us no home (headless CI), matching
/// [`default_data_dir`]. `model` is validated to be a plain path segment, so
/// the result can never escape the models root.
pub fn model_dir(model: &str) -> Option<PathBuf> {
    default_data_dir().map(|d| d.join("models").join(model))
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
        assert!(s.spellbook.terms().is_empty());
        assert!(!s.insights.track_apps);
        assert!(!s.insights.analyze_text);
    }

    #[test]
    fn explicit_values_override_defaults() {
        let s = Settings::from_toml(
            r#"
            [provider]
            kind = "openai"
            model = "whisper-1"

            [hotkey]
            ptt_key = "RCtrl"

            [audio]
            tail_ms = 200

            [inject]
            strategy = "type"

            [spellbook]
            terms = ["Hark", "Levenshtein"]
            "#,
        )
        .expect("valid TOML parses");
        assert_eq!(s.provider.kind, ProviderKind::Openai);
        assert_eq!(
            s.provider.resolved_base_url().as_deref(),
            Some("https://api.openai.com/v1")
        );
        assert_eq!(s.provider.resolved_model(), "whisper-1");
        assert_eq!(s.hotkey.ptt_key, "RCtrl");
        assert_eq!(s.audio.tail_ms, 200);
        // Untouched keys keep their defaults.
        assert_eq!(s.audio.preroll_ms, 300);
        assert_eq!(s.inject.strategy, InjectStrategy::Type);
        assert_eq!(s.spellbook.terms(), vec!["Hark", "Levenshtein"]);
    }
}

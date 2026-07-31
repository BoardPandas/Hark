//! Hark settings: `Settings` struct + TOML loader/saver with defaults.
//! SQLite storage lives in `hark-store`; this crate owns the TOML half.
//!
//! API keys never live in TOML: they come from the OS keychain or the
//! `HARK_STT_KEY` / `HARK_CLEANUP_KEY` env overrides via `hark-keychain`.
//!
//! Saving serializes the full `Settings` model back to disk: unknown keys a
//! user hand-added are dropped. Acceptable while the schema is additive
//! (Phase 4 spec §5); the struct is the source of truth.

mod invocations;
mod local;
mod voice;

pub use invocations::{Invocation, Invocations, Scope};
pub use local::{LocalMode, LocalStt, DEFAULT_FALLBACK_AFTER_MS, DEFAULT_MODEL};
pub use voice::{
    resolve_cleanup_provider, CleanupKeySource, CleanupResolution, ResolvedCleanupProvider, Voice,
    VoiceName, VoiceProvider,
};

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
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
pub const CONFIG_VERSION: u32 = 2;

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Hotkey {
    /// "+"-separated chord; hold ALL keys to record, release any to stop.
    /// Parsed by `hark-hotkey` at startup (unknown names error there).
    pub ptt_key: String,
    /// Stop Caps/Scroll Lock toggling when the lock key is pressed as part of
    /// the chord. Turning this off restores the observe-only hook exactly, and
    /// exists because none of the suppression can be verified without real
    /// Windows hardware — a config line beats waiting for a build.
    pub swallow_lock_keys: bool,
}

impl Default for Hotkey {
    fn default() -> Self {
        Hotkey {
            // Confirmed with the user 2026-07-16: chord, not a single key.
            ptt_key: "LCtrl+LWin".to_string(),
            swallow_lock_keys: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Audio {
    /// Which microphone to capture from, by cpal device name. `None` (or a
    /// name that no longer matches any present device) falls back to the OS
    /// default input device.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_device: Option<String>,
    /// Audio kept from before the chord registers (catches led-in words).
    pub preroll_ms: u32,
    /// Audio kept after release (catches trailing word endings). Adds
    /// directly to release-to-inject latency; 150 ms confirmed 2026-07-16.
    pub tail_ms: u32,
    /// Max hold; on exceed, transcribe what we have (ring sized from this).
    pub max_hold_s: u32,
    /// Holds shorter than this are dropped without a network request.
    pub min_speech_ms: u32,
    /// Clips whose loudest 100 ms window stays below this RMS are dropped
    /// without a network request. Measured as a peak window, not a whole-clip
    /// mean, so the threshold means the same thing for a one-word command as
    /// for a long sentence.
    pub silence_rms: f32,
}

impl Default for Audio {
    fn default() -> Self {
        Audio {
            input_device: None,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

/// One spellbook entry: the canonical spelling, plus any misheard spellings
/// that should be corrected to it outright.
///
/// `aliases` exist because the phonetic corrector has a hard edge — Double
/// Metaphone equality confirmed by Jaro-Winkler — and two common failures fall
/// outside it: proper nouns split into unrelated words, and providers emitting
/// a legitimate English word (where the corrector is deliberately conservative,
/// and should stay that way). An alias is the explicit override for exactly
/// those cases, matched before the phonetic pass.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SpellbookEntry {
    /// The correct spelling. This is what gets injected, and what goes to the
    /// provider as a biasing hint.
    pub term: String,
    /// Misheard spellings corrected to `term` on an exact (normalized) match.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
}

impl SpellbookEntry {
    /// A bare canonical term with no aliases — what every pre-0.29.0 entry was.
    pub fn new(term: impl Into<String>) -> Self {
        SpellbookEntry {
            term: term.into(),
            aliases: Vec::new(),
        }
    }
}

/// The `[spellbook]` section. Named `[dictionary]` before 0.26.0; the alias on
/// the `Settings` field keeps those files loading (see `Settings::spellbook`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Spellbook {
    /// Retired in schema 2, kept deserializable on purpose (BP
    /// `versioned-config-migration-backup`: old files must still parse for the
    /// migration to have something to read). `migrate` folds these into
    /// `entries` and empties this, after which it never serializes again.
    #[serde(
        rename = "terms",
        alias = "bias_terms",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub legacy_terms: Vec<String>,
    /// The real store since schema 2.
    #[serde(rename = "entries", skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<SpellbookEntry>,
}

impl Spellbook {
    /// Canonical spellings, in entry order: the phonetic corrector's targets
    /// and the source for provider biasing. Aliases are deliberately absent —
    /// they are misspellings, and hinting a provider toward them would teach it
    /// to produce the very text we correct away.
    pub fn terms(&self) -> Vec<String> {
        self.entries.iter().map(|e| e.term.clone()).collect()
    }

    /// `(term, aliases)` pairs in the shape `hark_spellbook::Corrector::new`
    /// takes. Lives here so the mapping exists once rather than at every call
    /// site, and so hark-spellbook stays free of a config dependency.
    pub fn corrector_entries(&self) -> Vec<(String, Vec<String>)> {
        self.entries
            .iter()
            .map(|e| (e.term.clone(), e.aliases.clone()))
            .collect()
    }

    /// Fold retired `terms` into `entries`. Idempotent, and it never clobbers:
    /// a hand-written file carrying both shapes keeps its entries and gains
    /// only the legacy terms that are not already present.
    fn migrate(&mut self) {
        for term in std::mem::take(&mut self.legacy_terms) {
            if !self.entries.iter().any(|e| e.term == term) {
                self.entries.push(SpellbookEntry::new(term));
            }
        }
    }
}

/// The `[history]` section: local capture + retention. Content capture and
/// numeric stats are deliberately independent: with `capture = false` no
/// dictation text is persisted, but the counters in the stats row still
/// tick (they carry no content).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

/// In-app update behavior. The check hits the GitHub Releases API (network),
/// so it is user-controllable; the default is on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Updates {
    /// Check GitHub for a newer release once at startup and surface a banner.
    pub check_on_startup: bool,
}

impl Default for Updates {
    fn default() -> Self {
        Updates {
            check_on_startup: true,
        }
    }
}

/// Launch behavior. `launch_at_login` is the source of truth for the OS
/// startup entry: `hark-autostart` reconciles the registry to it at startup
/// and after every Save. Default on, so a fresh install (and an upgrading
/// user whose config predates this section) starts with Windows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Startup {
    pub launch_at_login: bool,
}

impl Default for Startup {
    fn default() -> Self {
        Startup {
            launch_at_login: true,
        }
    }
}

/// Post-transcription text shaping applied just before injection, after any
/// cleanup pass. Independent of the STT provider and of cleanup, so it behaves
/// the same whether or not a voice ran.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Output {
    /// Drop the trailing period on a single-word dictation ("Hello." ->
    /// "Hello"). A lone word is rarely a sentence, so the period the STT
    /// provider or a cleanup voice appends is usually noise the user did not
    /// ask for. Default on; the setting exists to turn it back off.
    pub strip_single_word_period: bool,
}

impl Default for Output {
    fn default() -> Self {
        Output {
            strip_single_word_period: true,
        }
    }
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
    pub updates: Updates,
    pub startup: Startup,
    pub local_stt: LocalStt,
    /// Text shaping applied just before injection (see [`Output`]).
    pub output: Output,
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
            updates: Updates::default(),
            startup: Startup::default(),
            local_stt: LocalStt::default(),
            output: Output::default(),
            invocations: Invocations::default(),
        }
    }
}

impl Settings {
    /// Parse settings from TOML text. Unknown keys are tolerated (forward
    /// compatibility); missing keys take defaults; the result is validated.
    pub fn from_toml(text: &str) -> Result<Settings, ConfigError> {
        let mut settings: Settings = toml::from_str(text)?;
        // Before validation: the rest of the app must never see a half-migrated
        // model, and `version` is stamped to current so a later save cannot
        // re-trigger the migration.
        settings.spellbook.migrate();
        // Only ever bump *up*. A stamp from the future belongs to a file some
        // newer Hark wrote; stamping it down here would make this build claim
        // authorship of a schema it does not understand, and the next save
        // would quietly downgrade the user's file.
        if settings.version < CONFIG_VERSION {
            settings.version = CONFIG_VERSION;
        }
        settings.validate()?;
        Ok(settings)
    }

    /// Load settings from a file, migrating an older schema in place.
    ///
    /// A missing file is not an error: it yields the defaults (first-run
    /// onboarding is Phase 4).
    ///
    /// When the file predates [`CONFIG_VERSION`], the on-disk copy is backed
    /// up and rewritten in the new shape before this returns, so the migration
    /// happens once rather than on every launch. Both writes are best-effort:
    /// the returned settings are correct either way, and a config directory
    /// that cannot be written is not a reason to refuse to start.
    ///
    /// **If the backup cannot be written, the file is left alone.** The backup
    /// is what makes a bad mapping recoverable, so migrating without one would
    /// trade the user's only copy for a convenience.
    pub fn load(path: &Path) -> Result<Settings, ConfigError> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let from_version = Settings::version_of(&text);
                let settings = Settings::from_toml(&text)?;
                if from_version < CONFIG_VERSION {
                    settings.persist_migration(path, &text, from_version);
                }
                Ok(settings)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(e) => Err(ConfigError::Io {
                path: path.display().to_string(),
                source: e,
            }),
        }
    }

    /// The `version` stamp as written, before defaulting. Read from the raw
    /// text rather than the parsed struct because a file predating the stamp
    /// deserializes to the *current* value, which would hide the very
    /// migration this needs to detect.
    fn version_of(text: &str) -> u32 {
        text.parse::<toml::Table>()
            .ok()
            .and_then(|t| t.get("version").and_then(toml::Value::as_integer))
            .and_then(|v| u32::try_from(v).ok())
            .unwrap_or(0)
    }

    /// Back up the pre-migration text, then rewrite the file in the new shape.
    /// Best-effort by design; see [`Settings::load`].
    fn persist_migration(&self, path: &Path, original: &str, from_version: u32) {
        // Version-stamped so repeated migrations never overwrite each other's
        // backups, unlike a single `.bak`.
        let backup = path.with_extension(format!("toml.v{from_version}.bak"));
        if let Err(e) = std::fs::write(&backup, original) {
            log::warn!(
                "config schema v{from_version} -> v{CONFIG_VERSION}: backup failed ({e}); \
                 leaving the file as-is and migrating in memory only"
            );
            return;
        }
        match self.save(path) {
            Ok(()) => log::info!(
                "config migrated from schema v{from_version} to v{CONFIG_VERSION}; \
                 previous file kept alongside it"
            ),
            Err(e) => log::warn!("config schema migration could not be saved ({e})"),
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
        local::validate(&self.local_stt)?;
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

            [spellbook]
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
        assert_eq!(s.spellbook.terms(), vec!["Hark", "Levenshtein"]);
    }

    #[test]
    fn legacy_bias_terms_key_still_parses_via_alias() {
        // Pre-Phase-2 config files used `bias_terms`; the serde alias must
        // keep them loading forever.
        let s = Settings::from_toml("[spellbook]\nbias_terms = [\"Modero\"]")
            .expect("legacy key parses");
        assert_eq!(s.spellbook.terms(), vec!["Modero"]);
    }

    #[test]
    fn legacy_dictionary_section_still_parses_via_alias() {
        // The section was `[dictionary]` before 0.26.0. Every existing install
        // has one, and losing it would silently empty the user's terms -- the
        // one outcome a rename must never produce.
        let s = Settings::from_toml("[dictionary]\nterms = [\"Eldrazi\"]")
            .expect("legacy section parses");
        assert_eq!(s.spellbook.terms(), vec!["Eldrazi"]);
    }

    // --- schema v1 -> v2: flat terms become entries with aliases ---

    #[test]
    fn v1_flat_terms_become_entries_in_order() {
        let s = Settings::from_toml("[spellbook]\nterms = [\"Hark\", \"Eldrazi\"]")
            .expect("v1 shape parses");
        assert_eq!(s.spellbook.terms(), vec!["Hark", "Eldrazi"]);
        assert!(
            s.spellbook.entries.iter().all(|e| e.aliases.is_empty()),
            "a migrated term has no aliases to invent"
        );
        assert!(
            s.spellbook.legacy_terms.is_empty(),
            "the retired field must be drained, or it migrates again every load"
        );
    }

    #[test]
    fn v2_entries_with_aliases_round_trip() {
        let text = "[[spellbook.entries]]\nterm = \"Eldrazi\"\naliases = [\"Al Drazi\"]";
        let s = Settings::from_toml(text).expect("v2 shape parses");
        assert_eq!(s.spellbook.entries[0].term, "Eldrazi");
        assert_eq!(s.spellbook.entries[0].aliases, vec!["Al Drazi"]);

        let out = s.to_toml().expect("serializes");
        let again = Settings::from_toml(&out).expect("re-parses");
        assert_eq!(again.spellbook, s.spellbook);
    }

    #[test]
    fn migration_is_idempotent_and_never_clobbers_existing_entries() {
        // A hand-edited file carrying both shapes: the entry wins, and only
        // genuinely new legacy terms are appended.
        let text = "[spellbook]\nterms = [\"Eldrazi\", \"Hark\"]\n\
                    [[spellbook.entries]]\nterm = \"Eldrazi\"\naliases = [\"Al Drazi\"]";
        let s = Settings::from_toml(text).expect("mixed shape parses");
        assert_eq!(s.spellbook.terms(), vec!["Eldrazi", "Hark"]);
        assert_eq!(
            s.spellbook.entries[0].aliases,
            vec!["Al Drazi"],
            "the existing entry's aliases must survive the fold"
        );

        // Loading the saved result again changes nothing.
        let out = s.to_toml().expect("serializes");
        assert_eq!(Settings::from_toml(&out).unwrap().spellbook, s.spellbook);
    }

    #[test]
    fn a_saved_file_no_longer_carries_the_retired_terms_key() {
        let s = Settings::from_toml("[spellbook]\nterms = [\"Eldrazi\"]").unwrap();
        let out = s.to_toml().expect("serializes");
        assert!(out.contains("[[spellbook.entries]]"), "{out}");
        assert!(
            !out.contains("terms = ["),
            "the retired key must not be written back: {out}"
        );
        assert!(
            out.contains(&format!("version = {CONFIG_VERSION}")),
            "{out}"
        );
    }

    #[test]
    fn loading_a_v1_file_backs_it_up_and_rewrites_it_once() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        let original = "version = 1\n[spellbook]\nterms = [\"Eldrazi\"]\n";
        std::fs::write(&path, original).expect("seed v1 config");

        let s = Settings::load(&path).expect("loads");
        assert_eq!(s.spellbook.terms(), vec!["Eldrazi"]);

        // The backup is version-stamped and holds the file exactly as it was:
        // that is what makes a bad mapping recoverable.
        let backup = dir.path().join("config.toml.v1.bak");
        assert_eq!(
            std::fs::read_to_string(&backup).expect("backup written"),
            original
        );

        // The file itself is rewritten in the new shape, so the migration runs
        // once rather than on every launch.
        let migrated = std::fs::read_to_string(&path).expect("config still readable");
        assert!(migrated.contains("[[spellbook.entries]]"), "{migrated}");
        assert!(
            migrated.contains(&format!("version = {CONFIG_VERSION}")),
            "{migrated}"
        );

        // A second load is a no-op: no v2 backup appears.
        Settings::load(&path).expect("loads again");
        assert!(
            !dir.path().join("config.toml.v2.bak").exists(),
            "an already-current file must not be migrated again"
        );
    }

    #[test]
    fn a_current_file_is_never_touched_on_load() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        let current = Settings::default();
        current.save(&path).expect("save");
        let before = std::fs::read_to_string(&path).expect("read");

        Settings::load(&path).expect("loads");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        assert!(!dir.path().join("config.toml.v1.bak").exists());
        assert!(!dir.path().join("config.toml.v2.bak").exists());
    }

    #[test]
    fn an_unstamped_file_is_treated_as_pre_v1_and_migrated() {
        // Files predating the version stamp deserialize to the current value,
        // so the stamp has to be read from the raw text or the migration is
        // invisible.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[spellbook]\nterms = [\"Eldrazi\"]\n").expect("seed");

        Settings::load(&path).expect("loads");
        assert!(
            dir.path().join("config.toml.v0.bak").exists(),
            "an unstamped file must still be backed up before rewriting"
        );
    }

    #[test]
    fn legacy_dictionary_section_is_rewritten_under_the_new_name() {
        // The migration is one-way and implicit: load old, save new. Pinning it
        // here means a future serde change cannot quietly keep writing the old
        // section forever.
        let s = Settings::from_toml("[dictionary]\nbias_terms = [\"Eldrazi\"]")
            .expect("legacy section parses");
        let out = toml::to_string_pretty(&s).expect("serializes");
        assert!(
            out.contains("[[spellbook.entries]]"),
            "must save under the new name: {out}"
        );
        assert!(
            !out.contains("[dictionary]"),
            "must not re-emit the old name"
        );
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

        // An older stamp is migrated up: after `from_toml` the in-memory model
        // really is current-generation.
        let s = Settings::from_toml("version = 1").expect("explicit stamp parses");
        assert_eq!(s.version, CONFIG_VERSION);

        // A newer stamp is left alone. Two Hark versions sharing one config is
        // the case that matters: the older build must not relabel the file.
        let future = CONFIG_VERSION + 5;
        let s = Settings::from_toml(&format!("version = {future}")).expect("future stamp parses");
        assert_eq!(s.version, future, "a future schema stamp must survive");
    }

    #[test]
    fn input_device_defaults_to_none_and_round_trips_when_set() {
        // Absent: the OS default microphone (serialized as nothing).
        let s = Settings::from_toml("").expect("empty TOML parses");
        assert_eq!(s.audio.input_device, None);
        assert!(
            !Settings::default()
                .to_toml()
                .unwrap()
                .contains("input_device"),
            "the default (OS default mic) must not serialize a key"
        );

        // Present: the chosen device name parses and survives a save/load.
        let s = Settings::from_toml("[audio]\ninput_device = \"Yeti Stereo Microphone\"")
            .expect("named device parses");
        assert_eq!(
            s.audio.input_device.as_deref(),
            Some("Yeti Stereo Microphone")
        );
        let text = s.to_toml().expect("serializes");
        assert_eq!(
            Settings::from_toml(&text)
                .unwrap()
                .audio
                .input_device
                .as_deref(),
            Some("Yeti Stereo Microphone")
        );
    }

    #[test]
    fn updates_default_to_startup_check_on_and_round_trip() {
        let s = Settings::from_toml("").expect("empty TOML parses");
        assert!(s.updates.check_on_startup);

        let s = Settings::from_toml("[updates]\ncheck_on_startup = false")
            .expect("updates section parses");
        assert!(!s.updates.check_on_startup);
        let text = s.to_toml().expect("serializes");
        assert!(!Settings::from_toml(&text).unwrap().updates.check_on_startup);
    }

    #[test]
    fn startup_defaults_to_launch_at_login_on_and_round_trips() {
        // Default on: fresh installs and pre-[startup] config files both
        // launch at login until the user opts out.
        let s = Settings::from_toml("").expect("empty TOML parses");
        assert!(s.startup.launch_at_login);

        let s = Settings::from_toml("[startup]\nlaunch_at_login = false")
            .expect("startup section parses");
        assert!(!s.startup.launch_at_login);
        let text = s.to_toml().expect("serializes");
        assert!(!Settings::from_toml(&text).unwrap().startup.launch_at_login);
    }

    #[test]
    fn output_defaults_to_stripping_single_word_periods_and_round_trips() {
        // Default on: a fresh install and a config file predating [output]
        // both strip the lone-word period until the user opts out.
        let s = Settings::from_toml("").expect("empty TOML parses");
        assert!(s.output.strip_single_word_period);

        let s = Settings::from_toml("[output]\nstrip_single_word_period = false")
            .expect("output section parses");
        assert!(!s.output.strip_single_word_period);
        let text = s.to_toml().expect("serializes");
        assert!(
            !Settings::from_toml(&text)
                .unwrap()
                .output
                .strip_single_word_period
        );
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
            text.contains(&format!("version = {CONFIG_VERSION}")),
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
        s.spellbook.entries = vec![SpellbookEntry::new("Hark"), SpellbookEntry::new("Modero")];
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
        assert_eq!(loaded.spellbook.terms(), vec!["Hark", "Modero"]);
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
    fn legacy_bias_terms_migrate_into_entries() {
        // The oldest shape of all (pre-Phase-2 `bias_terms`) still has to reach
        // the current model, through both renames in one hop.
        let s = Settings::from_toml("[spellbook]\nbias_terms = [\"Modero\"]")
            .expect("legacy key parses");
        assert_eq!(s.spellbook.terms(), vec!["Modero"]);
        let text = s.to_toml().expect("serializes");
        assert!(text.contains("[[spellbook.entries]]"), "{text}");
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

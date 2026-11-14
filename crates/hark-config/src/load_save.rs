use crate::{
    groq_migration, meeting, ConfigError, Migrations, Settings, CONFIG_VERSION, GROQ_RETIRED_IN,
};
use std::path::Path;

impl Settings {
    /// Parse settings from TOML text. Unknown keys are tolerated (forward
    /// compatibility); missing keys take defaults; the result is validated.
    pub fn from_toml(text: &str) -> Result<Settings, ConfigError> {
        Settings::parse(text).map(|(settings, _)| settings)
    }

    /// [`Settings::from_toml`], also reporting what the migration converted.
    pub(crate) fn parse(text: &str) -> Result<(Settings, Migrations), ConfigError> {
        let mut migrations = Migrations::default();
        // The Groq fix-up has to happen on the TOML tree: serde rejects the
        // retired kind outright. Text that is not TOML at all falls through,
        // so the real parse below is what reports it.
        let fixed = match text.parse::<toml::Table>() {
            Ok(mut root) if stamp(&root) < GROQ_RETIRED_IN => {
                let converted = groq_migration::migrate(&mut root);
                migrations.stt_from_groq = converted.stt;
                migrations.cleanup_from_groq = converted.cleanup;
                (converted.stt || converted.cleanup).then_some(root)
            }
            _ => None,
        };
        let mut settings: Settings = match fixed {
            Some(root) => root.try_into()?,
            None => toml::from_str(text)?,
        };
        // Before validation: the rest of the app must never see a half-migrated
        // model, and `version` is stamped to current so a later save cannot
        // re-trigger the migration.
        settings.spellbook.migrate();
        // Also before validation: an absurd cap is a typo, not a reason to
        // refuse to start, so it is clamped in place rather than rejected.
        meeting::clamp(&mut settings.meeting);
        // Reads the file's own stamp, so it must run before the bump below.
        meeting::migrate(&mut settings.meeting, settings.version);
        // Only ever bump *up*. A stamp from the future belongs to a file some
        // newer Hark wrote; stamping it down here would make this build claim
        // authorship of a schema it does not understand, and the next save
        // would quietly downgrade the user's file.
        if settings.version < CONFIG_VERSION {
            settings.version = CONFIG_VERSION;
        }
        settings.validate()?;
        Ok((settings, migrations))
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
        Settings::load_with_migrations(path).map(|(settings, _)| settings)
    }

    /// [`Settings::load`], also reporting the follow-ups the migration left
    /// for the caller (the keychain copy config cannot do itself).
    ///
    /// The report does not depend on whether the rewrite could be saved: the
    /// returned settings are migrated either way, so this session needs the
    /// follow-up either way.
    pub fn load_with_migrations(path: &Path) -> Result<(Settings, Migrations), ConfigError> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let from_version = Settings::version_of(&text);
                let (settings, migrations) = Settings::parse(&text)?;
                if from_version < CONFIG_VERSION {
                    settings.persist_migration(path, &text, from_version);
                }
                Ok((settings, migrations))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Ok((Settings::default(), Migrations::default()))
            }
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
        text.parse::<toml::Table>().map(|t| stamp(&t)).unwrap_or(0)
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
}

/// The `version` stamp of a parsed file; 0 when absent (pre-stamp files).
fn stamp(root: &toml::Table) -> u32 {
    root.get("version")
        .and_then(toml::Value::as_integer)
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{InjectStrategy, ProviderKind, SpellbookEntry, VoiceName, VoiceProvider};

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
        assert_eq!(s.provider.resolved_model(), "gpt-transcribe");
    }

    #[test]
    fn malformed_toml_is_a_parse_error() {
        let err = Settings::from_toml("[provider\nkind=").expect_err("bad TOML must fail");
        assert!(matches!(err, ConfigError::Parse(_)));
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
        s.provider.kind = ProviderKind::Openai;
        s.provider.model = Some("whisper-1".to_string());
        s.provider.live = true;
        s.hotkey.ptt_key = "RCtrl".to_string();
        s.spellbook.entries = vec![SpellbookEntry::new("Hark"), SpellbookEntry::new("Modero")];
        s.history.capture = false;
        s.history.max_entries = 250;
        s.general.always_on_top = true;
        s.general.exit_on_close = true;
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
        assert_eq!(loaded.provider.kind, ProviderKind::Openai);
        assert_eq!(loaded.provider.model.as_deref(), Some("whisper-1"));
        assert!(loaded.provider.live);
        assert_eq!(loaded.provider.base_url, None);
        assert_eq!(loaded.hotkey.ptt_key, "RCtrl");
        assert_eq!(loaded.spellbook.terms(), vec!["Hark", "Modero"]);
        assert!(!loaded.history.capture);
        assert_eq!(loaded.history.max_entries, 250);
        assert!(loaded.general.always_on_top);
        assert!(loaded.general.exit_on_close);
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
}

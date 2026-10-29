//! Schema 7: the retired Groq preset.
//!
//! `ProviderKind` no longer has a `groq` variant, and it has no
//! `#[serde(other)]`, so an old `kind = "groq"` would fail the *whole* parse.
//! The app answers a failed parse with defaults behind a banner, and the next
//! Save writes those defaults over the user's file. So this runs on the TOML
//! tree, before serde sees it, and rewrites each Groq table as the
//! `openai-compatible` endpoint it always was, with every per-kind default the
//! preset used to supply written out explicitly (BP
//! `versioned-config-migration-backup`: map fields explicitly, prefer the
//! user's working behaviour over the new default).

use crate::{Provider, ProviderKind};
use toml::{Table, Value};

/// The keychain account the retired preset stored its key under: its label.
pub(crate) const GROQ_KEY_ACCOUNT: &str = "groq";
const GROQ_KIND: &str = "groq";
const GROQ_BASE_URL: &str = "https://api.groq.com/openai/v1";
const GROQ_STT_MODEL: &str = "whisper-large-v3-turbo";
const GROQ_CHAT_MODEL: &str = "llama-3.1-8b-instant";
/// The preset's cleanup temperature. `openai-compatible` has no preset, so
/// without this the endpoint would silently fall back to its server default.
const GROQ_CHAT_TEMPERATURE: f64 = 0.2;
const MIGRATED_KIND: &str = "openai-compatible";

impl Provider {
    /// The keychain copy a former Groq install may still be owed, as
    /// `(from, to)` account names: an STT provider on `openai-compatible`
    /// pointed at Groq's own endpoint, which is what the schema 7 migration
    /// writes.
    ///
    /// A condition on the settings, not a flag from the migrating load: the
    /// file is already v7 by the time the copy runs, so a keychain that fails
    /// at that one moment would otherwise strand the key under `groq` for
    /// good. Asking again on a later launch is safe, because the copy only
    /// fills an empty slot from a stored key.
    pub fn groq_key_carry(&self) -> Option<(&'static str, &'static str)> {
        let base = self.base_url.as_deref()?.trim().trim_end_matches('/');
        (self.kind == ProviderKind::OpenaiCompatible && base.eq_ignore_ascii_case(GROQ_BASE_URL))
            .then_some((GROQ_KEY_ACCOUNT, ProviderKind::OpenaiCompatible.label()))
    }
}

/// Which tables the fix-up converted.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Converted {
    pub stt: bool,
    pub cleanup: bool,
}

/// Rewrite every Groq table in `root`. Idempotent: a second pass finds no
/// `kind = "groq"` and changes nothing.
pub(crate) fn migrate(root: &mut Table) -> Converted {
    let stt_base_url = root
        .get_mut("provider")
        .and_then(Value::as_table_mut)
        .and_then(|p| convert(p, GROQ_STT_MODEL));
    let cleanup = migrate_cleanup(root, stt_base_url.as_deref());
    Converted {
        stt: stt_base_url.is_some(),
        cleanup,
    }
}

/// Convert one provider table if it names Groq, keeping any `base_url` or
/// `model` the user set. Returns the table's base URL when it converted.
fn convert(table: &mut Table, default_model: &str) -> Option<String> {
    if table.get("kind").and_then(Value::as_str) != Some(GROQ_KIND) {
        return None;
    }
    table.insert("kind".into(), Value::String(MIGRATED_KIND.into()));
    let base_url = table
        .entry("base_url")
        .or_insert_with(|| Value::String(GROQ_BASE_URL.into()))
        .as_str()
        // A non-string base_url fails the real parse anyway.
        .unwrap_or(GROQ_BASE_URL)
        .to_string();
    table
        .entry("model")
        .or_insert_with(|| Value::String(default_model.into()));
    Some(base_url)
}

/// `[voice.provider]`, in the two shapes a Groq install can have.
///
/// An explicit Groq table converts like the STT one. An absent table is the
/// subtle case: cleanup was *inherited* from a Groq STT provider, and
/// `openai-compatible` is deliberately not inherited (speaking
/// `/audio/transcriptions` does not imply `/chat/completions`). Converting only
/// the STT table would therefore turn every such user's cleanup off — and
/// Clean is the default voice. So the inherited provider is written out, with
/// the same endpoint the inherit path used.
fn migrate_cleanup(root: &mut Table, stt_base_url: Option<&str>) -> bool {
    let has_table = root
        .get("voice")
        .and_then(Value::as_table)
        .is_some_and(|v| v.contains_key("provider"));
    if has_table {
        let Some(p) = root
            .get_mut("voice")
            .and_then(Value::as_table_mut)
            .and_then(|v| v.get_mut("provider"))
            .and_then(Value::as_table_mut)
        else {
            return false;
        };
        if convert(p, GROQ_CHAT_MODEL).is_none() {
            return false;
        }
        p.entry("temperature")
            .or_insert(Value::Float(GROQ_CHAT_TEMPERATURE));
        // With a non-Groq STT provider this table had its own key slot,
        // `groq`. Pointing it there keeps that exact key; otherwise it would
        // read `openai-compatible`, which belongs to another endpoint or none.
        if stt_base_url.is_none() {
            p.entry("key_account")
                .or_insert_with(|| Value::String(GROQ_KEY_ACCOUNT.into()));
        }
        return true;
    }
    let Some(base_url) = stt_base_url else {
        return false;
    };
    let Some(voice) = root
        .entry("voice")
        .or_insert_with(|| Value::Table(Table::new()))
        .as_table_mut()
    else {
        // A non-table `voice` fails the real parse anyway.
        return false;
    };
    let mut provider = Table::new();
    provider.insert("kind".into(), Value::String(MIGRATED_KIND.into()));
    provider.insert("base_url".into(), Value::String(base_url.into()));
    provider.insert("model".into(), Value::String(GROQ_CHAT_MODEL.into()));
    provider.insert("temperature".into(), Value::Float(GROQ_CHAT_TEMPERATURE));
    voice.insert("provider".into(), Value::Table(provider));
    true
}

#[cfg(test)]
mod tests {
    use crate::{
        CleanupKeySource, CleanupResolution, Provider, ProviderKind, Settings, CONFIG_VERSION,
    };

    fn resolve(s: &Settings) -> CleanupResolution {
        crate::resolve_cleanup_provider(&s.provider, &s.voice, s.voice.default)
    }

    fn resolved(s: &Settings) -> crate::ResolvedCleanupProvider {
        match resolve(s) {
            CleanupResolution::Resolved(r) => r,
            other => panic!("expected a resolved cleanup provider, got {other:?}"),
        }
    }

    #[test]
    fn a_groq_stt_provider_becomes_openai_compatible_with_its_endpoint_written_out() {
        let (s, m) = Settings::parse("version = 6\n[provider]\nkind = \"groq\"\n").unwrap();
        assert_eq!(s.provider.kind, ProviderKind::OpenaiCompatible);
        assert_eq!(
            s.provider.base_url.as_deref(),
            Some("https://api.groq.com/openai/v1")
        );
        assert_eq!(s.provider.model.as_deref(), Some("whisper-large-v3-turbo"));
        assert!(m.stt_from_groq);
        assert_eq!(
            m.key_copy(),
            Some(("groq", "openai-compatible")),
            "the key must follow the provider into its new slot"
        );
        // Whisper-family: still the single-`prompt` contract, as before.
        assert!(!s.provider.uses_keywords_biasing());
    }

    #[test]
    fn the_key_carry_is_owed_on_every_load_of_a_groq_endpoint_not_just_the_first() {
        // The migrating load and every later one ask the same question, so a
        // keychain that failed the first time is asked again next launch.
        let (migrated, _) = Settings::parse("version = 6\n[provider]\nkind = \"groq\"\n").unwrap();
        let carry = Some(("groq", "openai-compatible"));
        assert_eq!(migrated.provider.groq_key_carry(), carry);
        let saved = migrated.to_toml().unwrap();
        let (later, m) = Settings::parse(&saved).unwrap();
        assert_eq!(m, crate::Migrations::default(), "nothing left to migrate");
        assert_eq!(later.provider.groq_key_carry(), carry);
        // Hand-typed variants of the same endpoint.
        for url in [
            "https://api.groq.com/openai/v1/",
            " HTTPS://API.GROQ.COM/openai/v1 ",
        ] {
            let p = Provider {
                kind: ProviderKind::OpenaiCompatible,
                base_url: Some(url.into()),
                ..Provider::default()
            };
            assert_eq!(p.groq_key_carry(), carry, "{url}");
        }
    }

    #[test]
    fn no_key_carry_is_owed_anywhere_else() {
        for provider in [
            Provider::default(),
            Provider {
                kind: ProviderKind::OpenaiCompatible,
                base_url: Some("http://localhost:8000/v1".into()),
                ..Provider::default()
            },
            Provider {
                kind: ProviderKind::OpenaiCompatible,
                base_url: None,
                ..Provider::default()
            },
            // Groq's URL under another kind reads another slot entirely.
            Provider {
                kind: ProviderKind::Openai,
                base_url: Some("https://api.groq.com/openai/v1".into()),
                ..Provider::default()
            },
        ] {
            assert_eq!(provider.groq_key_carry(), None, "{provider:?}");
        }
    }

    #[test]
    fn a_pinned_groq_model_and_url_survive_the_conversion() {
        let (s, _) = Settings::parse(
            "version = 6\n[provider]\nkind = \"groq\"\nmodel = \"whisper-large-v3\"\n\
             base_url = \"https://groq-proxy.example/v1\"\n",
        )
        .unwrap();
        assert_eq!(s.provider.model.as_deref(), Some("whisper-large-v3"));
        assert_eq!(
            s.provider.base_url.as_deref(),
            Some("https://groq-proxy.example/v1")
        );
    }

    #[test]
    fn inherited_groq_cleanup_is_written_out_so_it_keeps_running() {
        // Clean is the default voice, so a stock Groq install ran cleanup on
        // Groq's chat model through the inherit path. openai-compatible is not
        // inherited, which would quietly turn that cleanup off.
        let (s, m) = Settings::parse("version = 6\n[provider]\nkind = \"groq\"\n").unwrap();
        assert!(m.cleanup_from_groq);
        let r = resolved(&s);
        assert_eq!(r.kind, ProviderKind::OpenaiCompatible);
        assert_eq!(r.base_url, "https://api.groq.com/openai/v1");
        assert_eq!(r.model, "llama-3.1-8b-instant");
        assert_eq!(r.temperature, Some(0.2));
        assert_eq!(r.reasoning_effort, None);
        // Same slot as the STT provider: one key, as before.
        assert_eq!(
            r.key_source,
            CleanupKeySource::Account("openai-compatible".into())
        );
    }

    #[test]
    fn inherited_cleanup_follows_a_custom_stt_base_url() {
        let (s, _) = Settings::parse(
            "version = 6\n[provider]\nkind = \"groq\"\nbase_url = \"https://groq-proxy.example/v1\"\n",
        )
        .unwrap();
        assert_eq!(resolved(&s).base_url, "https://groq-proxy.example/v1");
    }

    #[test]
    fn an_explicit_groq_cleanup_table_converts_and_keeps_its_own_key_slot() {
        // Deepgram STT plus Groq cleanup: the cleanup key lived under `groq`,
        // apart from the STT key, and must still be read from there.
        let (s, m) = Settings::parse(
            "version = 6\n[provider]\nkind = \"deepgram\"\n[voice.provider]\nkind = \"groq\"\n",
        )
        .unwrap();
        assert!(!m.stt_from_groq);
        assert!(m.cleanup_from_groq);
        assert_eq!(m.key_copy(), None, "the STT key did not move");
        let r = resolved(&s);
        assert_eq!(r.kind, ProviderKind::OpenaiCompatible);
        assert_eq!(r.base_url, "https://api.groq.com/openai/v1");
        assert_eq!(r.model, "llama-3.1-8b-instant");
        assert_eq!(r.temperature, Some(0.2));
        assert_eq!(r.key_source, CleanupKeySource::Account("groq".into()));
    }

    #[test]
    fn groq_for_both_roles_shares_one_slot_after_the_conversion() {
        let (s, _) = Settings::parse(
            "version = 6\n[provider]\nkind = \"groq\"\n\
             [voice.provider]\nkind = \"groq\"\nmodel = \"openai/gpt-oss-20b\"\ntemperature = 0.5\n",
        )
        .unwrap();
        let r = resolved(&s);
        assert_eq!(r.model, "openai/gpt-oss-20b", "a pinned chat model is kept");
        assert_eq!(r.temperature, Some(0.5), "a pinned temperature is kept");
        assert_eq!(
            r.key_source,
            CleanupKeySource::Account("openai-compatible".into())
        );
    }

    #[test]
    fn an_explicit_key_account_is_never_replaced() {
        let (s, _) = Settings::parse(
            "version = 6\n[voice.provider]\nkind = \"groq\"\nkey_account = \"my-groq\"\n",
        )
        .unwrap();
        assert_eq!(
            resolved(&s).key_source,
            CleanupKeySource::Account("my-groq".into())
        );
    }

    #[test]
    fn non_groq_files_report_nothing_to_do() {
        let (s, m) =
            Settings::parse("version = 6\n[provider]\nkind = \"openai\"\n[voice]\nx = 1\n")
                .unwrap();
        assert_eq!(s.provider.kind, ProviderKind::Openai);
        assert_eq!(m, crate::Migrations::default());
        assert!(s.voice.provider.is_none(), "nothing is invented for OpenAI");
    }

    #[test]
    fn an_unstamped_groq_file_is_migrated_too() {
        // Pre-stamp files are v0 for migration purposes.
        let (s, m) = Settings::parse("[provider]\nkind = \"groq\"\n").unwrap();
        assert_eq!(s.provider.kind, ProviderKind::OpenaiCompatible);
        assert!(m.stt_from_groq);
    }

    #[test]
    fn inline_and_dotted_provider_tables_are_found() {
        // TOML-aware on purpose: a regex over `[provider]` would miss these.
        for text in [
            "version = 6\nprovider = { kind = \"groq\" }\n",
            "version = 6\nprovider.kind = \"groq\"\n",
        ] {
            let (s, m) = Settings::parse(text).unwrap();
            assert_eq!(s.provider.kind, ProviderKind::OpenaiCompatible, "{text}");
            assert!(m.stt_from_groq, "{text}");
        }
    }

    #[test]
    fn a_current_schema_file_is_not_rewritten() {
        // A v7 file never holds `groq` from this build; a hand-edited one is
        // an unknown kind like any typo, not something to migrate silently
        // on every launch without a backup.
        let err = Settings::parse(&format!(
            "version = {CONFIG_VERSION}\n[provider]\nkind = \"groq\"\n"
        ))
        .expect_err("an unknown kind in a current file must fail the parse");
        assert!(matches!(err, crate::ConfigError::Parse(_)));
    }

    #[test]
    fn the_rest_of_a_real_v6_file_survives_the_fix_up() {
        let text =
            "version = 6\n\n[provider]\nkind = \"groq\"\n\n[hotkey]\nptt_key = \"RCtrl\"\n\n\
                    [voice]\ndefault = \"professional\"\n\n\
                    [[spellbook.entries]]\nterm = \"Eldrazi\"\naliases = [\"Al Drazi\"]\n\n\
                    [[invocations.entries]]\nphrase = \"access granted now\"\nexpansion = \"ok\"\n";
        let (s, _) = Settings::parse(text).unwrap();
        assert_eq!(s.hotkey.ptt_key, "RCtrl");
        assert_eq!(s.voice.default, crate::VoiceName::Professional);
        assert_eq!(s.spellbook.entries[0].aliases, vec!["Al Drazi"]);
        assert_eq!(s.invocations.entries.len(), 1);
        assert_eq!(s.version, CONFIG_VERSION);
        // What it saves no longer names Groq as a kind, and loads cleanly.
        let saved = s.to_toml().unwrap();
        assert!(!saved.contains("kind = \"groq\""), "{saved}");
        assert_eq!(Settings::from_toml(&saved).unwrap(), s);
    }

    #[test]
    fn loading_a_v6_groq_file_backs_it_up_byte_exact_and_migrates_once() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        // Comments and odd spacing: the backup must keep every byte of them.
        let original = "# my config\r\nversion = 6\r\n[provider]\r\nkind   = \"groq\"  # fast\r\n";
        std::fs::write(&path, original).unwrap();

        let (s, m) = Settings::load_with_migrations(&path).expect("loads");
        assert_eq!(s.provider.kind, ProviderKind::OpenaiCompatible);
        assert!(m.stt_from_groq);
        let backup = dir.path().join("config.toml.v6.bak");
        assert_eq!(std::fs::read(&backup).unwrap(), original.as_bytes());

        let rewritten = std::fs::read_to_string(&path).unwrap();
        assert!(
            rewritten.contains("kind = \"openai-compatible\""),
            "{rewritten}"
        );
        assert!(
            rewritten.contains(&format!("version = {CONFIG_VERSION}")),
            "{rewritten}"
        );

        // The second load is a no-op: nothing to report, nothing rewritten.
        let (again, m2) = Settings::load_with_migrations(&path).expect("loads again");
        assert_eq!(m2, crate::Migrations::default());
        assert_eq!(again, s);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), rewritten);
        assert_eq!(std::fs::read(&backup).unwrap(), original.as_bytes());
        assert!(!dir.path().join("config.toml.v7.bak").exists());
    }
}

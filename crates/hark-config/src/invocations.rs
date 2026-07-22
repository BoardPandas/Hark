//! The `[invocations]` section: user-authored trigger phrase -> canned text.
//!
//! Its own module because `lib.rs` is already over the project's 500-line
//! rule. Purely additive under `#[serde(default)]`, so [`crate::CONFIG_VERSION`]
//! stays 1: old files load unchanged and new files stay readable by older
//! builds (unknown keys are already tolerated).
//!
//! There is deliberately **no `validate` rule** here. Hard-rejecting a bad
//! phrase would make a hand-edited config unloadable and strand the user
//! with no UI to fix it; instead `hark_dictionary::Expander` skips entries
//! that cannot arm, and the Invocations page explains each one per row.

use serde::{Deserialize, Serialize};

/// Where in a dictation a trigger is allowed to fire. Mirrors
/// `hark_dictionary::Scope` (the parallel-enums pattern this crate already
/// uses for provider and voice taxonomies); the pipeline maps between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Scope {
    /// The trigger must account for the whole dictation. The default: it is
    /// the rule that cannot fire mid-sentence by accident.
    #[default]
    Utterance,
    /// The trigger fires anywhere inside a longer dictation.
    Anywhere,
}

impl Scope {
    /// Plain-language label for the UI and for round-trip tests.
    pub fn label(self) -> &'static str {
        match self {
            Scope::Utterance => "Whole dictation",
            Scope::Anywhere => "Anywhere in the sentence",
        }
    }
}

/// One invocation.
///
/// Derives `Debug` because [`crate::Settings`] does — which means a stray
/// `log::debug!("{settings:?}")` would dump every expansion to disk. Logging
/// discipline for this type is counts, char lengths, and indices only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Invocation {
    /// What the user says. The phrase is the entry's identity; there are no
    /// hidden ids, so the TOML stays hand-editable.
    pub phrase: String,
    /// What gets injected, byte for byte as authored. Multi-line is fine.
    pub expansion: String,
    pub scope: Scope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Invocations {
    pub entries: Vec<Invocation>,
}

#[cfg(test)]
mod tests {
    use crate::Settings;

    use super::*;

    #[test]
    fn absent_section_yields_no_invocations() {
        let s = Settings::from_toml("").expect("empty TOML parses");
        assert!(s.invocations.entries.is_empty());
    }

    #[test]
    fn a_pre_invocations_config_still_loads() {
        // The whole point of staying additive at CONFIG_VERSION 1: a file
        // written by an older build must load without a migration.
        let s = Settings::from_toml(
            r#"
            version = 1

            [provider]
            kind = "groq"

            [dictionary]
            terms = ["Modero"]
            "#,
        )
        .expect("a config predating [invocations] must load");
        assert!(s.invocations.entries.is_empty());
        assert_eq!(s.dictionary.terms, vec!["Modero"]);
    }

    #[test]
    fn scope_defaults_to_whole_dictation() {
        let s = Settings::from_toml(
            "[[invocations.entries]]\nphrase = \"access granted\"\nexpansion = \"x\"",
        )
        .expect("entry without a scope parses");
        assert_eq!(s.invocations.entries[0].scope, Scope::Utterance);
    }

    #[test]
    fn both_scope_values_parse_from_kebab_case() {
        let s = Settings::from_toml(
            r#"
            [[invocations.entries]]
            phrase = "access granted"
            expansion = "a"
            scope = "utterance"

            [[invocations.entries]]
            phrase = "ticket closed"
            expansion = "b"
            scope = "anywhere"
            "#,
        )
        .expect("both scopes parse");
        assert_eq!(s.invocations.entries[0].scope, Scope::Utterance);
        assert_eq!(s.invocations.entries[1].scope, Scope::Anywhere);
    }

    #[test]
    fn round_trips_a_multi_line_expansion_and_both_scopes() {
        // The expansion is injected byte for byte, so the save/load cycle
        // must not touch newlines, tabs, quotes, or trailing spaces.
        let expansion = "You have access to:\n\t- ticketing \n\t- \"remote assist\"\n";
        let mut s = Settings::default();
        s.invocations.entries = vec![
            Invocation {
                phrase: "access granted".to_string(),
                expansion: expansion.to_string(),
                scope: Scope::Utterance,
            },
            Invocation {
                phrase: "ticket closed".to_string(),
                expansion: "Closing this out.".to_string(),
                scope: Scope::Anywhere,
            },
        ];

        let text = s.to_toml().expect("serializes");
        let loaded = Settings::from_toml(&text).expect("round trips");

        assert_eq!(loaded.invocations.entries.len(), 2);
        assert_eq!(loaded.invocations.entries[0].phrase, "access granted");
        assert_eq!(
            loaded.invocations.entries[0].expansion, expansion,
            "the expansion must survive byte for byte"
        );
        assert_eq!(loaded.invocations.entries[0].scope, Scope::Utterance);
        assert_eq!(loaded.invocations.entries[1].scope, Scope::Anywhere);

        // A second cycle must be a fixed point, not a slow drift.
        assert_eq!(loaded.to_toml().expect("re-serializes"), text);
    }

    #[test]
    fn default_settings_serialize_without_an_entries_array() {
        let text = Settings::default().to_toml().expect("serializes");
        assert!(
            !text.contains("[[invocations.entries]]"),
            "an empty set must not write array-of-table headers: {text}"
        );
        Settings::from_toml(&text).expect("serialized defaults re-parse");
    }

    #[test]
    fn an_unarmable_entry_still_loads_rather_than_failing_the_config() {
        // Fail-soft is the whole contract: a one-word trigger and an empty
        // expansion are the Expander's problem to skip, not the loader's to
        // reject. Rejecting here would leave a hand-edited file unloadable
        // and the user with no UI to repair it.
        let s =
            Settings::from_toml("[[invocations.entries]]\nphrase = \"granted\"\nexpansion = \"\"")
                .expect("a bad entry must not fail the whole config");
        assert_eq!(s.invocations.entries.len(), 1);
    }
}

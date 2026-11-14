use serde::{Deserialize, Serialize};

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
    pub(crate) fn migrate(&mut self) {
        for term in std::mem::take(&mut self.legacy_terms) {
            if !self.entries.iter().any(|e| e.term == term) {
                self.entries.push(SpellbookEntry::new(term));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Settings, CONFIG_VERSION};

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
}

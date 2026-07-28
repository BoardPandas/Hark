//! Pure list edits for the spellbook page, and the common-word guard.
//!
//! Split from the page so the rules that decide what enters the user's
//! spellbook are testable without a `Ui`. Every one of them either returns
//! true (the caller persists and restarts the pipeline) or leaves the list
//! exactly as it was.

use hark_config::SpellbookEntry;

/// Add a trimmed, non-empty, non-duplicate entry, with an optional alias.
///
/// A term that already exists is not an error and not a duplicate row: the
/// alias is merged into the existing entry instead. Adding "Eldrazi" twice
/// from two different misheard spellings is the normal way this feature gets
/// used, and it must accumulate aliases rather than refuse the second one.
pub fn add_entry(entries: &mut Vec<SpellbookEntry>, raw_term: &str, raw_alias: &str) -> bool {
    let term = raw_term.trim();
    if term.is_empty() {
        return false;
    }
    let alias = raw_alias.trim();
    // An alias equal to its own term would be a no-op match that only makes
    // the row confusing.
    let alias = (!alias.is_empty() && !alias.eq_ignore_ascii_case(term)).then_some(alias);

    if let Some(existing) = entries.iter_mut().find(|e| e.term == term) {
        return match alias {
            Some(a) if !existing.aliases.iter().any(|x| x.eq_ignore_ascii_case(a)) => {
                existing.aliases.push(a.to_string());
                true
            }
            _ => false,
        };
    }
    entries.push(SpellbookEntry {
        term: term.to_string(),
        aliases: alias
            .map(|a| vec![a.to_string()])
            .into_iter()
            .flatten()
            .collect(),
    });
    true
}

/// Commit an inline term edit: trimmed and unique replaces; empty or duplicate
/// input reverts (a row is deleted with its button, never by blanking).
/// Aliases ride along with the entry.
pub fn commit_edit(entries: &mut [SpellbookEntry], index: usize, raw: &str) -> bool {
    let term = raw.trim();
    if index >= entries.len() || term.is_empty() || entries[index].term == term {
        return false;
    }
    if entries.iter().any(|e| e.term == term) {
        return false;
    }
    entries[index].term = term.to_string();
    true
}

/// Add an alias to an existing entry. Rejects blanks, duplicates within the
/// entry, and an alias equal to the term itself.
pub fn add_alias(entries: &mut [SpellbookEntry], index: usize, raw: &str) -> bool {
    let alias = raw.trim();
    if index >= entries.len() || alias.is_empty() {
        return false;
    }
    let entry = &mut entries[index];
    if alias.eq_ignore_ascii_case(&entry.term)
        || entry.aliases.iter().any(|a| a.eq_ignore_ascii_case(alias))
    {
        return false;
    }
    entry.aliases.push(alias.to_string());
    true
}

/// Remove one alias from an entry. Out-of-range is a no-op, never a panic:
/// the list can move between the click and the frame that handles it.
pub fn remove_alias(entries: &mut [SpellbookEntry], index: usize, alias_index: usize) -> bool {
    match entries.get_mut(index) {
        Some(entry) if alias_index < entry.aliases.len() => {
            entry.aliases.remove(alias_index);
            true
        }
        _ => false,
    }
}

/// Remove the entry a just-added term created. Matches by term rather than
/// index because the list can move underneath the undo affordance.
pub fn undo_add(entries: &mut Vec<SpellbookEntry>, added: &str) -> bool {
    match entries.iter().position(|e| e.term == added) {
        Some(index) => {
            entries.remove(index);
            true
        }
        None => false,
    }
}

/// Words common enough that aliasing one would fire constantly.
///
/// Deliberately tiny and deliberately not a dictionary: this warns, it does not
/// block, so it only has to catch the cases where the mistake is obvious and
/// expensive. A full frequency list would add weight and false confidence
/// without changing the outcome, since the user decides either way.
const COMMON_WORDS: &[&str] = &[
    "a", "about", "all", "an", "and", "are", "as", "at", "be", "but", "by", "can", "do", "for",
    "from", "get", "go", "had", "has", "have", "he", "her", "here", "him", "his", "i", "if", "in",
    "is", "it", "its", "just", "know", "like", "make", "me", "my", "no", "not", "now", "of", "on",
    "one", "or", "our", "out", "over", "say", "see", "she", "so", "some", "take", "than", "that",
    "the", "their", "them", "then", "there", "these", "they", "think", "this", "time", "to", "up",
    "us", "use", "want", "was", "way", "we", "well", "were", "what", "when", "which", "who",
    "will", "with", "would", "you", "your",
];

/// True when every word of `alias` is a common English word.
///
/// Whole-phrase, not any-word: "the Eldrazi" is a perfectly sensible alias and
/// warning about it would train the user to ignore the warning. Only an alias
/// made *entirely* of common words is the dangerous kind.
pub fn is_common_word(alias: &str) -> bool {
    let mut words = alias.split_whitespace().peekable();
    if words.peek().is_none() {
        return false;
    }
    words.all(|w| {
        let w = w
            .trim_matches(|c: char| !c.is_alphanumeric())
            .to_lowercase();
        !w.is_empty() && COMMON_WORDS.contains(&w.as_str())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(term: &str, aliases: &[&str]) -> SpellbookEntry {
        SpellbookEntry {
            term: term.to_string(),
            aliases: aliases.iter().map(|a| a.to_string()).collect(),
        }
    }

    #[test]
    fn add_trims_dedupes_and_rejects_empty() {
        let mut entries = vec![entry("Hark", &[])];
        assert!(add_entry(&mut entries, "  Deepgram  ", ""));
        assert_eq!(entries[1].term, "Deepgram");
        assert!(!add_entry(&mut entries, "Hark", ""));
        assert!(!add_entry(&mut entries, "   ", ""));
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn adding_an_existing_term_merges_the_alias_instead_of_refusing() {
        // Two different mishearings of one name is the normal path through this
        // feature; the second must not bounce off a duplicate check.
        let mut entries = vec![entry("Eldrazi", &["Al Drazi"])];
        assert!(add_entry(&mut entries, "Eldrazi", "El Drossy"));
        assert_eq!(entries.len(), 1, "no duplicate row");
        assert_eq!(entries[0].aliases, ["Al Drazi", "El Drossy"]);

        // The same alias twice changes nothing.
        assert!(!add_entry(&mut entries, "Eldrazi", "al drazi"));
        assert_eq!(entries[0].aliases.len(), 2);
    }

    #[test]
    fn an_alias_equal_to_its_own_term_is_dropped() {
        let mut entries = Vec::new();
        assert!(add_entry(&mut entries, "Eldrazi", "eldrazi"));
        assert!(
            entries[0].aliases.is_empty(),
            "a self-alias is a no-op match and only clutters the row"
        );
    }

    #[test]
    fn edit_replaces_the_term_and_keeps_its_aliases() {
        let mut entries = vec![entry("Hark", &["park"]), entry("Deepgram", &[])];
        assert!(commit_edit(&mut entries, 0, " Harken "));
        assert_eq!(entries[0].term, "Harken");
        assert_eq!(entries[0].aliases, ["park"], "aliases survive a term edit");

        assert!(!commit_edit(&mut entries, 0, "  "));
        assert!(!commit_edit(&mut entries, 0, "Deepgram"));
        assert!(!commit_edit(&mut entries, 1, "Deepgram"));
        assert!(!commit_edit(&mut entries, 9, "x"));
        assert_eq!(entries[0].term, "Harken");
    }

    #[test]
    fn aliases_are_added_and_removed_without_panicking_out_of_range() {
        let mut entries = vec![entry("Eldrazi", &[])];
        assert!(add_alias(&mut entries, 0, " Al Drazi "));
        assert_eq!(entries[0].aliases, ["Al Drazi"]);
        assert!(
            !add_alias(&mut entries, 0, "al drazi"),
            "case-insensitive dupe"
        );
        assert!(
            !add_alias(&mut entries, 0, "Eldrazi"),
            "equals its own term"
        );
        assert!(!add_alias(&mut entries, 0, "   "));
        assert!(!add_alias(&mut entries, 9, "x"));

        assert!(remove_alias(&mut entries, 0, 0));
        assert!(entries[0].aliases.is_empty());
        assert!(!remove_alias(&mut entries, 0, 0));
        assert!(!remove_alias(&mut entries, 9, 0));
    }

    #[test]
    fn undo_removes_the_added_entry_and_tolerates_it_being_gone() {
        let mut entries = vec![entry("Hark", &[]), entry("Eldrazi", &["Al Drazi"])];
        assert!(undo_add(&mut entries, "Eldrazi"));
        assert_eq!(entries.len(), 1);
        assert!(!undo_add(&mut entries, "Eldrazi"));
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn common_word_guard_flags_the_dangerous_aliases_only() {
        // An alias made entirely of common words fires on ordinary speech.
        assert!(is_common_word("there"));
        assert!(is_common_word("The Way"));
        assert!(is_common_word("out of time"));

        // A proper noun, or any phrase containing one, is the normal case and
        // must not warn -- a warning everyone dismisses protects nobody.
        assert!(!is_common_word("Al Drazi"));
        assert!(!is_common_word("the Eldrazi"));
        assert!(!is_common_word("Modero"));
        assert!(!is_common_word(""));
        assert!(!is_common_word("   "));
    }
}

//! The History handoff as the Spellbook page experiences it: what a save, a
//! cancel and an undo leave behind. Page-level, so no `Ui` is needed.

use super::*;

fn entry(term: &str, aliases: &[&str]) -> SpellbookEntry {
    SpellbookEntry {
        term: term.to_string(),
        aliases: aliases.iter().map(|a| a.to_string()).collect(),
    }
}

#[test]
fn saving_a_handoff_keeps_the_misheard_text_as_an_alias_and_asks_to_go_back() {
    let mut page = SpellbookPage::new();
    let mut entries = Vec::new();
    page.prime_add("Al Drazi".to_string());
    assert_eq!(
        page.add_alias, "Al Drazi",
        "the selection arrives as an alias"
    );
    assert!(page.add.is_empty(), "the term is for the user to type");
    assert!(page.add_needs_focus, "focus lands in the term field");

    page.add = "Eldrazi".to_string();
    assert!(page.submit_add(&mut entries));
    assert_eq!(entries, [entry("Eldrazi", &["Al Drazi"])]);
    assert_eq!(page.last_added(), Some("Eldrazi"));
    assert!(page.take_return_to_history());
    assert!(
        !page.take_return_to_history(),
        "asked for once, not every frame"
    );
    assert!(page.primed_from.is_none(), "the prefill does not linger");

    assert!(page.undo_last(&mut entries));
    assert!(entries.is_empty());
    assert_eq!(page.last_added(), None);
}

#[test]
fn cancelling_a_handoff_adds_nothing_and_asks_to_go_back() {
    let mut page = SpellbookPage::new();
    page.prime_add("Al Drazi".to_string());
    page.cancel_handoff();
    assert!(page.take_return_to_history());
    assert!(page.add.is_empty() && page.add_alias.is_empty());
    assert!(page.primed_from.is_none());
    assert_eq!(page.last_added(), None);
}

#[test]
fn a_plain_add_never_sends_the_user_to_history() {
    let mut page = SpellbookPage::new();
    let mut entries = Vec::new();
    page.add = "Deepgram".to_string();
    assert!(page.submit_add(&mut entries));
    assert!(!page.take_return_to_history());
}

#[test]
fn a_handoff_that_changes_nothing_stays_put() {
    // The alias is already there: nothing was added, so there is nothing to
    // report and no reason to move the user.
    let mut page = SpellbookPage::new();
    let mut entries = vec![entry("Eldrazi", &["Al Drazi"])];
    page.prime_add("al drazi".to_string());
    page.add = "Eldrazi".to_string();
    assert!(!page.submit_add(&mut entries));
    assert!(!page.take_return_to_history());
    assert_eq!(page.last_added(), None);
}

#[test]
fn undoing_a_handoff_into_an_existing_term_keeps_that_term() {
    let mut page = SpellbookPage::new();
    let mut entries = vec![entry("Eldrazi", &["El Drossy"])];
    page.prime_add("Al Drazi".to_string());
    page.add = "Eldrazi".to_string();
    assert!(page.submit_add(&mut entries));
    assert_eq!(entries[0].aliases, ["El Drossy", "Al Drazi"]);

    assert!(page.undo_last(&mut entries));
    assert_eq!(entries, [entry("Eldrazi", &["El Drossy"])]);
}

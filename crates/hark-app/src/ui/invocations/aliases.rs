//! Alternate-phrase field and its pure validation rules.

use crate::theme;
use egui::{Key, RichText, TextEdit, Ui};
use hark_config::Invocation;
use hark_spellbook::MIN_TRIGGER_WORDS;

/// Render the existing alternates plus the add row. Returns true when any
/// matcher input changed, so the caller can rebuild its preview once.
pub fn show(
    ui: &mut Ui,
    phrase: &str,
    aliases: &mut Vec<String>,
    new_alias: &mut String,
    entries: &[Invocation],
    editing: Option<usize>,
) -> bool {
    ui.label(RichText::new("Also recognize").text_style(theme::subheading()));
    ui.label(
        RichText::new("Exact alternatives for what transcription may hear. Two words or more.")
            .small()
            .weak(),
    );
    ui.add_space(4.0);

    let mut changed = false;
    let mut remove = None;
    for (index, alias) in aliases.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(RichText::new(alias).monospace().small());
            if ui
                .small_button(theme::icon_text(theme::icons::X))
                .on_hover_text("Remove this alternate phrase")
                .clicked()
            {
                remove = Some(index);
            }
        });
    }
    if let Some(index) = remove {
        aliases.remove(index);
        changed = true;
    }

    let mut alias_problem = None;
    let mut add = false;
    ui.horizontal(|ui| {
        let response = ui.add(
            TextEdit::singleline(new_alias)
                .hint_text("for example, come in and")
                .desired_width(320.0),
        );
        changed |= response.changed();
        alias_problem = candidate_problem(phrase, aliases, new_alias, entries, editing);
        if response.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
            add = true;
            response.request_focus();
        }
        if ui
            .add_enabled(
                !new_alias.trim().is_empty() && alias_problem.is_none(),
                egui::Button::new("Add"),
            )
            .clicked()
        {
            add = true;
        }
    });
    if let Some(problem) = alias_problem {
        if !new_alias.trim().is_empty() {
            ui.label(
                RichText::new(problem)
                    .small()
                    .color(theme::warning(ui.visuals())),
            );
        }
    }
    if add && candidate_problem(phrase, aliases, new_alias, entries, editing).is_none() {
        aliases.push(new_alias.trim().to_string());
        new_alias.clear();
        changed = true;
    }
    changed
}

/// Validate the primary phrase against other entries, then every committed or
/// pending alternate against this invocation and the rest of the list.
pub fn entry_problem(
    phrase: &str,
    aliases: &[String],
    new_alias: &str,
    entries: &[Invocation],
    editing: Option<usize>,
) -> Option<String> {
    let key = hark_spellbook::normalized_phrase(phrase);
    if phrase_used_by_other_entry(entries, editing, &key) {
        return Some(
            "Another invocation already uses this phrase as a trigger or alternate.".to_string(),
        );
    }

    let mut seen = vec![key];
    for alias in aliases {
        if alias.trim().is_empty() {
            return Some("Remove the blank alternate phrase.".to_string());
        }
        if hark_spellbook::phrase_word_count(alias) < MIN_TRIGGER_WORDS {
            return Some("Each alternate phrase needs at least two words.".to_string());
        }
        let key = hark_spellbook::normalized_phrase(alias);
        if seen.contains(&key) {
            return Some(
                "An alternate phrase duplicates this trigger or another alternate.".to_string(),
            );
        }
        if phrase_used_by_other_entry(entries, editing, &key) {
            return Some(
                "Another invocation already uses one of these alternate phrases.".to_string(),
            );
        }
        seen.push(key);
    }
    if !new_alias.trim().is_empty() {
        return candidate_problem(phrase, aliases, new_alias, entries, editing);
    }
    None
}

fn candidate_problem(
    phrase: &str,
    aliases: &[String],
    candidate: &str,
    entries: &[Invocation],
    editing: Option<usize>,
) -> Option<String> {
    if candidate.trim().is_empty() {
        return Some("Enter an alternate phrase.".to_string());
    }
    if hark_spellbook::phrase_word_count(candidate) < MIN_TRIGGER_WORDS {
        return Some("An alternate phrase needs at least two words.".to_string());
    }
    let key = hark_spellbook::normalized_phrase(candidate);
    if key == hark_spellbook::normalized_phrase(phrase)
        || aliases
            .iter()
            .any(|alias| hark_spellbook::normalized_phrase(alias) == key)
    {
        return Some("That phrase is already part of this invocation.".to_string());
    }
    phrase_used_by_other_entry(entries, editing, &key).then(|| {
        "Another invocation already uses that phrase as a trigger or alternate.".to_string()
    })
}

fn phrase_used_by_other_entry(
    entries: &[Invocation],
    editing: Option<usize>,
    normalized: &str,
) -> bool {
    entries.iter().enumerate().any(|(index, entry)| {
        Some(index) != editing
            && std::iter::once(entry.phrase.as_str())
                .chain(entry.aliases.iter().map(String::as_str))
                .any(|phrase| hark_spellbook::normalized_phrase(phrase) == normalized)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hark_config::Scope;

    fn entry(phrase: &str, aliases: &[&str]) -> Invocation {
        Invocation {
            phrase: phrase.to_string(),
            aliases: aliases.iter().map(|alias| alias.to_string()).collect(),
            expansion: "text".to_string(),
            scope: Scope::Utterance,
        }
    }

    #[test]
    fn an_alternate_must_be_two_words_and_unique_within_the_invocation() {
        assert!(candidate_problem("commit and", &[], "and", &[], None)
            .unwrap()
            .contains("at least two words"));
        assert!(
            candidate_problem("commit and", &[], "Commit-And!", &[], None)
                .unwrap()
                .contains("already part")
        );

        let aliases = vec!["come in and".to_string()];
        assert!(
            candidate_problem("commit and", &aliases, "COME IN AND", &[], None)
                .unwrap()
                .contains("already part")
        );
        assert_eq!(
            candidate_problem("commit and", &aliases, "coming in", &[], None),
            None
        );
    }

    #[test]
    fn triggers_and_alternates_cannot_collide_across_invocations() {
        let entries = [entry("commit and", &["come in and"])];

        assert!(entry_problem("come in and", &[], "", &entries, None).is_some());
        assert!(candidate_problem("another trigger", &[], "commit and", &entries, None).is_some());
        assert!(
            candidate_problem("another trigger", &[], "COME IN AND!", &entries, None).is_some()
        );
    }

    #[test]
    fn editing_an_entry_may_keep_its_own_alternates() {
        let entries = [entry("commit and", &["come in and"])];

        assert_eq!(
            entry_problem(
                "commit and",
                &["come in and".to_string()],
                "",
                &entries,
                Some(0),
            ),
            None
        );
        assert_eq!(
            candidate_problem(
                "commit and",
                &["come in and".to_string()],
                "another version",
                &entries,
                Some(0),
            ),
            None
        );
    }
}

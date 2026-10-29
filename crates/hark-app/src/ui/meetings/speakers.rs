//! The speakers card: one chip per speaker with lines, Me included, in color
//! order. Click a chip to rename it; a name another speaker already has makes
//! them one person, and the field says so before it saves. A guessed name
//! carries a marker until the user saves it (spec §2.5).

use crate::theme;
use egui::{RichText, TextEdit, Ui};
use hark_meeting::export::format_timestamp;
use hark_meeting::{default_label, Roster, SpeakerKey};
use hark_store::{NameSource, SpeakerName};

pub(super) struct SpeakersCard {
    /// The key being renamed and its edit buffer.
    renaming: Option<(SpeakerKey, String)>,
}

impl SpeakersCard {
    pub fn new() -> Self {
        SpeakersCard { renaming: None }
    }

    pub fn reset(&mut self) {
        self.renaming = None;
    }

    /// Returns a name to save (`SetSpeakerName`) when the user finished one.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        roster: &Roster,
        names: &[SpeakerName],
    ) -> Option<(SpeakerKey, String)> {
        let speaking = roster.speaking();
        if speaking.is_empty() {
            return None;
        }
        let mut save = None;
        ui.label(RichText::new("Speakers").strong());
        if names.is_empty() {
            ui.label(
                RichText::new("Click a speaker to name them.")
                    .small()
                    .weak(),
            );
        }
        ui.horizontal_wrapped(|ui| {
            for key in speaking {
                let hue = theme::speaker_hue(roster.slot(key));
                let row = name_row(names, key);
                ui.horizontal(|ui| {
                    theme::swatch(ui, hue);
                    match &mut self.renaming {
                        Some((editing, buf)) if *editing == key => {
                            let r = ui.add(
                                TextEdit::singleline(buf)
                                    .char_limit(NAME_MAX_CHARS)
                                    .desired_width(140.0),
                            );
                            if let Some(other) = roster.named(buf, key) {
                                ui.label(
                                    RichText::new(format!(
                                        "Shown as the same person as {}",
                                        default_label(other)
                                    ))
                                    .small()
                                    .weak(),
                                );
                            }
                            if r.lost_focus() {
                                let cancelled = ui.input(|i| i.key_pressed(egui::Key::Escape));
                                if !cancelled {
                                    if let Some(name) =
                                        rename_to_send(row, &default_label(key), buf)
                                    {
                                        save = Some((key, name));
                                    }
                                }
                                self.renaming = None;
                            } else {
                                r.request_focus();
                            }
                        }
                        _ => {
                            let label = roster.label(key);
                            let chip = ui
                                .add(theme::chip_button(ui.style(), hue, &label, None))
                                .on_hover_text("Rename");
                            if chip.clicked() {
                                self.renaming = Some((key, label));
                            }
                            if let Some(row) = row.filter(|r| r.source == NameSource::Guess) {
                                ui.label(theme::icon_text(theme::icons::SPARKLE).weak())
                                    .on_hover_text(guessed_tooltip(row.evidence_ms));
                            }
                        }
                    }
                });
            }
        });
        ui.add_space(theme::GAP);
        save
    }
}

/// The settings field and every rename share this cap (spec §5.3).
pub(super) const NAME_MAX_CHARS: usize = 40;

fn name_row(names: &[SpeakerName], key: SpeakerKey) -> Option<&SpeakerName> {
    let text = key.to_string();
    names.iter().find(|n| n.key == text)
}

/// What a finished rename should store, if anything. Saving an unchanged
/// guess confirms it (source becomes user); saving an unchanged user name,
/// or the default label where there is no name, is a no-op; an empty name
/// reverts to the default label.
fn rename_to_send(row: Option<&SpeakerName>, default: &str, typed: &str) -> Option<String> {
    let typed = typed.trim();
    match row {
        Some(r) if r.source == NameSource::Guess => Some(typed.to_string()),
        Some(r) if r.display_name.trim() == typed => None,
        Some(_) => Some(typed.to_string()),
        None if typed.is_empty() || typed == default => None,
        None => Some(typed.to_string()),
    }
}

fn guessed_tooltip(evidence_ms: Option<i64>) -> String {
    match evidence_ms {
        Some(ms) => format!(
            "Guessed from a name said at {}. Click to change.",
            format_timestamp(ms.max(0) as u64)
        ),
        None => "Guessed from a name said in the call. Click to change.".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, source: NameSource) -> SpeakerName {
        SpeakerName {
            key: "them:1".to_string(),
            display_name: name.to_string(),
            source,
            evidence_ms: Some(724_000),
        }
    }

    #[test]
    fn saving_an_unchanged_guess_confirms_it() {
        let guess = row("Sam", NameSource::Guess);
        assert_eq!(
            rename_to_send(Some(&guess), "Speaker 2", "Sam").as_deref(),
            Some("Sam")
        );
    }

    #[test]
    fn unchanged_names_and_default_labels_store_nothing() {
        let user = row("Sam", NameSource::User);
        assert_eq!(rename_to_send(Some(&user), "Speaker 2", " Sam "), None);
        assert_eq!(rename_to_send(None, "Speaker 2", "Speaker 2"), None);
        assert_eq!(rename_to_send(None, "Speaker 2", "  "), None);
    }

    #[test]
    fn a_new_or_emptied_name_is_sent() {
        let user = row("Sam", NameSource::User);
        assert_eq!(
            rename_to_send(Some(&user), "Speaker 2", "Samantha").as_deref(),
            Some("Samantha")
        );
        // Empty reverts the stored name to the default label.
        assert_eq!(
            rename_to_send(Some(&user), "Speaker 2", "").as_deref(),
            Some("")
        );
        assert_eq!(rename_to_send(None, "Me", "Chaz").as_deref(), Some("Chaz"));
    }

    #[test]
    fn the_guessed_marker_names_when_the_name_was_heard() {
        assert_eq!(
            guessed_tooltip(Some(724_000)),
            "Guessed from a name said at 12:04. Click to change."
        );
        assert!(guessed_tooltip(None).contains("in the call"));
    }

    #[test]
    fn the_card_lists_me_and_renders_in_every_palette_without_panics() {
        let roster = Roster::new(
            [(0, None, None), (1, Some(0), None), (1, Some(1), None)],
            [("them:1", "Sam")],
        );
        let names = [row("Sam", NameSource::Guess)];
        for appearance in crate::theme::Appearance::ALL {
            let ctx = egui::Context::default();
            crate::theme::apply(&ctx);
            crate::theme::set_appearance(&ctx, appearance);
            let mut card = SpeakersCard::new();
            let mut text = String::new();
            let mut output = ctx.run_ui(Default::default(), |ui| {
                assert!(card.show(ui, &roster, &names).is_none());
            });
            output.textures_delta.clear();
            for clipped in &output.shapes {
                if let egui::Shape::Text(t) = &clipped.shape {
                    text.push_str(t.galley.text());
                    text.push('|');
                }
            }
            assert!(text.contains("|Me|"), "{text}");
            assert!(text.contains("Speaker 1") && text.contains("Sam"), "{text}");
        }
    }
}

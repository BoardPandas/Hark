//! The notes card: summary, key points, decisions and tickable action items,
//! rendered with the meeting's current speaker names. Ticks write the stored
//! notes (speaker tokens and all), never the rendered copy. When moved lines
//! left the notes crediting the old speaker, one line says so, with an
//! Update notes button (spec §2.5, §10).

use crate::meeting::NotesStatus;
use crate::theme;
use egui::{RichText, Ui};
use hark_meeting::SpeakerKey;
use hark_pipeline::meeting::NotesRequest;
use hark_store::MeetingDetail;
use hark_voice::MeetingNotes;

/// What the user did in the card this frame.
#[derive(Default)]
pub(super) struct NotesAction {
    /// An action item ticked or unticked: (index, done).
    pub toggled: Option<(usize, bool)>,
    pub update: bool,
}

/// `stale` is the line under the heading, if the notes are stale; `None`
/// inside means a rewrite is running.
pub(super) fn card(
    ui: &mut Ui,
    notes: &MeetingNotes,
    stale: Option<Option<String>>,
) -> NotesAction {
    let mut action = NotesAction::default();
    theme::card(ui, |ui| {
        ui.label(RichText::new("Summary").text_style(theme::subheading()));
        match &stale {
            Some(Some(text)) => {
                ui.horizontal_wrapped(|ui| {
                    ui.label(theme::icon_label_job(
                        ui.style(),
                        theme::icons::WARNING,
                        text,
                    ));
                    action.update = ui.button("Update notes").clicked();
                });
            }
            Some(None) => {
                ui.label(theme::icon_label_job(
                    ui.style(),
                    theme::icons::SPINNER,
                    "Updating notes…",
                ));
            }
            None => {}
        }
        ui.label(&notes.summary);
        list(ui, "Key points", &notes.key_points);
        list(ui, "Decisions", &notes.decisions);
        if !notes.action_items.is_empty() {
            ui.add_space(theme::GAP);
            ui.label(RichText::new("Action items").strong());
            for (i, item) in notes.action_items.iter().enumerate() {
                let label = match &item.owner {
                    Some(owner) => format!("{} ({owner})", item.text),
                    None => item.text.clone(),
                };
                let mut done = item.done;
                if ui.checkbox(&mut done, label).changed() {
                    action.toggled = Some((i, done));
                }
            }
        }
    });
    action
}

fn list(ui: &mut Ui, heading: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    ui.add_space(theme::GAP);
    ui.label(RichText::new(heading).strong());
    for item in items {
        ui.label(format!("• {item}"));
    }
}

/// The stored notes' JSON with one action item's tick changed.
pub(super) fn ticked(stored: &mut MeetingNotes, index: usize, done: bool) -> Option<String> {
    stored.action_items.get_mut(index)?.done = done;
    Some(stored.to_json())
}

/// The stale line for a meeting, `Some(None)` while a rewrite runs, `None`
/// when the notes match the speakers.
pub(super) fn stale_line(
    detail: &MeetingDetail,
    status: Option<NotesStatus>,
) -> Option<Option<String>> {
    detail.notes_stale_ms?;
    if status == Some(NotesStatus::Updating) {
        return Some(None);
    }
    let moved = detail
        .segments
        .iter()
        .filter(|s| {
            SpeakerKey::effective(s.channel, s.speaker, s.speaker_override.as_deref())
                != SpeakerKey::transcribed(s.channel, s.speaker)
        })
        .count();
    let failed = match status {
        Some(NotesStatus::Failed(reason)) => Some(reason),
        _ => None,
    };
    Some(Some(stale_text(moved, failed)))
}

fn stale_text(moved: usize, failed: Option<&str>) -> String {
    let lines = match moved {
        _ if failed.is_some() => "moved lines".to_string(),
        0 => "moved lines".to_string(),
        1 => "1 moved line".to_string(),
        n => format!("{n} moved lines"),
    };
    let first = format!("Notes still credit the earlier speaker for {lines}.");
    let next = match failed {
        None => return first,
        Some("no-provider") => "Add a text provider key in Settings to update them.",
        Some("notes-off") => "Turn on notes in Settings > Meetings to update them.",
        Some("auth") => {
            "Your text provider refused the key. Check it in Settings, then choose Update notes."
        }
        Some("unavailable") => {
            "Meeting notes are not running right now. Choose Update notes later."
        }
        Some(_) => "Your text provider did not answer. Choose Update notes to try again.",
    };
    format!("{first} {next}")
}

/// A rewrite request from the meeting as stored now: every line with the
/// speaker it is shown as.
pub(super) fn request(detail: &MeetingDetail) -> NotesRequest {
    NotesRequest {
        id: detail.summary.id.clone(),
        lines: detail
            .segments
            .iter()
            .map(|s| {
                let key =
                    SpeakerKey::effective(s.channel, s.speaker, s.speaker_override.as_deref());
                (s.start_ms.max(0) as u64, key, s.text.clone())
            })
            .collect(),
        requested_ms: jiff::Timestamp::now().as_millisecond(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tick_writes_the_stored_token_notes_never_the_rendered_names() {
        let mut stored = hark_voice::parse_notes(
            r#"{"title":"t","summary":"s","action_items":[{"text":"Call {Me}"}]}"#,
            &[("Me".to_string(), "me".to_string())],
        )
        .unwrap();
        let shown = stored.render(&[("me".into(), "Me".into(), "Chaz".into())]);
        assert_eq!(shown.action_items[0].text, "Call Chaz");
        let json = ticked(&mut stored, 0, true).unwrap();
        let written = MeetingNotes::from_json(&json).unwrap();
        assert_eq!(written.action_items[0].text, "Call {{me}}");
        assert!(written.action_items[0].done);
        assert_eq!(written.speaker_refs, hark_voice::SpeakerRefs::Tokens);
        assert!(ticked(&mut stored, 5, true).is_none());
    }

    #[test]
    fn the_stale_line_counts_moved_lines_and_names_each_failure() {
        assert_eq!(
            stale_text(3, None),
            "Notes still credit the earlier speaker for 3 moved lines."
        );
        assert_eq!(
            stale_text(1, None),
            "Notes still credit the earlier speaker for 1 moved line."
        );
        assert_eq!(
            stale_text(2, Some("no-provider")),
            "Notes still credit the earlier speaker for moved lines. Add a text provider key in \
             Settings to update them."
        );
        for (slug, words) in [
            ("timeout", "did not answer"),
            ("network", "did not answer"),
            ("provider", "did not answer"),
            ("auth", "refused the key"),
            ("notes-off", "Turn on notes"),
            ("unavailable", "not running"),
        ] {
            assert!(stale_text(1, Some(slug)).contains(words), "{slug}");
        }
    }

    #[test]
    fn stale_notes_show_a_line_and_a_running_rewrite_hides_the_button() {
        let mut detail = super::super::transcript::tests::detail();
        assert_eq!(stale_line(&detail, None), None, "not stale");
        detail.notes_stale_ms = Some(1);
        assert_eq!(
            stale_line(&detail, None),
            Some(Some(
                "Notes still credit the earlier speaker for 1 moved line.".into()
            ))
        );
        assert_eq!(stale_line(&detail, Some(NotesStatus::Updating)), Some(None));
        let request = request(&detail);
        let keys: Vec<SpeakerKey> = request.lines.iter().map(|(_, k, _)| *k).collect();
        assert_eq!(
            keys,
            [
                SpeakerKey::Me,
                SpeakerKey::Diarized(0),
                SpeakerKey::Me,
                SpeakerKey::Diarized(2)
            ],
            "effective speakers, the moved line included"
        );
    }
}

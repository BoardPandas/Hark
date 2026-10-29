//! The notes card: summary, key points, decisions and tickable action items,
//! rendered with the meeting's current speaker names, each in its speaker's
//! color (the name is the text, so color is never alone). Ticks write the
//! stored notes (speaker tokens and all), never a rendered copy. When moved
//! lines left the notes crediting the old speaker, one line says so, with
//! an Update notes button (spec §2.5, §10).

use crate::meeting::NotesStatus;
use crate::theme;
use egui::text::{LayoutJob, TextFormat};
use egui::{RichText, Ui};
use hark_meeting::{Roster, SpeakerKey};
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

/// Everything a notes field renders through: the roster for colors, and its
/// `(key, default label, display label)` table.
pub(super) struct Names<'a> {
    pub roster: &'a Roster,
    pub table: &'a [(String, String, String)],
}

/// `notes` as stored. `stale` is the line under the heading, if the notes
/// are stale; `None` inside means a rewrite is running.
pub(super) fn card(
    ui: &mut Ui,
    notes: &MeetingNotes,
    names: &Names,
    stale: Option<Option<String>>,
) -> NotesAction {
    let mut action = NotesAction::default();
    let field = |ui: &Ui, parts: &[(&str, &str)]| field_job(ui, notes, names, parts);
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
        ui.label(field(ui, &[("", &notes.summary)]));
        let lists = [
            ("Key points", &notes.key_points),
            ("Decisions", &notes.decisions),
        ];
        for (heading, items) in lists {
            if items.is_empty() {
                continue;
            }
            ui.add_space(theme::GAP);
            ui.label(RichText::new(heading).strong());
            for item in items {
                ui.label(field(ui, &[("• ", item)]));
            }
        }
        if !notes.action_items.is_empty() {
            ui.add_space(theme::GAP);
            ui.label(RichText::new("Action items").strong());
            for (i, item) in notes.action_items.iter().enumerate() {
                let label = match &item.owner {
                    Some(owner) => field(ui, &[("", &item.text), (" (", owner), (")", "")]),
                    None => field(ui, &[("", &item.text)]),
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

/// One field as a layout job: each `(plain prefix, notes text)` part in
/// order, with speaker names in their hue's ink and everything else in the
/// body text color. The text is exactly what `MeetingNotes::render` shows.
fn field_job(ui: &Ui, notes: &MeetingNotes, names: &Names, parts: &[(&str, &str)]) -> LayoutJob {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let plain = ui.visuals().text_color();
    let mut job = LayoutJob::default();
    for (prefix, text) in parts {
        if !prefix.is_empty() {
            job.append(prefix, 0.0, TextFormat::simple(font.clone(), plain));
        }
        for span in hark_voice::speaker_spans(text, names.table, notes.speaker_refs) {
            let color = span
                .speaker
                .and_then(|i| SpeakerKey::parse(&names.table[i].0))
                .map_or(plain, |key| {
                    theme::hue(ui.visuals(), theme::speaker_hue(names.roster.slot(key)))
                });
            job.append(&span.text, 0.0, TextFormat::simple(font.clone(), color));
        }
    }
    job
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
    fn speaker_names_in_the_card_are_drawn_in_their_hue_with_the_rendered_text() {
        let detail = super::super::transcript::tests::detail();
        let roster = crate::storage::meetings::roster_of(&detail);
        let table = roster.notes_names();
        let notes = hark_voice::parse_notes(
            r#"{"title":"t","summary":"{Speaker 1} met {Me}.","action_items":[{"text":"Send it","owner":"{Me}"}]}"#,
            &[
                ("Me".to_string(), "me".to_string()),
                ("Speaker 1".to_string(), "them:0".to_string()),
            ],
        )
        .unwrap();
        let rendered = notes.render(&table);
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let mut visuals = egui::Visuals::default();
        let mut sections = Vec::new();
        let mut out = ctx.run_ui(Default::default(), |ui| {
            visuals = ui.visuals().clone();
            let names = Names {
                roster: &roster,
                table: &table,
            };
            card(ui, &notes, &names, None);
        });
        out.textures_delta.clear();
        let mut texts = Vec::new();
        for clipped in &out.shapes {
            if let egui::Shape::Text(t) = &clipped.shape {
                texts.push(t.galley.text().to_string());
                for s in &t.galley.job.sections {
                    sections.push((
                        t.galley.job.text[s.byte_range.start.0..s.byte_range.end.0].to_string(),
                        s.format.color,
                    ));
                }
            }
        }
        assert!(texts.contains(&rendered.summary), "{texts:?}");
        let ink = |slot| theme::hue(&visuals, theme::speaker_hue(slot));
        assert!(
            sections.contains(&("Speaker 1".to_string(), ink(1))),
            "{sections:?}"
        );
        assert!(
            sections.contains(&("Me".to_string(), ink(0))),
            "{sections:?}"
        );
        assert!(sections.contains(&(" met ".to_string(), visuals.text_color())));
        assert!(texts.iter().any(|t| t == "Send it (Me)"), "{texts:?}");
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

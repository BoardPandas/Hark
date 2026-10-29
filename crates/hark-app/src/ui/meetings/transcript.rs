//! The transcript: each line has its speaker's colored rule, the time, a
//! speaker chip that opens the reassign menu, and the selectable text. The
//! menu lists Me and everyone else in color order, "As transcribed" once a
//! line was moved, and "New speaker…" for someone the diarization missed
//! (spec §2.5). Hues sit next to names only; the current speaker is a
//! selected row, never a check glyph.

use crate::theme;
use egui::containers::menu::{MenuButton, MenuConfig};
use egui::{PopupCloseBehavior, Rect, RichText, Sense, TextEdit, Ui, Vec2};
use hark_meeting::export::format_timestamp;
use hark_meeting::{Person, Roster, SpeakerKey};
use hark_store::MeetingDetail;

/// What the user chose for one line.
pub(super) enum Reassign {
    /// Show the line as `key`, or as transcribed again (`None`).
    Move {
        segment_id: i64,
        key: Option<SpeakerKey>,
    },
    /// A new speaker, named when a name was typed, takes the line.
    New {
        segment_id: i64,
        key: SpeakerKey,
        name: String,
    },
}

pub(super) struct TranscriptView {
    /// The open "New speaker…" field: its line and buffer.
    naming: Option<(i64, String)>,
}

impl TranscriptView {
    pub fn new() -> Self {
        TranscriptView { naming: None }
    }

    pub fn reset(&mut self) {
        self.naming = None;
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        detail: &MeetingDetail,
        roster: &Roster,
    ) -> Option<Reassign> {
        ui.label(RichText::new("Transcript").text_style(theme::subheading()));
        if detail.segments.is_empty() {
            ui.label(RichText::new("No transcript.").weak());
            return None;
        }
        let people = roster.people();
        let mut picked = None;
        for s in &detail.segments {
            let transcribed = SpeakerKey::transcribed(s.channel, s.speaker);
            let current =
                SpeakerKey::effective(s.channel, s.speaker, s.speaker_override.as_deref());
            let ink = theme::hue(ui.visuals(), theme::speaker_hue(roster.slot(current)));
            let (rule, body) = ui
                .horizontal(|ui| {
                    let (rule, _) = ui.allocate_exact_size(
                        Vec2::new(theme::SPEAKER_RULE_WIDTH, ui.spacing().interact_size.y),
                        Sense::hover(),
                    );
                    let body = ui
                        .horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(format_timestamp(s.start_ms.max(0) as u64))
                                    .small()
                                    .weak(),
                            );
                            if let Some(choice) =
                                self.menu(ui, s.id, roster, &people, current, transcribed)
                            {
                                picked = Some(choice);
                            }
                            if current != transcribed {
                                ui.label(theme::icon_text(theme::icons::ARROWS_LEFT_RIGHT).weak())
                                    .on_hover_text(format!(
                                        "Moved from {}",
                                        roster.label(transcribed)
                                    ));
                            }
                            ui.add(egui::Label::new(&s.text).selectable(true));
                        })
                        .response
                        .rect;
                    (rule, body)
                })
                .inner;
            ui.painter().rect_filled(
                Rect::from_x_y_ranges(rule.x_range(), body.y_range()),
                1.0,
                ink,
            );
        }
        picked
    }

    fn menu(
        &mut self,
        ui: &mut Ui,
        segment_id: i64,
        roster: &Roster,
        people: &[Person],
        current: SpeakerKey,
        transcribed: SpeakerKey,
    ) -> Option<Reassign> {
        let hue = theme::speaker_hue(roster.slot(current));
        let chip = theme::chip_button(
            ui.style(),
            hue,
            &roster.label(current),
            Some(theme::icons::CARET_DOWN),
        );
        let mut picked = None;
        let (_, open) = MenuButton::from_button(chip)
            .config(MenuConfig::new().close_behavior(PopupCloseBehavior::CloseOnClickOutside))
            .ui(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(("speaker-menu", segment_id))
                    .max_height(theme::SPEAKER_MENU_HEIGHT)
                    .show(ui, |ui| {
                        for person in people {
                            let selected = person.keys.contains(&current);
                            let clicked = ui
                                .horizontal(|ui| {
                                    theme::swatch(ui, theme::speaker_hue(person.slot));
                                    ui.selectable_label(selected, &person.label).clicked()
                                })
                                .inner;
                            if clicked {
                                if let Some(key) = choose(person, current, transcribed) {
                                    picked = Some(Reassign::Move { segment_id, key });
                                }
                                ui.close();
                            }
                        }
                    });
                ui.separator();
                if let Some(label) = as_transcribed(roster, current, transcribed) {
                    if ui.button(label).clicked() {
                        picked = Some(Reassign::Move {
                            segment_id,
                            key: None,
                        });
                        ui.close();
                    }
                }
                match &mut self.naming {
                    Some((line, buf)) if *line == segment_id => {
                        let r = ui.add(
                            TextEdit::singleline(buf)
                                .hint_text("Name (optional)")
                                .char_limit(super::speakers::NAME_MAX_CHARS)
                                .desired_width(160.0),
                        );
                        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if !r.lost_focus() && !r.has_focus() {
                            r.request_focus();
                        }
                        if enter || ui.button("Move line").clicked() {
                            picked = Some(Reassign::New {
                                segment_id,
                                key: roster.next_added(),
                                name: buf.trim().to_string(),
                            });
                            ui.close();
                        }
                    }
                    _ => {
                        let new = egui::Button::new(theme::icon_label_job(
                            ui.style(),
                            theme::icons::PLUS,
                            "New speaker…",
                        ));
                        if ui.add(new).clicked() {
                            self.naming = Some((segment_id, String::new()));
                        }
                    }
                }
            });
        let closed = open.is_none();
        if picked.is_some()
            || (closed && self.naming.as_ref().is_some_and(|(l, _)| *l == segment_id))
        {
            self.naming = None;
        }
        picked
    }
}

/// The override a menu pick stores: `None` inside means "as transcribed"
/// (choosing the line's own speaker stores no duplicate), and `None` outside
/// means the line already shows that person.
fn choose(
    person: &Person,
    current: SpeakerKey,
    transcribed: SpeakerKey,
) -> Option<Option<SpeakerKey>> {
    if person.keys.contains(&current) {
        None
    } else if person.keys.contains(&transcribed) {
        Some(None)
    } else {
        Some(Some(person.key))
    }
}

/// "As transcribed (Speaker 2)", only for a line that was moved.
fn as_transcribed(roster: &Roster, current: SpeakerKey, transcribed: SpeakerKey) -> Option<String> {
    (current != transcribed).then(|| format!("As transcribed ({})", roster.label(transcribed)))
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    fn roster() -> Roster {
        Roster::new(
            [
                (0, None, None),
                (1, Some(0), None),
                (1, Some(1), Some("me")),
                (1, Some(2), None),
            ],
            [("them:2", "Sam"), ("them:0", "sam")],
        )
    }

    fn person(r: &Roster, key: SpeakerKey) -> Person {
        r.people()
            .into_iter()
            .find(|p| p.keys.contains(&key))
            .unwrap()
    }

    #[test]
    fn the_menu_lists_me_then_people_in_slot_order_merged_by_name() {
        let labels: Vec<String> = roster().people().into_iter().map(|p| p.label).collect();
        assert_eq!(labels, ["Me", "sam", "Speaker 2"]);
    }

    #[test]
    fn choosing_the_transcribed_speaker_clears_the_override() {
        let r = roster();
        let me = SpeakerKey::Me;
        let s1 = SpeakerKey::Diarized(1);
        // Line transcribed as speaker 1, moved to Me: back to speaker 1.
        assert_eq!(choose(&person(&r, s1), me, s1), Some(None));
        // Moved to someone else: stored as their key.
        assert_eq!(
            choose(&person(&r, SpeakerKey::Diarized(0)), me, s1),
            Some(Some(SpeakerKey::Diarized(0)))
        );
        // Already that person: nothing to send.
        assert_eq!(choose(&person(&r, me), me, s1), None);
        // A merged person that includes the transcribed key also clears it.
        let s2 = SpeakerKey::Diarized(2);
        assert_eq!(
            choose(&person(&r, SpeakerKey::Diarized(0)), me, s2),
            Some(None)
        );
    }

    #[test]
    fn as_transcribed_shows_only_for_a_moved_line() {
        let r = roster();
        let s1 = SpeakerKey::Diarized(1);
        assert_eq!(
            as_transcribed(&r, SpeakerKey::Me, s1).as_deref(),
            Some("As transcribed (Speaker 2)")
        );
        assert_eq!(as_transcribed(&r, s1, s1), None);
    }

    /// Me, three far-side speakers, one line moved to Me and one override
    /// this build does not know.
    pub(in crate::ui::meetings) fn detail() -> MeetingDetail {
        let mut store = hark_store::Store::open_in_memory().unwrap();
        store
            .create_meeting(&hark_store::NewMeeting {
                id: "m".into(),
                started_ms: 0,
                trigger: "manual".into(),
                app_hint: None,
                stt_provider: "deepgram".into(),
            })
            .unwrap();
        for (i, (channel, speaker)) in [(0, None), (1, Some(0)), (1, Some(1)), (1, Some(2))]
            .into_iter()
            .enumerate()
        {
            let at = i as i64 * 1_000;
            let line = hark_store::MeetingSegment {
                start_ms: at,
                end_ms: at + 900,
                channel,
                speaker,
                text: format!("line {i}"),
            };
            store.append_meeting_segment("m", &line).unwrap();
        }
        let ids: Vec<i64> = store
            .meeting("m")
            .unwrap()
            .unwrap()
            .segments
            .iter()
            .map(|s| s.id)
            .collect();
        store
            .assign_segment_speaker("m", ids[2], Some("me"), 1)
            .unwrap();
        store
            .assign_segment_speaker("m", ids[3], Some("bogus"), 1)
            .unwrap();
        store.meeting("m").unwrap().unwrap()
    }

    #[test]
    fn the_transcript_renders_names_beside_every_rule_in_every_palette() {
        let detail = detail();
        let roster = crate::storage::meetings::roster_of(&detail);
        for appearance in crate::theme::Appearance::ALL {
            let ctx = egui::Context::default();
            crate::theme::apply(&ctx);
            crate::theme::set_appearance(&ctx, appearance);
            let mut view = TranscriptView::new();
            let mut output = ctx.run_ui(Default::default(), |ui| {
                assert!(view.show(ui, &detail, &roster).is_none());
            });
            output.textures_delta.clear();
            let texts: Vec<String> = output
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                    _ => None,
                })
                .collect();
            // Every line keeps a visible name; the moved one says so.
            let chips = texts
                .iter()
                .filter(|t| t.contains(theme::icons::CARET_DOWN))
                .count();
            assert_eq!(chips, 4, "{texts:?}");
            assert!(texts
                .iter()
                .any(|t| t.contains(theme::icons::ARROWS_LEFT_RIGHT)));
            assert!(texts.iter().any(|t| t.starts_with("Me")));
        }
    }
}

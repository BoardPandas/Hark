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
    /// Each line's height when last drawn (0: never drawn), at `width`.
    /// Lines outside the visible area take their space from here instead of
    /// being laid out: a 3 h meeting is ~2,500 wrapped lines, and laying out
    /// all of them every frame cost ~75 ms (debug build). Rows vary in height,
    /// so this is a measured cache, not `show_rows`.
    heights: Vec<f32>,
    width: f32,
}

impl TranscriptView {
    pub fn new() -> Self {
        TranscriptView {
            naming: None,
            heights: Vec::new(),
            width: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.naming = None;
        self.heights.clear();
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
        let width = ui.available_width();
        if self.heights.len() != detail.segments.len() || (self.width - width).abs() > 0.5 {
            self.heights = vec![0.0; detail.segments.len()];
            self.width = width;
        }
        let visible = ui.clip_rect();
        let estimate = ui.spacing().interact_size.y;
        for (i, s) in detail.segments.iter().enumerate() {
            let known = self.heights[i];
            let top = ui.cursor().top();
            let height = if known > 0.0 { known } else { estimate };
            if top > visible.bottom() || top + height < visible.top() {
                ui.allocate_space(Vec2::new(width, height));
                continue;
            }
            let transcribed = SpeakerKey::transcribed(s.channel, s.speaker);
            let current =
                SpeakerKey::effective(s.channel, s.speaker, s.speaker_override.as_deref());
            let ink = theme::hue(ui.visuals(), theme::speaker_hue(roster.slot(current)));
            let row = ui.horizontal(|ui| {
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
                                .on_hover_text(format!("Moved from {}", roster.label(transcribed)));
                        }
                        ui.add(egui::Label::new(&s.text).selectable(true));
                    })
                    .response
                    .rect;
                (rule, body)
            });
            let (rule, body) = row.inner;
            self.heights[i] = row.response.rect.height();
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
pub(super) mod tests;

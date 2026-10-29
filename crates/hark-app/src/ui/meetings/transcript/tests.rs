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
fn lines_outside_the_visible_area_keep_their_space_but_are_not_laid_out() {
    let mut detail = detail();
    let template = detail.segments.pop().unwrap();
    detail.segments = (0..400)
        .map(|i| hark_store::StoredSegment {
            id: i,
            start_ms: i * 1_000,
            text: format!("line {i}"),
            speaker_override: None,
            ..template_clone(&template)
        })
        .collect();
    let roster = crate::storage::meetings::roster_of(&detail);
    let ctx = egui::Context::default();
    crate::theme::apply(&ctx);
    let mut view = TranscriptView::new();
    let input = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800.0, 600.0),
        )),
        ..Default::default()
    };
    let mut drawn = Vec::new();
    for _ in 0..3 {
        let mut height = 0.0;
        let mut out = ctx.run_ui(input(), |ui| {
            height = egui::ScrollArea::vertical()
                .id_salt("test-transcript")
                .show(ui, |ui| view.show(ui, &detail, &roster))
                .content_size
                .y;
        });
        out.textures_delta.clear();
        let lines = out
            .shapes
            .iter()
            .filter(|c| matches!(&c.shape, egui::Shape::Text(t) if t.galley.text().starts_with("line ")))
            .count();
        drawn.push((lines, height));
    }
    let (lines, height) = drawn[2];
    assert!(lines > 0 && lines < 100, "{drawn:?}");
    assert!(
        height > 400.0 * 10.0,
        "every line keeps its space: {drawn:?}"
    );
}

fn template_clone(s: &hark_store::StoredSegment) -> hark_store::StoredSegment {
    hark_store::StoredSegment {
        id: s.id,
        start_ms: s.start_ms,
        end_ms: s.end_ms,
        channel: s.channel,
        speaker: s.speaker,
        speaker_override: s.speaker_override.clone(),
        text: s.text.clone(),
    }
}

fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

/// Tab reaches the first speaker chip, Enter opens its menu, Tab moves
/// into the menu's items, and Esc closes it: no mouse needed.
#[test]
fn the_speaker_menu_works_from_the_keyboard() {
    let detail = detail();
    let roster = crate::storage::meetings::roster_of(&detail);
    let ctx = egui::Context::default();
    crate::theme::apply(&ctx);
    let mut view = TranscriptView::new();
    let mut frame = |events: Vec<egui::Event>| {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        ctx.run_ui(input, |ui| {
            view.show(ui, &detail, &roster);
        })
        .textures_delta
        .clear();
        (egui::Popup::is_any_open(&ctx), ctx.memory(|m| m.focused()))
    };
    assert_eq!(frame(Vec::new()), (false, None));
    let (open, chip) = frame(vec![key(egui::Key::Tab)]);
    assert!(!open && chip.is_some(), "Tab focuses the first chip");
    frame(vec![key(egui::Key::Enter)]);
    let (open, _) = frame(Vec::new());
    assert!(open, "Enter on a focused chip opens its menu");
    let (open, inside) = frame(vec![key(egui::Key::Tab)]);
    assert!(
        open && inside.is_some() && inside != chip,
        "Tab moves into the menu"
    );
    frame(vec![key(egui::Key::Escape)]);
    let (open, _) = frame(Vec::new());
    assert!(!open, "Esc closes the menu");
}

/// A Gemini final pass numbers speakers per five-minute window: six
/// windows of three speakers is eighteen keys, all of which must stay
/// distinct by name, never Blue, and listed once each.
#[test]
fn six_gemini_windows_of_three_speakers_stay_usable() {
    let mut detail = detail();
    let template = template_clone(&detail.segments[1]);
    detail.segments = (0..18u32)
        .map(|n| {
            let (window, speaker) = (n / 3, n % 3);
            hark_store::StoredSegment {
                id: i64::from(n),
                start_ms: i64::from(n) * 100_000,
                speaker: Some(0x8000_0000 | (window << 10) | speaker),
                text: format!("window line {n}"),
                ..template_clone(&template)
            }
        })
        .collect();
    let roster = crate::storage::meetings::roster_of(&detail);
    let people = roster.people();
    assert_eq!(people.len(), 19, "Me plus eighteen window speakers");
    let mut labels: Vec<&str> = people.iter().map(|p| p.label.as_str()).collect();
    labels.sort_unstable();
    labels.dedup();
    assert_eq!(labels.len(), 19, "every label is distinct");
    assert!(people
        .iter()
        .skip(1)
        .all(|p| theme::speaker_hue(p.slot) != theme::Hue::Blue));
    let ctx = egui::Context::default();
    crate::theme::apply(&ctx);
    let mut view = TranscriptView::new();
    let mut card = super::super::speakers::SpeakersCard::new();
    let mut out = ctx.run_ui(Default::default(), |ui| {
        card.show(ui, &roster, &detail.speakers);
        view.show(ui, &detail, &roster);
    });
    out.textures_delta.clear();
    let texts: Vec<String> = out
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Text(t) => Some(t.galley.text().to_string()),
            _ => None,
        })
        .collect();
    assert!(
        texts.iter().any(|t| t.contains("Window 6 · Speaker 3")),
        "{texts:?}"
    );
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

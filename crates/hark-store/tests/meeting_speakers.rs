//! Migration 007 and the speaker store API: names keyed by speaker key,
//! per-line speaker overrides, and the notes-stale stamp (spec
//! `intent/meeting-speakers-are-hard-to-tell-apart/spec.md` §4, §5.1). File
//! databases where a second connection or a reopen is the point, in-memory
//! otherwise. No test formats a meeting type with `{:?}`.

use hark_store::{MeetingDetail, MeetingSegment, NameSource, NewMeeting, SpeakerName, Store};

fn meeting(id: &str) -> NewMeeting {
    NewMeeting {
        id: id.to_string(),
        started_ms: 1_000,
        trigger: "manual".to_string(),
        app_hint: None,
        stt_provider: "deepgram".to_string(),
    }
}

fn segment(start_ms: i64, channel: u8, speaker: Option<u32>, text: &str) -> MeetingSegment {
    MeetingSegment {
        start_ms,
        end_ms: start_ms + 500,
        channel,
        speaker,
        text: text.to_string(),
    }
}

fn name(key: &str, display_name: &str, source: NameSource) -> SpeakerName {
    SpeakerName {
        key: key.to_string(),
        display_name: display_name.to_string(),
        source,
        evidence_ms: (source == NameSource::Guess).then_some(12_000),
    }
}

/// `(key, name, is_guess, evidence_ms)` for every name row.
fn names(detail: &MeetingDetail) -> Vec<(String, String, bool, Option<i64>)> {
    detail
        .speakers
        .iter()
        .map(|n| {
            (
                n.key.clone(),
                n.display_name.clone(),
                n.source == NameSource::Guess,
                n.evidence_ms,
            )
        })
        .collect()
}

fn row(
    key: &str,
    name: &str,
    guess: bool,
    evidence: Option<i64>,
) -> (String, String, bool, Option<i64>) {
    (key.to_string(), name.to_string(), guess, evidence)
}

fn detail(store: &Store, id: &str) -> MeetingDetail {
    store.meeting(id).unwrap().unwrap()
}

/// A meeting with three lines: Me, then speakers 0 and 1 on the far side.
fn three_lines(store: &mut Store, id: &str) -> Vec<i64> {
    store.create_meeting(&meeting(id)).unwrap();
    for s in [
        segment(0, 0, None, "hello"),
        segment(1_000, 1, Some(0), "hi"),
        segment(2_000, 1, Some(1), "hey"),
    ] {
        store.append_meeting_segment(id, &s).unwrap();
    }
    detail(store, id).segments.iter().map(|s| s.id).collect()
}

#[test]
fn migration_007_copies_renames_as_user_names_and_keeps_the_old_table() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("hark.db");
    let gemini: i64 = 0x8000_0000 | (2 << 10) | 1;

    // A database exactly as migration 006 left it, with renames in it.
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        for sql in [
            include_str!("../migrations/001_init.sql"),
            include_str!("../migrations/002_stats_total_ms.sql"),
            include_str!("../migrations/003_entries_invocation.sql"),
            include_str!("../migrations/004_meetings.sql"),
            include_str!("../migrations/005_insight_events.sql"),
            include_str!("../migrations/006_meeting_transcript_status.sql"),
        ] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 6).unwrap();
        conn.execute_batch(&format!(
            "INSERT INTO meetings (id, started_ms, trigger, stt_provider, notes_json, refined) \
             VALUES ('old', 1000, 'manual', 'deepgram', '{{}}', 1), \
                    ('quiet', 2000, 'manual', 'deepgram', NULL, 0); \
             INSERT INTO meeting_segments (meeting_id, start_ms, end_ms, channel, speaker, text) \
             VALUES ('old', 0, 10, 0, NULL, 'a'), ('old', 10, 20, 1, 2, 'b'), \
                    ('old', 20, 30, 1, {gemini}, 'c'); \
             INSERT INTO meeting_speakers (meeting_id, speaker, display_name) \
             VALUES ('old', 2, 'Dana'), ('old', {gemini}, 'Sam'), ('quiet', 0, 'Pat');"
        ))
        .unwrap();
        // The fixture really stores integers (TEXT affinity would hide a bad
        // key format behind a string that only looks right).
        let kind: String = conn
            .query_row(
                "SELECT typeof(speaker) FROM meeting_speakers WHERE display_name = 'Sam'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(kind, "integer");
    }

    let store = Store::open(&db_path).unwrap();
    let old = detail(&store, "old");
    assert_eq!(
        names(&old),
        [
            row("them:2", "Dana", false, None),
            row(&format!("them:{gemini}"), "Sam", false, None),
        ]
    );
    assert!(old.segments.iter().all(|s| s.speaker_override.is_none()));
    assert_eq!(old.notes_stale_ms, None);
    assert_eq!(old.summary.segment_count, 3);
    // A dormant meeting nobody opens migrates too.
    assert_eq!(
        names(&detail(&store, "quiet")),
        [row("them:0", "Pat", false, None)]
    );
    drop(store);

    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 7);
    let types: (String, String, String) = conn
        .query_row(
            "SELECT typeof(speaker_key), typeof(source), typeof(evidence_ms) \
             FROM meeting_speaker_names WHERE display_name = 'Sam'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(types, ("text".into(), "text".into(), "null".into()));
    // What a build rolled back to 006 reads still works on a 007 database.
    let kept: Vec<(i64, String)> = conn
        .prepare("SELECT speaker, display_name FROM meeting_speakers WHERE meeting_id = 'old' ORDER BY speaker")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(kept, [(2, "Dana".to_string()), (gemini, "Sam".to_string())]);
    let old_query: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM (SELECT start_ms, end_ms, channel, speaker, text \
             FROM meeting_segments WHERE meeting_id = 'old' ORDER BY start_ms, channel)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(old_query, 3);
    drop(conn);

    // Reopening does not run 007 twice (CREATE TABLE would fail if it did).
    Store::open(&db_path).unwrap();
}

#[test]
fn a_name_is_set_replaced_and_cleared_per_key() {
    let mut store = Store::open_in_memory().unwrap();
    three_lines(&mut store, "m");
    store.set_speaker_name("m", "me", "  Chaz ").unwrap();
    store.set_speaker_name("m", "them:1", "Dana").unwrap();
    store.set_speaker_name("m", "added:0", "Pat").unwrap();
    store.set_speaker_name("m", "them:1", "Dana R").unwrap();
    assert_eq!(
        names(&detail(&store, "m")),
        [
            row("added:0", "Pat", false, None),
            row("me", "Chaz", false, None),
            row("them:1", "Dana R", false, None),
        ]
    );
    store.set_speaker_name("m", "me", "   ").unwrap();
    assert_eq!(detail(&store, "m").speakers.len(), 2);
    // Two people with the same name stay two rows: keyed by key.
    store.set_speaker_name("m", "them:0", "Dana R").unwrap();
    assert_eq!(detail(&store, "m").speakers.len(), 3);
    // An unknown meeting is a no-op, not a foreign-key error.
    store.set_speaker_name("missing", "me", "X").unwrap();
    assert!(store.meeting("missing").unwrap().is_none());
}

#[test]
fn a_guess_never_overwrites_a_user_name_and_does_overwrite_a_guess() {
    let mut store = Store::open_in_memory().unwrap();
    three_lines(&mut store, "m");
    store.set_speaker_name("m", "them:0", "Dana").unwrap();
    let written = store
        .apply_speaker_names(
            "m",
            &[
                name("them:0", "Sam", NameSource::Guess),
                name("them:1", "Lee", NameSource::Guess),
                name("me", " ", NameSource::User),
            ],
        )
        .unwrap();
    assert_eq!(written, 1);
    assert_eq!(
        names(&detail(&store, "m")),
        [
            row("them:0", "Dana", false, None),
            row("them:1", "Lee", true, Some(12_000)),
        ]
    );

    // A later guess replaces a guess; "Your name" (a user row) replaces a
    // guess too, but never a user's own name.
    store.set_speaker_name("m", "me", "Chaz").unwrap();
    store
        .apply_speaker_names(
            "m",
            &[
                name("them:1", "Leigh", NameSource::Guess),
                name("me", "Charles", NameSource::User),
            ],
        )
        .unwrap();
    assert_eq!(
        names(&detail(&store, "m")),
        [
            row("me", "Chaz", false, None),
            row("them:0", "Dana", false, None),
            row("them:1", "Leigh", true, Some(12_000)),
        ]
    );
    store
        .apply_speaker_names("m", &[name("them:1", "Lee", NameSource::User)])
        .unwrap();
    assert_eq!(
        names(&detail(&store, "m"))[2],
        row("them:1", "Lee", false, None)
    );

    // Saving a guessed name as the user confirms it: the marker goes.
    store
        .apply_speaker_names("m", &[name("added:0", "Pat", NameSource::Guess)])
        .unwrap();
    store.set_speaker_name("m", "added:0", "Pat").unwrap();
    assert_eq!(
        names(&detail(&store, "m"))[0],
        row("added:0", "Pat", false, None)
    );

    assert_eq!(
        store
            .apply_speaker_names("missing", &[name("me", "X", NameSource::User)])
            .unwrap(),
        0
    );
}

#[test]
fn an_unknown_source_reads_as_a_user_name() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hark.db");
    let mut store = Store::open(&path).unwrap();
    three_lines(&mut store, "m");
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "INSERT INTO meeting_speaker_names (meeting_id, speaker_key, display_name, source) \
             VALUES ('m', 'them:0', 'Dana', 'imported')",
            [],
        )
        .unwrap();
    assert!(detail(&store, "m").speakers[0].source == NameSource::User);
    store
        .apply_speaker_names("m", &[name("them:0", "Sam", NameSource::Guess)])
        .unwrap();
    assert_eq!(detail(&store, "m").speakers[0].display_name, "Dana");
}

#[test]
fn a_line_is_moved_and_moved_back_within_its_own_meeting_only() {
    let mut store = Store::open_in_memory().unwrap();
    let ids = three_lines(&mut store, "m");
    let other = three_lines(&mut store, "other");

    assert_eq!(
        store
            .assign_segment_speaker("m", ids[1], Some("them:1"), 5_000)
            .unwrap(),
        Some(false),
        "no notes, so nothing goes stale"
    );
    let d = detail(&store, "m");
    assert_eq!(d.segments[1].speaker_override.as_deref(), Some("them:1"));
    assert_eq!(
        d.segments[1].speaker,
        Some(0),
        "the transcribed speaker stays"
    );
    assert_eq!(d.notes_stale_ms, None);

    // A segment of another meeting: nothing changes anywhere.
    assert_eq!(
        store
            .assign_segment_speaker("m", other[2], Some("me"), 5_000)
            .unwrap(),
        None
    );
    assert!(detail(&store, "other")
        .segments
        .iter()
        .all(|s| s.speaker_override.is_none()));
    assert_eq!(
        store
            .assign_segment_speaker("missing", ids[1], None, 5_000)
            .unwrap(),
        None
    );

    store
        .assign_segment_speaker("m", ids[1], None, 6_000)
        .unwrap();
    assert!(detail(&store, "m").segments[1].speaker_override.is_none());
    // The update trigger re-indexed FTS without losing the line.
    assert_eq!(store.meetings(Some("hi")).unwrap().len(), 2);
}

#[test]
fn moving_a_line_stamps_notes_stale_and_only_an_older_refresh_clears_it() {
    let mut store = Store::open_in_memory().unwrap();
    let ids = three_lines(&mut store, "m");
    store.set_meeting_title("m", "Kept title").unwrap();
    store.set_meeting_notes("m", Some("{\"v\":1}")).unwrap();
    assert_eq!(
        store
            .assign_segment_speaker("m", ids[2], Some("me"), 5_000)
            .unwrap(),
        Some(true)
    );
    assert_eq!(detail(&store, "m").notes_stale_ms, Some(5_000));

    // A refresh requested before the latest move keeps the stamp.
    assert!(store.set_refreshed_notes("m", "{\"v\":2}", 4_999).unwrap());
    let d = detail(&store, "m");
    assert_eq!(d.notes_json.as_deref(), Some("{\"v\":2}"));
    assert_eq!(d.notes_stale_ms, Some(5_000));

    assert!(store.set_refreshed_notes("m", "{\"v\":3}", 5_000).unwrap());
    let d = detail(&store, "m");
    assert_eq!(d.notes_json.as_deref(), Some("{\"v\":3}"));
    assert_eq!(d.notes_stale_ms, None);
    assert_eq!(d.summary.title.as_deref(), Some("Kept title"));

    // Not stale at all: a refresh leaves it that way.
    assert!(store.set_refreshed_notes("m", "{\"v\":4}", 1).unwrap());
    assert_eq!(detail(&store, "m").notes_stale_ms, None);
    assert!(!store.set_refreshed_notes("missing", "{}", 1).unwrap());
}

#[test]
fn reprocessing_clears_names_overrides_and_the_stamp_and_bakes_the_notes() {
    let mut store = Store::open_in_memory().unwrap();
    let ids = three_lines(&mut store, "m");
    store
        .set_meeting_notes("m", Some("{\"tokens\":true}"))
        .unwrap();
    store.set_speaker_name("m", "them:0", "Dana").unwrap();
    store
        .assign_segment_speaker("m", ids[1], Some("added:0"), 5_000)
        .unwrap();
    store
        .reprocess_meeting_segments(
            "m",
            &[segment(0, 1, Some(3), "fresh")],
            Some("{\"baked\":true}"),
        )
        .unwrap();
    let d = detail(&store, "m");
    assert!(d.speakers.is_empty());
    assert_eq!(d.notes_stale_ms, None);
    assert_eq!(d.notes_json.as_deref(), Some("{\"baked\":true}"));
    assert_eq!(d.segments.len(), 1);
    assert!(d.segments[0].speaker_override.is_none());

    // No baked copy: the notes stay as they are.
    store
        .reprocess_meeting_segments("m", &[segment(0, 1, Some(3), "again")], None)
        .unwrap();
    assert_eq!(
        detail(&store, "m").notes_json.as_deref(),
        Some("{\"baked\":true}")
    );
}

#[test]
fn a_failed_reprocess_rolls_back_names_overrides_stamp_and_notes_together() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rollback.db");
    let mut store = Store::open(&path).unwrap();
    let ids = three_lines(&mut store, "m");
    store
        .set_meeting_notes("m", Some("{\"tokens\":true}"))
        .unwrap();
    store.set_speaker_name("m", "me", "Chaz").unwrap();
    store
        .assign_segment_speaker("m", ids[2], Some("me"), 5_000)
        .unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER reject_replacement BEFORE INSERT ON meeting_segments \
             BEGIN SELECT RAISE(ABORT, 'fixture failure'); END;",
        )
        .unwrap();
    assert!(store
        .reprocess_meeting_segments("m", &[segment(0, 1, None, "bad")], Some("{\"baked\":1}"))
        .is_err());
    let d = detail(&store, "m");
    assert_eq!(names(&d), [row("me", "Chaz", false, None)]);
    assert_eq!(d.notes_stale_ms, Some(5_000));
    assert_eq!(d.notes_json.as_deref(), Some("{\"tokens\":true}"));
    assert_eq!(d.segments.len(), 3);
    assert_eq!(d.segments[2].speaker_override.as_deref(), Some("me"));
}

#[test]
fn deleting_a_meeting_removes_its_names() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hark.db");
    let mut store = Store::open(&path).unwrap();
    three_lines(&mut store, "m");
    store.set_speaker_name("m", "me", "Chaz").unwrap();
    store
        .apply_speaker_names("m", &[name("them:0", "Sam", NameSource::Guess)])
        .unwrap();
    assert!(store.delete_meeting("m").unwrap());
    let left: i64 = rusqlite::Connection::open(&path)
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM meeting_speaker_names WHERE meeting_id = 'm'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(left, 0);
}

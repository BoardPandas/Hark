//! hark-store meeting behavior tests (migration 004), on in-memory
//! databases plus temp files where reopen/migration behavior is the point.
//! No test touches user data locations, and no test asserts on transcript
//! text via `{:?}` (the meeting types deliberately do not derive `Debug`).

use hark_store::{MeetingSegment, NewMeeting, Store};

fn meeting(id: &str, started_ms: i64) -> NewMeeting {
    NewMeeting {
        id: id.to_string(),
        started_ms,
        trigger: "manual".to_string(),
        app_hint: Some("ms-teams.exe".to_string()),
        stt_provider: "deepgram".to_string(),
    }
}

#[test]
fn reprocessing_resets_speaker_names_and_fts_but_preserves_notes_and_title() {
    let mut store = Store::open_in_memory().unwrap();
    store.create_meeting(&meeting("m", 0)).unwrap();
    store.set_meeting_title("m", "Kept title").unwrap();
    store
        .set_meeting_notes("m", Some("kept notes and checked actions"))
        .unwrap();
    store
        .append_meeting_segment("m", &segment(0, 100, 1, Some(0), "oldword"))
        .unwrap();
    store.rename_meeting_speaker("m", 0, "Old person").unwrap();
    store
        .reprocess_meeting_segments("m", &[segment(0, 120, 1, Some(0), "newword")])
        .unwrap();
    let detail = store.meeting("m").unwrap().unwrap();
    assert!(detail.summary.refined);
    assert!(detail.speakers.is_empty());
    assert_eq!(detail.summary.title.as_deref(), Some("Kept title"));
    assert_eq!(
        detail.notes_json.as_deref(),
        Some("kept notes and checked actions")
    );
    assert!(store.meetings(Some("oldword")).unwrap().is_empty());
    assert_eq!(store.meetings(Some("newword")).unwrap().len(), 1);
}

#[test]
fn failed_reprocessing_rolls_back_transcript_and_speaker_names() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rollback.db");
    let mut store = Store::open(&path).unwrap();
    store.create_meeting(&meeting("m", 0)).unwrap();
    store
        .append_meeting_segment("m", &segment(0, 100, 1, Some(0), "keepword"))
        .unwrap();
    store.rename_meeting_speaker("m", 0, "Kept person").unwrap();
    // Inject an insertion failure after replacement has deleted the old rows.
    // This exercises real SQLite rollback without changing the product schema.
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute_batch("CREATE TRIGGER reject_replacement BEFORE INSERT ON meeting_segments BEGIN SELECT RAISE(ABORT, 'fixture failure'); END;").unwrap();
    assert!(store
        .reprocess_meeting_segments("m", &[segment(0, 100, 1, Some(0), "badword")])
        .is_err());
    let detail = store.meeting("m").unwrap().unwrap();
    assert_eq!(detail.segments[0].text, "keepword");
    assert_eq!(detail.speakers[0].1, "Kept person");
    assert_eq!(store.meetings(Some("keepword")).unwrap().len(), 1);
    assert!(store.meetings(Some("badword")).unwrap().is_empty());
}

fn segment(
    start_ms: i64,
    end_ms: i64,
    channel: u8,
    speaker: Option<u32>,
    text: &str,
) -> MeetingSegment {
    MeetingSegment {
        start_ms,
        end_ms,
        channel,
        speaker,
        text: text.to_string(),
    }
}

#[test]
fn fts5_is_available_in_the_bundled_sqlite() {
    // Migration 004 creates an FTS5 virtual table unconditionally; if the
    // bundled SQLite ever stops compiling FTS5 in, this fails loudly at the
    // one place that matters instead of surfacing as "search returns
    // nothing" downstream.
    let conn = rusqlite::Connection::open_in_memory().expect("open");
    conn.execute_batch("CREATE VIRTUAL TABLE t USING fts5(x)")
        .expect("FTS5 must be compiled into the bundled SQLite hark-store links");
}

#[test]
fn create_append_finish_list_and_detail_round_trip() {
    let mut store = Store::open_in_memory().expect("open");
    store.create_meeting(&meeting("m1", 1_000)).expect("create");
    store
        .append_meeting_segment("m1", &segment(0, 500, 0, None, "hello there"))
        .expect("append");
    store
        .append_meeting_segment("m1", &segment(500, 1_000, 1, None, "hi back"))
        .expect("append");
    store.finish_meeting("m1", 5_000).expect("finish");

    let list = store.meetings(None).expect("list");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, "m1");
    assert_eq!(list[0].started_ms, 1_000);
    assert_eq!(list[0].ended_ms, Some(5_000));
    assert_eq!(list[0].trigger, "manual");
    assert_eq!(list[0].app_hint.as_deref(), Some("ms-teams.exe"));
    assert_eq!(list[0].audio_bytes, 0);
    assert_eq!(list[0].audio_evicted_ms, None);
    assert!(!list[0].has_notes);
    assert!(!list[0].refined);
    assert_eq!(list[0].segment_count, 2);

    let detail = store.meeting("m1").expect("detail").expect("exists");
    assert_eq!(detail.stt_provider, "deepgram");
    assert_eq!(detail.notes_json, None);
    assert_eq!(detail.segments.len(), 2);
    assert_eq!(detail.segments[0].text, "hello there");
    assert_eq!(detail.segments[1].text, "hi back");
    assert!(detail.speakers.is_empty());

    assert!(store.meeting("missing").expect("query").is_none());
}

#[test]
fn replace_segments_sets_refined_and_drops_old_rows_and_fts_hits() {
    let mut store = Store::open_in_memory().expect("open");
    store.create_meeting(&meeting("m1", 1_000)).expect("create");
    store
        .append_meeting_segment("m1", &segment(0, 500, 0, None, "unrefined zebra text"))
        .expect("append");

    let hits = store.meetings(Some("zebra")).expect("search before");
    assert_eq!(
        hits.len(),
        1,
        "live segment is searchable before the final pass"
    );

    store
        .replace_meeting_segments(
            "m1",
            &[
                segment(0, 400, 0, Some(1), "refined giraffe text"),
                segment(400, 900, 1, Some(2), "second speaker"),
            ],
            true,
        )
        .expect("replace");

    let detail = store.meeting("m1").expect("detail").expect("exists");
    assert!(detail.summary.refined);
    assert_eq!(detail.segments.len(), 2);
    assert_eq!(detail.segments[0].text, "refined giraffe text");
    assert_eq!(detail.segments[0].speaker, Some(1));

    assert!(
        store
            .meetings(Some("zebra"))
            .expect("search after")
            .is_empty(),
        "old segment text must drop out of the FTS index, not just the table"
    );
    assert_eq!(
        store.meetings(Some("giraffe")).expect("search new").len(),
        1
    );
}

#[test]
fn cascade_delete_removes_segments_and_speakers() {
    let mut store = Store::open_in_memory().expect("open");
    store.create_meeting(&meeting("m1", 1_000)).expect("create");
    store
        .append_meeting_segment("m1", &segment(0, 500, 0, Some(1), "one"))
        .expect("append");
    store
        .rename_meeting_speaker("m1", 1, "Dana")
        .expect("rename");

    assert!(store.delete_meeting("m1").expect("delete"));
    assert_eq!(store.meetings(None).expect("list").len(), 0);
    assert!(store.meeting("m1").expect("query").is_none());

    // Cascades must actually be enforced, not merely orphan rows the reader
    // never sees: reopen a fresh connection with foreign_keys pragma-only
    // behavior identical to Store's, using raw SQL to inspect the tables
    // Store does not expose directly.
    let conn = rusqlite::Connection::open_in_memory().expect("raw open");
    conn.execute_batch(include_str!("../migrations/001_init.sql"))
        .expect("001");
    conn.execute_batch(include_str!("../migrations/002_stats_total_ms.sql"))
        .expect("002");
    conn.execute_batch(include_str!("../migrations/003_entries_invocation.sql"))
        .expect("003");
    conn.execute_batch(include_str!("../migrations/004_meetings.sql"))
        .expect("004");
    conn.pragma_update(None, "foreign_keys", "ON")
        .expect("fk on");
    conn.execute(
        "INSERT INTO meetings (id, started_ms, trigger, stt_provider) VALUES ('m2', 1, 'manual', 'deepgram')",
        [],
    )
    .expect("seed meeting");
    conn.execute(
        "INSERT INTO meeting_segments (meeting_id, start_ms, end_ms, channel, text) \
         VALUES ('m2', 0, 1, 0, 'x')",
        [],
    )
    .expect("seed segment");
    conn.execute("DELETE FROM meetings WHERE id = 'm2'", [])
        .expect("delete meeting");
    let remaining: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM meeting_segments WHERE meeting_id = 'm2'",
            [],
            |r| r.get(0),
        )
        .expect("count");
    assert_eq!(remaining, 0, "ON DELETE CASCADE must remove child segments");
}

#[test]
fn set_and_clear_notes() {
    let mut store = Store::open_in_memory().expect("open");
    store.create_meeting(&meeting("m1", 1_000)).expect("create");

    store
        .set_meeting_notes("m1", Some("{\"summary\":\"ok\"}"))
        .expect("set notes");
    assert!(store.meetings(None).expect("list")[0].has_notes);
    let detail = store.meeting("m1").expect("detail").expect("exists");
    assert_eq!(detail.notes_json.as_deref(), Some("{\"summary\":\"ok\"}"));

    store.set_meeting_notes("m1", None).expect("clear notes");
    assert!(!store.meetings(None).expect("list")[0].has_notes);
    assert_eq!(
        store
            .meeting("m1")
            .expect("detail")
            .expect("exists")
            .notes_json,
        None
    );
}

#[test]
fn set_title_reports_whether_the_meeting_exists() {
    let mut store = Store::open_in_memory().expect("open");
    store.create_meeting(&meeting("m1", 1_000)).expect("create");

    assert!(store.set_meeting_title("m1", "Standup").expect("set"));
    assert_eq!(
        store.meetings(None).expect("list")[0].title.as_deref(),
        Some("Standup")
    );
    assert!(!store
        .set_meeting_title("missing", "x")
        .expect("set missing"));
}

#[test]
fn rename_and_clear_speaker() {
    let mut store = Store::open_in_memory().expect("open");
    store.create_meeting(&meeting("m1", 1_000)).expect("create");

    store
        .rename_meeting_speaker("m1", 2, "Dana")
        .expect("rename");
    let detail = store.meeting("m1").expect("detail").expect("exists");
    assert_eq!(detail.speakers, vec![(2, "Dana".to_string())]);

    // Renaming again overwrites, it does not duplicate the (meeting, speaker) key.
    store
        .rename_meeting_speaker("m1", 2, "Dana R")
        .expect("rename again");
    let detail = store.meeting("m1").expect("detail").expect("exists");
    assert_eq!(detail.speakers, vec![(2, "Dana R".to_string())]);

    // An empty (after trim) name removes the rename.
    store
        .rename_meeting_speaker("m1", 2, "   ")
        .expect("clear rename");
    let detail = store.meeting("m1").expect("detail").expect("exists");
    assert!(detail.speakers.is_empty());
}

#[test]
fn audio_bytes_and_eviction() {
    let mut store = Store::open_in_memory().expect("open");
    store.create_meeting(&meeting("m1", 1_000)).expect("create");
    store.finish_meeting("m1", 2_000).expect("finish");
    store.create_meeting(&meeting("m2", 1_500)).expect("create");
    store.finish_meeting("m2", 2_500).expect("finish");

    store
        .set_meeting_audio_bytes("m1", 12_345)
        .expect("set bytes");
    store.set_meeting_audio_bytes("m2", 999).expect("set bytes");

    let index = store.meeting_audio_index().expect("index");
    assert_eq!(index.len(), 2, "both meetings still have audio");

    store
        .mark_meeting_audio_evicted("m1", 9_999)
        .expect("evict");
    let summary = &store.meetings(None).expect("list")[1]; // m1 is older, listed second
    assert_eq!(summary.id, "m1");
    assert_eq!(summary.audio_bytes, 0, "eviction zeroes audio_bytes");
    assert_eq!(summary.audio_evicted_ms, Some(9_999));

    let index = store.meeting_audio_index().expect("index after evict");
    assert_eq!(index.len(), 1, "evicted meeting drops out of the index");
    assert_eq!(index[0].0, "m2");
}

#[test]
fn unfinished_meetings_are_ones_still_recording_or_orphaned_by_a_crash() {
    let mut store = Store::open_in_memory().expect("open");
    store.create_meeting(&meeting("m1", 1_000)).expect("create");
    store.create_meeting(&meeting("m2", 2_000)).expect("create");
    store.finish_meeting("m2", 3_000).expect("finish");

    let unfinished = store.unfinished_meetings().expect("unfinished");
    assert_eq!(unfinished, vec!["m1".to_string()]);
}

#[test]
fn search_matches_segment_word_or_title_case_insensitively() {
    let mut store = Store::open_in_memory().expect("open");
    store.create_meeting(&meeting("m1", 1_000)).expect("create");
    store
        .append_meeting_segment(
            "m1",
            &segment(0, 500, 0, None, "let's discuss the Modero rollout"),
        )
        .expect("append");
    store.create_meeting(&meeting("m2", 2_000)).expect("create");
    store
        .append_meeting_segment("m2", &segment(0, 500, 0, None, "unrelated content"))
        .expect("append");
    store
        .set_meeting_title("m2", "Weekly Standup")
        .expect("title");

    let by_word = store.meetings(Some("modero")).expect("search word");
    assert_eq!(by_word.len(), 1);
    assert_eq!(by_word[0].id, "m1");

    let by_title = store.meetings(Some("standup")).expect("search title");
    assert_eq!(by_title.len(), 1);
    assert_eq!(by_title[0].id, "m2");

    assert!(store
        .meetings(Some("nothing"))
        .expect("no match")
        .is_empty());
}

#[test]
fn search_never_errors_on_hostile_input() {
    let mut store = Store::open_in_memory().expect("open");
    store.create_meeting(&meeting("m1", 1_000)).expect("create");
    store
        .append_meeting_segment("m1", &segment(0, 500, 0, None, "plain content"))
        .expect("append");

    for hostile in [
        "\"unterminated quote",
        "AND",
        "OR NOT",
        "*",
        "(unbalanced",
        "unbalanced)",
        "col:match",
        "",
        "   ",
        "%_",
        "\\",
    ] {
        store
            .meetings(Some(hostile))
            .unwrap_or_else(|e| panic!("search {hostile:?} must not error: {e}"));
    }

    // Blank search means no filter, same as the dictation history search.
    assert_eq!(store.meetings(Some("   ")).expect("blank").len(), 1);
}

#[test]
fn meetings_list_is_newest_first() {
    let mut store = Store::open_in_memory().expect("open");
    for (id, started) in [("m1", 1_000), ("m2", 3_000), ("m3", 2_000)] {
        store.create_meeting(&meeting(id, started)).expect("create");
    }

    let list = store.meetings(None).expect("list");
    let ids: Vec<&str> = list.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, vec!["m2", "m3", "m1"]);
}

#[test]
fn migration_004_applies_to_a_003_era_database_with_data_intact() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("hark.db");

    // Build a database exactly as migration 003 left it: 001..003 applied
    // in order, user_version = 3, and a dictation row already in it.
    {
        let conn = rusqlite::Connection::open(&db_path).expect("raw open");
        conn.execute_batch(include_str!("../migrations/001_init.sql"))
            .expect("apply 001");
        conn.execute_batch(include_str!("../migrations/002_stats_total_ms.sql"))
            .expect("apply 002");
        conn.execute_batch(include_str!("../migrations/003_entries_invocation.sql"))
            .expect("apply 003");
        conn.pragma_update(None, "user_version", 3).expect("stamp");
        conn.execute(
            "INSERT INTO stats (id, dictations, words, since_ts_ms) VALUES (1, 7, 40, 5_000)",
            [],
        )
        .expect("seed stats");
        conn.execute(
            "INSERT INTO entries (ts_ms, raw_text, final_text, voice, stt_provider, \
             stt_model, stt_ms, total_ms) VALUES (1, 'r', 'f', 'clean', 'deepgram', \
             'nova-3', 100, 200)",
            [],
        )
        .expect("seed entry");
    }

    let mut store = Store::open(&db_path).expect("open runs migration 004");

    // Pre-004 dictation data survives untouched.
    let entries = store.entries(None, 10, 0).expect("entries");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].final_text, "f");
    let stats = store.stats().expect("stats");
    assert_eq!(stats.dictations, 7, "003-era counters survive the upgrade");
    assert_eq!(stats.words, 40);

    // The new meeting tables work on the upgraded database.
    store
        .create_meeting(&meeting("m1", 9_000))
        .expect("create after migration");
    store
        .append_meeting_segment("m1", &segment(0, 100, 0, None, "post-migration text"))
        .expect("append after migration");
    let list = store.meetings(None).expect("list after migration");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].segment_count, 1);
}

//! hark-store behavior tests on in-memory databases (plus temp files where
//! reopen/WAL behavior is the point). No test touches user data locations.

use hark_store::{NewDictation, Retention, Store};

/// A dictation with distinguishable content and timings.
fn dictation(ts_ms: i64, raw: &str, fin: &str) -> NewDictation {
    NewDictation {
        ts_ms,
        raw_text: raw.to_string(),
        final_text: fin.to_string(),
        voice: "clean".to_string(),
        stt_provider: "deepgram".to_string(),
        stt_model: "nova-3".to_string(),
        cleanup_model: Some("gpt-5-nano".to_string()),
        audio_ms: 1_500,
        stt_ms: 400,
        cleanup_ms: Some(300),
        total_ms: 800,
    }
}

const KEEP_ALL: Retention = Retention {
    max_entries: 1_000,
    max_age_days: 90,
};

#[test]
fn record_with_capture_stores_entry_and_ticks_stats() {
    let mut store = Store::open_in_memory().expect("open");
    store
        .record(
            &dictation(1_000, "raw words here", "three final words"),
            true,
        )
        .expect("record");

    let entries = store.entries(None, 10, 0).expect("entries");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].ts_ms, 1_000);
    assert_eq!(entries[0].raw_text, "raw words here");
    assert_eq!(entries[0].final_text, "three final words");
    assert_eq!(entries[0].voice, "clean");
    assert_eq!(entries[0].stt_provider, "deepgram");
    assert_eq!(entries[0].stt_model, "nova-3");
    assert_eq!(entries[0].cleanup_model.as_deref(), Some("gpt-5-nano"));
    assert_eq!(entries[0].stt_ms, 400);
    assert_eq!(entries[0].cleanup_ms, Some(300));
    assert_eq!(entries[0].total_ms, 800);

    let stats = store.stats().expect("stats");
    assert_eq!(stats.dictations, 1);
    assert_eq!(stats.words, 3, "words count the final text");
    assert_eq!(stats.audio_ms, 1_500);
    assert_eq!(stats.stt_ms, 400);
    assert_eq!(stats.cleanup_ms, 300);
    assert_eq!(stats.total_ms, 800, "release-to-inject sum (migration 002)");
    assert!(stats.since_ts_ms > 0, "since date seeded at open");
}

#[test]
fn capture_off_ticks_stats_but_stores_no_content() {
    let mut store = Store::open_in_memory().expect("open");
    store
        .record(&dictation(1_000, "private words", "private words"), false)
        .expect("record");

    assert_eq!(store.entry_count(None).expect("count"), 0);
    assert!(store.entries(None, 10, 0).expect("entries").is_empty());

    let stats = store.stats().expect("stats");
    assert_eq!(stats.dictations, 1, "counters still tick with capture off");
    assert_eq!(stats.words, 2);
}

#[test]
fn no_cleanup_fields_store_null_and_add_zero() {
    let mut store = Store::open_in_memory().expect("open");
    let mut d = dictation(1_000, "verbatim text", "verbatim text");
    d.cleanup_model = None;
    d.cleanup_ms = None;
    store.record(&d, true).expect("record");

    let entries = store.entries(None, 10, 0).expect("entries");
    assert_eq!(entries[0].cleanup_model, None);
    assert_eq!(entries[0].cleanup_ms, None);
    assert_eq!(store.stats().expect("stats").cleanup_ms, 0);
}

#[test]
fn entries_page_newest_first() {
    let mut store = Store::open_in_memory().expect("open");
    for i in 0..5 {
        store
            .record(&dictation(1_000 + i, "raw", &format!("text {i}")), true)
            .expect("record");
    }

    let page = store.entries(None, 2, 0).expect("page 1");
    assert_eq!(page.len(), 2);
    assert_eq!(page[0].final_text, "text 4", "newest first");
    assert_eq!(page[1].final_text, "text 3");

    let page = store.entries(None, 2, 2).expect("page 2");
    assert_eq!(page[0].final_text, "text 2");
    assert_eq!(page[1].final_text, "text 1");

    let page = store.entries(None, 2, 4).expect("page 3");
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].final_text, "text 0");
}

#[test]
fn search_matches_raw_and_final_case_insensitively() {
    let mut store = Store::open_in_memory().expect("open");
    store
        .record(&dictation(1, "umm Modero thing", "the Modero thing"), true)
        .expect("record");
    store
        .record(&dictation(2, "raw only ZEBRA", "cleaned away"), true)
        .expect("record");
    store
        .record(&dictation(3, "unrelated", "unrelated"), true)
        .expect("record");

    let hits = store.entries(Some("modero"), 10, 0).expect("search final");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].ts_ms, 1);

    let hits = store.entries(Some("zebra"), 10, 0).expect("search raw");
    assert_eq!(hits.len(), 1, "raw text is searched too");
    assert_eq!(hits[0].ts_ms, 2);

    assert_eq!(store.entry_count(Some("unrelated")).expect("count"), 1);
    assert_eq!(store.entry_count(Some("nothing")).expect("count"), 0);
    assert!(store
        .entries(Some("nothing"), 10, 0)
        .expect("none")
        .is_empty());

    // Blank search means no filter.
    assert_eq!(store.entry_count(Some("  ")).expect("blank"), 3);
}

#[test]
fn search_treats_like_wildcards_literally() {
    let mut store = Store::open_in_memory().expect("open");
    store
        .record(&dictation(1, "raw", "sale is 100% off"), true)
        .expect("record");
    store
        .record(&dictation(2, "raw", "salexoff has no percent"), true)
        .expect("record");

    let hits = store.entries(Some("100% off"), 10, 0).expect("search");
    assert_eq!(hits.len(), 1, "% must not act as a wildcard");
    assert_eq!(hits[0].ts_ms, 1);

    assert_eq!(store.entry_count(Some("e_o")).expect("count"), 0);
}

#[test]
fn delete_entry_removes_one_row() {
    let mut store = Store::open_in_memory().expect("open");
    store.record(&dictation(1, "a", "a"), true).expect("record");
    store.record(&dictation(2, "b", "b"), true).expect("record");
    let id = store.entries(None, 1, 0).expect("newest")[0].id;

    assert!(store.delete_entry(id).expect("delete"));
    assert_eq!(store.entry_count(None).expect("count"), 1);
    assert!(
        !store.delete_entry(id).expect("re-delete"),
        "deleting a missing id reports false, not an error"
    );
}

#[test]
fn clear_history_leaves_stats_untouched() {
    let mut store = Store::open_in_memory().expect("open");
    for i in 0..3 {
        store
            .record(&dictation(i, "raw", "two words"), true)
            .expect("record");
    }

    let cleared = store.clear_entries().expect("clear");
    assert_eq!(cleared, 3);
    assert_eq!(store.entry_count(None).expect("count"), 0);

    let stats = store.stats().expect("stats");
    assert_eq!(
        stats.dictations, 3,
        "lifetime stats survive a history clear"
    );
    assert_eq!(stats.words, 6);
}

#[test]
fn reset_stats_leaves_history_untouched() {
    let mut store = Store::open_in_memory().expect("open");
    for i in 0..3 {
        store
            .record(&dictation(i, "raw", "words"), true)
            .expect("record");
    }

    store.reset_stats(42_000).expect("reset");

    let stats = store.stats().expect("stats");
    assert_eq!(stats.dictations, 0);
    assert_eq!(stats.words, 0);
    assert_eq!(stats.audio_ms, 0);
    assert_eq!(stats.stt_ms, 0);
    assert_eq!(stats.cleanup_ms, 0);
    assert_eq!(stats.total_ms, 0);
    assert_eq!(stats.since_ts_ms, 42_000, "since date restarts at reset");

    assert_eq!(
        store.entry_count(None).expect("count"),
        3,
        "history survives a stats reset"
    );
}

#[test]
fn prune_by_age_is_a_strict_boundary() {
    let mut store = Store::open_in_memory().expect("open");
    let now_ms = 100 * 86_400_000; // day 100
    let cutoff = now_ms - 90 * 86_400_000; // 90-day policy => day 10

    store
        .record(&dictation(cutoff - 1, "old", "old"), true)
        .expect("record");
    store
        .record(&dictation(cutoff, "edge", "edge"), true)
        .expect("record");
    store
        .record(&dictation(now_ms, "new", "new"), true)
        .expect("record");

    let pruned = store.prune(KEEP_ALL, now_ms).expect("prune");
    assert_eq!(pruned, 1, "only the strictly-older row goes");

    let kept = store.entries(None, 10, 0).expect("entries");
    assert_eq!(kept.len(), 2);
    assert_eq!(kept[0].final_text, "new");
    assert_eq!(
        kept[1].final_text, "edge",
        "row exactly at the cutoff stays"
    );
}

#[test]
fn prune_by_count_keeps_the_newest() {
    let mut store = Store::open_in_memory().expect("open");
    for i in 0..10 {
        store
            .record(&dictation(1_000 + i, "raw", &format!("text {i}")), true)
            .expect("record");
    }

    let policy = Retention {
        max_entries: 3,
        max_age_days: 90,
    };
    let pruned = store.prune(policy, 2_000).expect("prune");
    assert_eq!(pruned, 7);

    let kept = store.entries(None, 10, 0).expect("entries");
    assert_eq!(kept.len(), 3);
    assert_eq!(kept[0].final_text, "text 9");
    assert_eq!(kept[2].final_text, "text 7");
}

#[test]
fn prune_applies_both_rules_and_stats_survive() {
    let mut store = Store::open_in_memory().expect("open");
    let now_ms = 100 * 86_400_000;
    let cutoff = now_ms - 90 * 86_400_000;

    // Two too old, four recent (one over the count cap).
    for i in 0..2 {
        store
            .record(&dictation(cutoff - 10 + i, "old", "old"), true)
            .expect("record");
    }
    for i in 0..4 {
        store
            .record(
                &dictation(now_ms - 100 + i, "raw", &format!("recent {i}")),
                true,
            )
            .expect("record");
    }

    let policy = Retention {
        max_entries: 3,
        max_age_days: 90,
    };
    let pruned = store.prune(policy, now_ms).expect("prune");
    assert_eq!(pruned, 3, "two by age, one by count");
    assert_eq!(store.entry_count(None).expect("count"), 3);
    assert_eq!(
        store.stats().expect("stats").dictations,
        6,
        "pruning never touches lifetime stats"
    );
}

#[test]
fn file_db_reopens_with_data_wal_and_same_stats_row() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("data").join("hark.db");

    {
        let mut store = Store::open(&db_path).expect("open creates parent dirs");
        store
            .record(&dictation(1, "persisted", "persisted"), true)
            .expect("record");
        store.reset_stats(7_777).expect("reset");
        store
            .record(&dictation(2, "after", "after"), true)
            .expect("record");
    }

    let store = Store::open(&db_path).expect("reopen");
    assert_eq!(store.entry_count(None).expect("count"), 2);
    let stats = store.stats().expect("stats");
    assert_eq!(
        stats.since_ts_ms, 7_777,
        "reopen must not reseed the fixed-id stats row"
    );
    assert_eq!(stats.dictations, 1, "counters persisted across reopen");
}

#[test]
fn migration_002_upgrades_a_v1_database_without_losing_data() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("hark.db");

    // Build a database exactly as migration 001 left it: the real 001 file,
    // user_version = 1, and a stats row with counters already accumulated.
    {
        let conn = rusqlite::Connection::open(&db_path).expect("raw open");
        conn.execute_batch(include_str!("../migrations/001_init.sql"))
            .expect("apply 001");
        conn.pragma_update(None, "user_version", 1).expect("stamp");
        conn.execute(
            "INSERT INTO stats (id, dictations, words, since_ts_ms) VALUES (1, 5, 25, 9_000)",
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

    let mut store = Store::open(&db_path).expect("open runs migration 002");
    let stats = store.stats().expect("stats");
    assert_eq!(stats.dictations, 5, "001-era counters survive the upgrade");
    assert_eq!(stats.words, 25);
    assert_eq!(stats.total_ms, 0, "pre-002 rows contribute a zero sum");
    assert_eq!(stats.since_ts_ms, 9_000);
    assert_eq!(store.entry_count(None).expect("count"), 1);

    // New dictations accumulate into the new column.
    store
        .record(&dictation(2, "raw", "two words"), true)
        .expect("record");
    assert_eq!(store.stats().expect("stats").total_ms, 800);
}

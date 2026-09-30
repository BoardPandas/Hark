//! Numeric privacy, historical coverage and calendar behavior at the public
//! storage boundary. All databases are isolated from the user's data.

use hark_store::{InsightsRequest, NewDictation, Retention, Store};
use jiff::{tz::TimeZone, Timestamp};
use rusqlite::{params, Connection};

const DAY: i64 = 86_400_000;

fn ms(value: &str) -> i64 {
    value
        .parse::<Timestamp>()
        .expect("timestamp")
        .as_millisecond()
}

fn request(now: &str, days: u16) -> InsightsRequest {
    InsightsRequest {
        days,
        now_ms: ms(now),
        time_zone: TimeZone::UTC,
        analyze_text: false,
    }
}

fn dictation(ts_ms: i64) -> NewDictation {
    NewDictation {
        ts_ms,
        raw_text: "alpha beta".into(),
        final_text: "Alpha beta.".into(),
        voice: "clean".into(),
        stt_provider: "deepgram".into(),
        stt_model: "nova-3".into(),
        cleanup_model: None,
        invocation: None,
        audio_ms: 1_000,
        stt_ms: 100,
        cleanup_ms: None,
        total_ms: 200,
        spellbook_replacements: Some(0),
        foreground_app: None,
    }
}

#[test]
fn capture_off_clear_delete_and_reset_have_independent_lifecycles() {
    let mut store = Store::open_in_memory().expect("store");
    let query = request("2026-09-30T12:00:00Z", 7);
    let d = dictation(query.now_ms - 1_000);
    store.record(&d, false).expect("record with capture off");
    store.record(&d, true).expect("record with capture on");
    let id = store.entries(None, 1, 0).expect("entries")[0].id;
    store.delete_entry(id).expect("delete entry");
    store.record(&d, true).expect("another entry");
    store.clear_entries().expect("clear transcripts");
    let insights = store.insights(&query).expect("insights");
    assert_eq!(insights.period.dictations, 3);
    assert_eq!(insights.period.timed_dictations, 3);
    assert_eq!(insights.period.measured_corrections, 3);
    assert_eq!(insights.lifetime.dictations, 3);
    assert_eq!(store.entry_count(None).expect("count"), 0);

    store
        .record(&d, true)
        .expect("transcript retained on reset");
    store.reset_stats(query.now_ms).expect("reset");
    let reset = store.insights(&query).expect("insights after reset");
    assert_eq!(reset.period.dictations, 0);
    assert_eq!(reset.lifetime.dictations, 0);
    assert_eq!(reset.current_streak, 0);
    assert_eq!(reset.coverage.tracking_since_ms, query.now_ms);
    assert_eq!(store.entry_count(None).expect("count"), 1);
    assert!(reset.period.estimated_wpm().is_none());
    assert!(reset.period.estimated_saved_ms().is_none());
}

#[test]
fn invocation_output_is_separate_from_dictated_words_and_time_estimate() {
    let mut store = Store::open_in_memory().expect("store");
    let query = request("2026-09-30T12:00:00Z", 7);
    let mut d = dictation(query.now_ms);
    d.invocation = Some("alpha beta".into());
    d.final_text = "One two three four five six seven eight".into();
    d.spellbook_replacements = Some(2);
    store.record(&d, true).expect("record");
    let insights = store.insights(&query).expect("insights");
    let totals = insights.period;
    assert_eq!(totals.words, 2);
    assert_eq!(totals.expanded_words, 8);
    assert_eq!(totals.invocations, 1);
    assert_eq!(totals.corrections, 2);
    assert_eq!(totals.estimated_wpm(), Some(120.0));
    assert_eq!(totals.estimated_saved_ms(), Some(2_000));
    assert_eq!(totals.mean_clip_ms(), Some(1_000));
}

#[test]
fn completed_latency_percentiles_and_breakdowns_use_real_rows() {
    let mut store = Store::open_in_memory().expect("store");
    let query = request("2026-09-30T12:00:00Z", 7);
    for n in 1..=20 {
        let mut d = dictation(query.now_ms);
        d.total_ms = n * 100;
        if n <= 10 {
            d.foreground_app = Some("editor".into());
        } else {
            d.stt_provider = "local".into();
        }
        store.record(&d, false).expect("record");
    }
    let insights = store.insights(&query).expect("insights");
    assert_eq!(insights.period.median_ms, Some(1_050));
    assert_eq!(insights.period.p95_ms, Some(1_900));
    assert_eq!(insights.providers.len(), 2);
    assert_eq!(insights.providers[0].label, "deepgram");
    assert_eq!(insights.providers[0].median_ms, Some(550));
    assert_eq!(insights.apps.len(), 1);
    assert_eq!(insights.apps[0].dictations, 10);
    assert_eq!(insights.unknown_app_dictations, 10);
    assert_eq!(insights.peak_hour, Some(12));
}

#[test]
fn range_boundaries_zero_fill_and_streak_grace_use_calendar_days() {
    let mut store = Store::open_in_memory().expect("store");
    let query = request("2026-09-30T12:00:00Z", 7);
    for date in [
        "2026-09-23T23:59:59Z",
        "2026-09-24T00:00:00Z",
        "2026-09-27T12:00:00Z",
        "2026-09-28T12:00:00Z",
        "2026-09-29T12:00:00Z",
    ] {
        store.record(&dictation(ms(date)), false).expect("record");
    }
    // Future records must not populate today's metrics.
    store
        .record(&dictation(ms("2026-09-30T13:00:00Z")), false)
        .expect("future record");
    let insights = store.insights(&query).expect("insights");
    assert_eq!(insights.period.dictations, 4);
    assert_eq!(insights.previous.dictations, 1);
    assert_eq!(insights.daily.len(), 7);
    assert_eq!(insights.activity.len(), 366);
    assert_eq!(insights.daily[0].date.to_string(), "2026-09-24");
    assert_eq!(insights.daily[1].dictations, 0);
    assert_eq!(insights.current_streak, 3, "today may still become active");
    assert_eq!(insights.longest_streak, 3);
    let tomorrow = store
        .insights(&request("2026-10-02T00:00:00Z", 7))
        .expect("later");
    assert_eq!(
        tomorrow.current_streak, 0,
        "a fully missed day ends the streak"
    );
}

#[test]
fn daylight_saving_days_and_local_midnight_do_not_assume_24_hours() {
    let mut store = Store::open_in_memory().expect("store");
    let mut query = request("2026-03-09T03:30:00Z", 1);
    query.time_zone = TimeZone::get("America/New_York").expect("zone");
    for date in [
        "2026-03-08T04:59:59Z", // March 7, 23:59:59 EST: previous day
        "2026-03-08T05:00:00Z", // March 8, 00:00 EST
        "2026-03-08T06:30:00Z", // 01:30 EST
        "2026-03-08T07:30:00Z", // 03:30 EDT, same local day
        "2026-03-09T03:00:00Z", // 23:00 EDT, still March 8
    ] {
        store.record(&dictation(ms(date)), false).expect("record");
    }
    let insights = store.insights(&query).expect("spring DST");
    assert_eq!(insights.daily[0].date.to_string(), "2026-03-08");
    assert_eq!(insights.period.dictations, 4);
    assert_eq!(insights.previous.dictations, 1);
    assert_eq!(insights.current_streak, 2);

    for date in ["2026-11-01T05:30:00Z", "2026-11-01T06:30:00Z"] {
        store
            .record(&dictation(ms(date)), false)
            .expect("repeated hour");
    }
    query.now_ms = ms("2026-11-02T04:30:00Z");
    let fall = store.insights(&query).expect("fall DST");
    assert_eq!(fall.period.dictations, 2);
    assert_eq!(
        fall.peak_hour,
        Some(1),
        "both 1:30 times belong to the same hour"
    );
}

#[test]
fn retention_removes_old_numeric_events_without_touching_lifetime_or_history() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("hark.db");
    let mut store = Store::open(&path).expect("store");
    let now = ms("2026-09-30T12:00:00Z");
    let cutoff = now - 366 * DAY;
    for ts in [cutoff - 1, cutoff, now] {
        store.record(&dictation(ts), true).expect("record");
    }
    store
        .prune(
            Retention {
                max_entries: 100,
                max_age_days: 1_000,
            },
            now,
        )
        .expect("prune");
    assert_eq!(store.stats().expect("stats").dictations, 3);
    assert_eq!(store.entry_count(None).expect("history count"), 3);
    let conn = Connection::open(path).expect("inspection connection");
    let events: i64 = conn
        .query_row("SELECT COUNT(*) FROM insight_events", [], |r| r.get(0))
        .expect("count");
    let first: i64 = conn
        .query_row("SELECT MIN(ts_ms) FROM insight_events", [], |r| r.get(0))
        .expect("first");
    assert_eq!(events, 2);
    assert_eq!(first, cutoff, "retention boundary is inclusive");
}

fn legacy_database(path: &std::path::Path, now: i64) {
    let conn = Connection::open(path).expect("legacy connection");
    for migration in [
        include_str!("../migrations/001_init.sql"),
        include_str!("../migrations/002_stats_total_ms.sql"),
        include_str!("../migrations/003_entries_invocation.sql"),
        include_str!("../migrations/004_meetings.sql"),
    ] {
        conn.execute_batch(migration).expect("legacy migration");
    }
    conn.pragma_update(None, "user_version", 4)
        .expect("legacy version");
    conn.execute(
        "INSERT INTO stats (id, dictations, words, since_ts_ms) VALUES (1, 20, 70, ?1)",
        [now - 5 * DAY],
    )
    .expect("lifetime counters");
    for (ts, raw, final_text, invocation) in [
        (now - 7 * DAY, "old", "before stats reset", None),
        (
            now - 4 * DAY,
            "ignored raw",
            "alpha\u{2003}beta\tgamma",
            None,
        ),
        (
            now - 2 * DAY,
            "trigger phrase",
            "one two three four five six",
            Some("trigger phrase"),
        ),
    ] {
        conn.execute(
            "INSERT INTO entries (ts_ms, raw_text, final_text, voice, stt_provider, stt_model, \
             stt_ms, total_ms, invocation) VALUES (?1, ?2, ?3, 'clean', 'deepgram', 'nova-3', 100, 200, ?4)",
            params![ts, raw, final_text, invocation],
        ).expect("legacy entry");
    }
}

#[test]
fn migration_preserves_counters_backfills_only_known_fields_and_runs_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("hark.db");
    let now = Timestamp::now().as_millisecond();
    legacy_database(&path, now);
    let query = InsightsRequest {
        now_ms: now,
        days: 30,
        time_zone: TimeZone::UTC,
        analyze_text: false,
    };
    let mut store = Store::open(&path).expect("migrate");
    let old = store.insights(&query).expect("legacy insights");
    assert_eq!(old.lifetime.dictations, 20);
    assert_eq!(old.lifetime.words, 70);
    assert_eq!(
        old.period.dictations, 2,
        "prior-reset history is not resurrected"
    );
    assert_eq!(
        old.period.words, 5,
        "Unicode whitespace and invocation counting match lifetime"
    );
    assert_eq!(old.period.expanded_words, 6);
    assert_eq!(old.period.timed_dictations, 0);
    assert_eq!(old.period.measured_corrections, 0);
    assert_eq!(old.period.estimated_wpm(), None);
    assert_eq!(old.coverage.backfilled_dictations, 2);
    assert!(!old.coverage.period_complete);
    store
        .record(&dictation(now), false)
        .expect("new measured record");
    let mixed = store.insights(&query).expect("mixed coverage");
    assert_eq!(mixed.period.dictations, 3);
    assert_eq!(mixed.period.timed_dictations, 1);
    assert_eq!(mixed.period.timed_words, 2);
    assert_eq!(mixed.period.estimated_wpm(), Some(120.0));
    assert_eq!(mixed.period.estimated_saved_ms(), Some(2_000));
    drop(store);
    let reopened = Store::open(&path).expect("reopen");
    assert_eq!(
        reopened
            .insights(&query)
            .expect("reopened insights")
            .period
            .dictations,
        3
    );
    assert_eq!(reopened.entry_count(None).expect("history unchanged"), 3);
    let conn = Connection::open(path).expect("inspection connection");
    let unknowns: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM insight_events WHERE legacy = 1 AND audio_ms IS NULL \
         AND spellbook_replacements IS NULL AND foreground_app IS NULL",
            [],
            |r| r.get(0),
        )
        .expect("unknown measurement count");
    assert_eq!(unknowns, 2);
}

#[test]
fn text_analysis_requires_opt_in_and_does_not_count_canned_expansion() {
    let mut store = Store::open_in_memory().expect("store");
    let mut query = request("2026-09-30T12:00:00Z", 7);
    let mut d = dictation(query.now_ms);
    d.raw_text = "launch project".into();
    d.final_text = "secret template secret template secret template".into();
    d.invocation = Some("launch project".into());
    store.record(&d, true).expect("invocation");
    d.invocation = None;
    d.final_text = "Launch project. Launch project.".into();
    store.record(&d, true).expect("normal dictation");
    assert!(store.insights(&query).expect("opt out").patterns.is_none());
    query.analyze_text = true;
    let patterns = store
        .insights(&query)
        .expect("opt in")
        .patterns
        .expect("analysis");
    assert_eq!(patterns.sampled_dictations, 2);
    assert!(!patterns.truncated);
    assert_eq!(patterns.words[0].text, "launch");
    assert_eq!(patterns.words[0].count, 3);
    assert!(!patterns.words.iter().any(|word| word.text == "secret"));
    assert!(patterns
        .phrases
        .iter()
        .any(|p| p.text == "launch project" && p.count == 3));
    store.clear_entries().expect("clear");
    let empty = store.insights(&query).expect("after clear");
    assert_eq!(empty.period.dictations, 2);
    assert_eq!(
        empty.patterns.expect("empty analysis").sampled_dictations,
        0
    );
}

#[test]
fn invalid_ranges_fail_without_division_or_unbounded_queries() {
    let store = Store::open_in_memory().expect("store");
    for days in [0, 367, u16::MAX] {
        assert!(store
            .insights(&request("2026-09-30T12:00:00Z", days))
            .is_err());
    }
    let empty = store
        .insights(&request("2026-09-30T12:00:00Z", 7))
        .expect("empty");
    assert_eq!(empty.period.median_ms, None);
    assert_eq!(empty.period.p95_ms, None);
    assert_eq!(empty.peak_hour, None);
    assert!(empty.providers.is_empty());
}

#[test]
fn invalid_measurement_rolls_back_history_and_lifetime_in_the_same_transaction() {
    let mut store = Store::open_in_memory().expect("store");
    let query = request("2026-09-30T12:00:00Z", 7);
    let mut d = dictation(query.now_ms);
    d.spellbook_replacements = Some(-1);
    assert!(store.record(&d, true).is_err());
    assert_eq!(store.entry_count(None).expect("entries"), 0);
    assert_eq!(store.stats().expect("stats").dictations, 0);
    assert_eq!(
        store.insights(&query).expect("insights").period.dictations,
        0
    );
}

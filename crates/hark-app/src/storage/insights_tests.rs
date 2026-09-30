use super::*;
use jiff::tz::TimeZone;

fn record_command(index: usize) -> StorageCmd {
    StorageCmd::Record {
        record: Box::new(DictationRecord {
            raw_text: format!("raw {index}"),
            final_text: format!("final {index}"),
            voice: "verbatim".into(),
            stt_provider: "local".into(),
            stt_model: "parakeet".into(),
            cleanup_model: None,
            invocation: None,
            audio_ms: 1_000,
            stt_ms: 100,
            cleanup_ms: None,
            total_ms: 200,
            spellbook_replacements: Some(2),
            foreground_app: Some("editor".into()),
        }),
        capture: true,
        retention: Retention {
            max_entries: 100,
            max_age_days: 90,
        },
    }
}

#[test]
fn worker_queries_deliver_real_rows_without_incrementing_write_generation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = egui::Context::default();
    let handle = spawn(&dir.path().join("hark.db"), ctx).expect("worker");
    for index in 0..5 {
        handle.send(record_command(index));
    }
    let (tx, rx) = mpsc::channel();
    handle.send(StorageCmd::GetRecentEntries { reply: tx });
    let rows = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("worker answered")
        .expect("query");
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].final_text, "final 4");
    assert_eq!(rows[2].final_text, "final 2");
    let (tx, rx) = mpsc::channel();
    handle.send(StorageCmd::GetInsights {
        request: InsightsRequest {
            days: 7,
            now_ms: unix_now_ms(),
            time_zone: TimeZone::UTC,
            analyze_text: false,
        },
        reply: tx,
    });
    let result = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("worker answered")
        .expect("query");
    assert_eq!(result.period.dictations, 5);
    assert_eq!(result.period.corrections, 10);
    assert_eq!(result.apps[0].label, "editor");
    assert!(result.patterns.is_none());
    assert_eq!(
        handle.generation(),
        5,
        "read replies cannot trigger a reload loop"
    );
}

#[test]
fn invalid_insights_request_replies_with_error_and_worker_keeps_running() {
    let mut store = Store::open_in_memory().expect("store");
    let (tx, rx) = mpsc::channel();
    let changed = apply(
        &mut store,
        Path::new("unused"),
        StorageCmd::GetInsights {
            request: InsightsRequest {
                days: 0,
                now_ms: unix_now_ms(),
                time_zone: TimeZone::UTC,
                analyze_text: false,
            },
            reply: tx,
        },
    )
    .expect("query command is handled");
    assert!(!changed);
    assert!(rx.try_recv().expect("error reply").is_err());
    assert!(apply(&mut store, Path::new("unused"), record_command(0)).expect("next record"));
}

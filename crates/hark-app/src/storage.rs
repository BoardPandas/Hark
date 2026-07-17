//! The storage worker thread (spec §3.2/§3.3): consumes commands off a
//! channel, owns the writer `Store`, prunes after each insert, and wakes the
//! UI (`request_repaint` + a generation counter) after every write. DB writes
//! happen strictly after injection by construction: the only source of
//! `Record` commands is the pipeline's post-inject `Injected` event.
//!
//! The UI thread owns the reader `Store` (paged history queries, stats); all
//! mutations funnel through this thread so there is exactly one writer.

use hark_config::Settings;
use hark_pipeline::DictationRecord;
use hark_store::{NewDictation, Retention, Store, StoreError};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;

/// What the worker executes. `Record` carries the policy that was active
/// when its pipeline started (settings changes restart the pipeline, so the
/// policy travels with the run instead of racing a shared cell).
pub enum StorageCmd {
    /// Persist one completed dictation, then prune.
    Record {
        record: Box<DictationRecord>,
        capture: bool,
        retention: Retention,
    },
    /// Re-apply retention now. Sent on every pipeline start so a lowered cap
    /// takes effect at save/startup, not at the next dictation.
    Prune(Retention),
    /// History panel: delete one entry.
    DeleteEntry(i64),
    /// History panel "Clear all": entries only, stats untouched.
    ClearEntries,
    /// Stats panel "Reset stats": counters only, entries untouched.
    ResetStats,
}

/// The dictation-record half of the policy, cloned into the event pump at
/// pipeline start.
#[derive(Clone, Copy)]
pub struct RecordPolicy {
    pub capture: bool,
    pub retention: Retention,
}

pub fn record_policy(settings: &Settings) -> RecordPolicy {
    RecordPolicy {
        capture: settings.history.capture,
        retention: retention(settings),
    }
}

pub fn retention(settings: &Settings) -> Retention {
    Retention {
        max_entries: settings.history.max_entries,
        max_age_days: settings.history.max_age_days,
    }
}

/// UI-side handle: the command sender, the reader connection for queries,
/// and the write-generation counter pages key their caches on.
pub struct StorageHandle {
    tx: Option<Sender<StorageCmd>>,
    reader: Store,
    generation: Arc<AtomicU64>,
    worker: Option<JoinHandle<()>>,
}

impl StorageHandle {
    /// A sender clone for the event pump (`PipelineController`).
    pub fn sender(&self) -> Sender<StorageCmd> {
        self.tx.clone().expect("sender present until drop")
    }

    /// Queue a command. A send can only fail after the worker exited, which
    /// only happens at shutdown; nothing useful is left to do then.
    pub fn send(&self, cmd: StorageCmd) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(cmd);
        }
    }

    /// Bumped after every successful write; pages re-query when it moves.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// The UI thread's read-only connection (WAL: sees committed writes).
    pub fn reader(&self) -> &Store {
        &self.reader
    }
}

impl Drop for StorageHandle {
    /// Disconnect and join so pending writes commit before the process
    /// exits. `HarkApp` declares the pipeline before this handle: the pump
    /// (which holds a sender clone) is gone by the time this runs, so the
    /// worker's `recv` loop ends as soon as the queue drains.
    fn drop(&mut self) {
        self.tx.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Open both connections (creating the DB and applying migrations) and start
/// the worker. `ctx` is the sanctioned cross-thread wake-up: a repaint after
/// each write refreshes the history panel while the app sits idle.
pub fn spawn(db_path: &Path, ctx: egui::Context) -> Result<StorageHandle, StoreError> {
    let writer = Store::open(db_path)?;
    let reader = Store::open(db_path)?;
    let (tx, rx) = mpsc::channel();
    let generation = Arc::new(AtomicU64::new(0));
    let worker_generation = Arc::clone(&generation);
    let worker = std::thread::Builder::new()
        .name("hark-storage".to_string())
        .spawn(move || worker_loop(writer, rx, worker_generation, ctx))
        .expect("spawning the storage thread cannot fail");
    Ok(StorageHandle {
        tx: Some(tx),
        reader,
        generation,
        worker: Some(worker),
    })
}

fn worker_loop(
    mut store: Store,
    rx: Receiver<StorageCmd>,
    generation: Arc<AtomicU64>,
    ctx: egui::Context,
) {
    while let Ok(cmd) = rx.recv() {
        match apply(&mut store, cmd) {
            Ok(changed) => {
                if changed {
                    generation.fetch_add(1, Ordering::Release);
                    ctx.request_repaint();
                }
            }
            // Log labels and causes only; a failed history write must never
            // disturb dictation (the hot path finished before this ran).
            Err(e) => log::error!("storage write failed: {e}"),
        }
    }
}

/// Execute one command; `Ok(true)` when the database changed (the seam the
/// tests drive without a thread or an egui context).
fn apply(store: &mut Store, cmd: StorageCmd) -> Result<bool, StoreError> {
    match cmd {
        StorageCmd::Record {
            record,
            capture,
            retention,
        } => {
            let now_ms = unix_now_ms();
            store.record(&new_dictation(*record, now_ms), capture)?;
            store.prune(retention, now_ms)?;
            Ok(true)
        }
        StorageCmd::Prune(retention) => Ok(store.prune(retention, unix_now_ms())? > 0),
        StorageCmd::DeleteEntry(id) => store.delete_entry(id),
        StorageCmd::ClearEntries => Ok(store.clear_entries()? > 0),
        StorageCmd::ResetStats => {
            store.reset_stats(unix_now_ms())?;
            Ok(true)
        }
    }
}

/// Stamp the record with the wall clock at persist time (ms precision; the
/// hot path does not carry a timestamp and the write runs right after it).
fn new_dictation(r: DictationRecord, ts_ms: i64) -> NewDictation {
    NewDictation {
        ts_ms,
        raw_text: r.raw_text,
        final_text: r.final_text,
        voice: r.voice,
        stt_provider: r.stt_provider,
        stt_model: r.stt_model,
        cleanup_model: r.cleanup_model,
        audio_ms: r.audio_ms as i64,
        stt_ms: r.stt_ms as i64,
        cleanup_ms: r.cleanup_ms.map(|v| v as i64),
        total_ms: r.total_ms as i64,
    }
}

fn unix_now_ms() -> i64 {
    jiff::Timestamp::now().as_millisecond()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> Box<DictationRecord> {
        Box::new(DictationRecord {
            raw_text: "raw words".to_string(),
            final_text: "three final words".to_string(),
            voice: "clean".to_string(),
            stt_provider: "deepgram".to_string(),
            stt_model: "nova-3".to_string(),
            cleanup_model: Some("gpt-5-nano".to_string()),
            audio_ms: 1_500,
            stt_ms: 400,
            cleanup_ms: Some(300),
            total_ms: 800,
        })
    }

    const KEEP_ALL: Retention = Retention {
        max_entries: 1_000,
        max_age_days: 90,
    };

    #[test]
    fn record_command_persists_and_reports_a_change() {
        let mut store = Store::open_in_memory().expect("open");
        let changed = apply(
            &mut store,
            StorageCmd::Record {
                record: record(),
                capture: true,
                retention: KEEP_ALL,
            },
        )
        .expect("apply");
        assert!(changed);

        let entries = store.entries(None, 10, 0).expect("entries");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].final_text, "three final words");
        assert!(entries[0].ts_ms > 0, "persist time stamps the entry");
        assert_eq!(store.stats().expect("stats").total_ms, 800);
    }

    #[test]
    fn capture_off_records_no_content_but_still_reports_a_change() {
        let mut store = Store::open_in_memory().expect("open");
        let changed = apply(
            &mut store,
            StorageCmd::Record {
                record: record(),
                capture: false,
                retention: KEEP_ALL,
            },
        )
        .expect("apply");
        assert!(changed, "the stats tick must still repaint the UI");
        assert_eq!(store.entry_count(None).expect("count"), 0);
        assert_eq!(store.stats().expect("stats").dictations, 1);
    }

    #[test]
    fn record_prunes_with_the_policy_it_carries() {
        let mut store = Store::open_in_memory().expect("open");
        let tight = Retention {
            max_entries: 2,
            max_age_days: 90,
        };
        for _ in 0..4 {
            apply(
                &mut store,
                StorageCmd::Record {
                    record: record(),
                    capture: true,
                    retention: tight,
                },
            )
            .expect("apply");
        }
        assert_eq!(store.entry_count(None).expect("count"), 2);
        assert_eq!(store.stats().expect("stats").dictations, 4);
    }

    #[test]
    fn prune_reports_change_only_when_rows_went() {
        let mut store = Store::open_in_memory().expect("open");
        assert!(
            !apply(&mut store, StorageCmd::Prune(KEEP_ALL)).expect("apply"),
            "an empty prune must not repaint"
        );
        for _ in 0..3 {
            apply(
                &mut store,
                StorageCmd::Record {
                    record: record(),
                    capture: true,
                    retention: KEEP_ALL,
                },
            )
            .expect("apply");
        }
        let tight = Retention {
            max_entries: 1,
            max_age_days: 90,
        };
        assert!(apply(&mut store, StorageCmd::Prune(tight)).expect("apply"));
        assert_eq!(store.entry_count(None).expect("count"), 1);
    }

    #[test]
    fn delete_and_clear_touch_entries_only_and_reset_touches_stats_only() {
        let mut store = Store::open_in_memory().expect("open");
        for _ in 0..2 {
            apply(
                &mut store,
                StorageCmd::Record {
                    record: record(),
                    capture: true,
                    retention: KEEP_ALL,
                },
            )
            .expect("apply");
        }
        let id = store.entries(None, 1, 0).expect("entries")[0].id;

        assert!(apply(&mut store, StorageCmd::DeleteEntry(id)).expect("delete"));
        assert!(
            !apply(&mut store, StorageCmd::DeleteEntry(id)).expect("re-delete"),
            "a missing id is not a change"
        );
        assert!(apply(&mut store, StorageCmd::ClearEntries).expect("clear"));
        assert_eq!(store.stats().expect("stats").dictations, 2);

        assert!(apply(&mut store, StorageCmd::ResetStats).expect("reset"));
        assert_eq!(store.stats().expect("stats").dictations, 0);
    }

    #[test]
    fn policy_mirrors_the_history_settings() {
        let mut settings = Settings::default();
        settings.history.capture = false;
        settings.history.max_entries = 42;
        settings.history.max_age_days = 7;
        let policy = record_policy(&settings);
        assert!(!policy.capture);
        assert_eq!(policy.retention.max_entries, 42);
        assert_eq!(policy.retention.max_age_days, 7);
    }
}

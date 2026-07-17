//! Hark local storage: dictation history + lifetime stats in SQLite.
//!
//! Design (Phase 4 spec, `tasks/2026-07-16-phase4-ui-storage.md` §3.3):
//! - WAL journal, two-connection pattern: the storage worker owns a writer
//!   `Store`, the UI thread owns a reader `Store` for paged queries. This
//!   crate is thread-agnostic; each `Store` stays on one thread.
//! - Migrations are numbered, immutable, and embedded; `PRAGMA user_version`
//!   tracks the applied count. Never renumber an applied migration.
//! - The database is the sanctioned transcript store. The text-bearing types
//!   (`NewDictation`, `Entry`) deliberately do not derive `Debug`, so a
//!   stray `{:?}` in a log line cannot leak transcript content.
//! - "Clear history" and "Reset stats" are independent: clearing entries
//!   never touches the stats row, and vice versa.
//! - Capture-off semantics: `record(d, false)` writes no entry row (no
//!   transcript content persisted) but the numeric counters still tick.

use rusqlite::{params, Connection};
use std::path::Path;
use std::time::Duration;
use thiserror::Error;

/// Embedded migrations, applied in order; index + 1 == resulting
/// `user_version`. Append only; never edit or renumber an applied file.
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/001_init.sql"),
    include_str!("../migrations/002_stats_total_ms.sql"),
];

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("cannot create data directory {path}: {source}")]
    CreateDir {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot open database at {path}: {source}")]
    Open {
        path: String,
        #[source]
        source: rusqlite::Error,
    },
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
}

/// One completed dictation, ready to persist. Timing fields are wall-clock
/// milliseconds; `audio_ms` (speaking time) feeds stats only and is not
/// stored per entry. No `Debug` derive: carries transcript content.
pub struct NewDictation {
    pub ts_ms: i64,
    pub raw_text: String,
    pub final_text: String,
    pub voice: String,
    pub stt_provider: String,
    pub stt_model: String,
    /// `None` when cleanup did not run (Verbatim voice, gate skip, fail-open).
    pub cleanup_model: Option<String>,
    pub audio_ms: i64,
    pub stt_ms: i64,
    pub cleanup_ms: Option<i64>,
    pub total_ms: i64,
}

/// One stored history row. No `Debug` derive: carries transcript content.
pub struct Entry {
    pub id: i64,
    pub ts_ms: i64,
    pub raw_text: String,
    pub final_text: String,
    pub voice: String,
    pub stt_provider: String,
    pub stt_model: String,
    pub cleanup_model: Option<String>,
    pub stt_ms: i64,
    pub cleanup_ms: Option<i64>,
    pub total_ms: i64,
}

/// Lifetime counters (numbers only, safe to log or Debug-format).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    pub dictations: i64,
    pub words: i64,
    pub audio_ms: i64,
    pub stt_ms: i64,
    pub cleanup_ms: i64,
    /// Sum of release-to-inject wall times (migration 002); entries recorded
    /// before 002 contribute 0, so a derived average converges upward.
    pub total_ms: i64,
    pub since_ts_ms: i64,
}

/// Retention policy for [`Store::prune`]. Validation (both >= 1) lives in
/// hark-config; the store executes whatever it is given.
#[derive(Debug, Clone, Copy)]
pub struct Retention {
    /// Keep at most this many newest entries.
    pub max_entries: u32,
    /// Delete entries strictly older than this many days.
    pub max_age_days: u32,
}

pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open (creating if needed) the database at `path`, apply pending
    /// migrations, and seed the stats row. Creates the parent directory.
    pub fn open(path: &Path) -> Result<Store, StoreError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| StoreError::CreateDir {
                path: dir.display().to_string(),
                source: e,
            })?;
        }
        let conn = Connection::open(path).map_err(|e| StoreError::Open {
            path: path.display().to_string(),
            source: e,
        })?;
        Store::init(conn)
    }

    /// In-memory database for tests; same schema and pragmas (an in-memory
    /// DB reports journal_mode "memory" instead of "wal", which is fine).
    pub fn open_in_memory() -> Result<Store, StoreError> {
        Store::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Store, StoreError> {
        // WAL lets the reader connection see consistent snapshots while the
        // writer inserts. journal_mode returns the resulting mode as a row;
        // accept whatever comes back (file DBs report "wal").
        conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get::<_, String>(0))?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(BUSY_TIMEOUT)?;

        let mut store = Store { conn };
        store.migrate()?;
        // Seed the singleton stats row on first open. INSERT OR IGNORE keys
        // on the fixed id, so an existing row (and its counters) is kept.
        store.conn.execute(
            "INSERT OR IGNORE INTO stats (id, since_ts_ms) VALUES (1, ?1)",
            params![unix_now_ms()],
        )?;
        Ok(store)
    }

    fn migrate(&mut self) -> Result<(), StoreError> {
        let applied: usize =
            self.conn
                .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))? as usize;
        for (idx, sql) in MIGRATIONS.iter().enumerate().skip(applied) {
            let tx = self.conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", (idx + 1) as i64)?;
            tx.commit()?;
        }
        Ok(())
    }

    /// Persist one dictation: entry row (only when `capture`) plus stats
    /// update, in one transaction. With capture off, no transcript content
    /// is written but the numeric counters still tick.
    pub fn record(&mut self, d: &NewDictation, capture: bool) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        if capture {
            tx.execute(
                "INSERT INTO entries (ts_ms, raw_text, final_text, voice, stt_provider, \
                 stt_model, cleanup_model, stt_ms, cleanup_ms, total_ms) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    d.ts_ms,
                    d.raw_text,
                    d.final_text,
                    d.voice,
                    d.stt_provider,
                    d.stt_model,
                    d.cleanup_model,
                    d.stt_ms,
                    d.cleanup_ms,
                    d.total_ms,
                ],
            )?;
        }
        tx.execute(
            "UPDATE stats SET dictations = dictations + 1, words = words + ?1, \
             audio_ms = audio_ms + ?2, stt_ms = stt_ms + ?3, cleanup_ms = cleanup_ms + ?4, \
             total_ms = total_ms + ?5 \
             WHERE id = 1",
            params![
                word_count(&d.final_text),
                d.audio_ms,
                d.stt_ms,
                d.cleanup_ms.unwrap_or(0),
                d.total_ms,
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Apply retention: delete entries strictly older than `max_age_days`
    /// and entries beyond the newest `max_entries`. Returns rows deleted.
    pub fn prune(&mut self, retention: Retention, now_ms: i64) -> Result<usize, StoreError> {
        let cutoff = now_ms - i64::from(retention.max_age_days) * 86_400_000;
        let tx = self.conn.transaction()?;
        let by_age = tx.execute("DELETE FROM entries WHERE ts_ms < ?1", params![cutoff])?;
        // LIMIT -1 OFFSET n selects everything after the newest n.
        let by_count = tx.execute(
            "DELETE FROM entries WHERE id IN (\
             SELECT id FROM entries ORDER BY ts_ms DESC, id DESC LIMIT -1 OFFSET ?1)",
            params![retention.max_entries],
        )?;
        tx.commit()?;
        Ok(by_age + by_count)
    }

    /// One page of history, newest first. `search` filters (case-insensitive
    /// substring over raw + final text); LIKE wildcards in the query are
    /// treated literally.
    pub fn entries(
        &self,
        search: Option<&str>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<Entry>, StoreError> {
        const COLS: &str = "id, ts_ms, raw_text, final_text, voice, stt_provider, \
                            stt_model, cleanup_model, stt_ms, cleanup_ms, total_ms";
        let map = |r: &rusqlite::Row<'_>| -> rusqlite::Result<Entry> {
            Ok(Entry {
                id: r.get(0)?,
                ts_ms: r.get(1)?,
                raw_text: r.get(2)?,
                final_text: r.get(3)?,
                voice: r.get(4)?,
                stt_provider: r.get(5)?,
                stt_model: r.get(6)?,
                cleanup_model: r.get(7)?,
                stt_ms: r.get(8)?,
                cleanup_ms: r.get(9)?,
                total_ms: r.get(10)?,
            })
        };
        let rows = match search.map(str::trim).filter(|s| !s.is_empty()) {
            Some(q) => {
                let mut stmt = self.conn.prepare(&format!(
                    "SELECT {COLS} FROM entries \
                     WHERE raw_text LIKE '%' || ?1 || '%' ESCAPE '\\' \
                        OR final_text LIKE '%' || ?1 || '%' ESCAPE '\\' \
                     ORDER BY ts_ms DESC, id DESC LIMIT ?2 OFFSET ?3"
                ))?;
                let found = stmt.query_map(params![escape_like(q), limit, offset], map)?;
                found.collect::<Result<Vec<_>, _>>()?
            }
            None => {
                let mut stmt = self.conn.prepare(&format!(
                    "SELECT {COLS} FROM entries ORDER BY ts_ms DESC, id DESC \
                     LIMIT ?1 OFFSET ?2"
                ))?;
                let found = stmt.query_map(params![limit, offset], map)?;
                found.collect::<Result<Vec<_>, _>>()?
            }
        };
        Ok(rows)
    }

    /// Total entries matching `search` (same semantics as [`Store::entries`]).
    pub fn entry_count(&self, search: Option<&str>) -> Result<u64, StoreError> {
        let n: i64 = match search.map(str::trim).filter(|s| !s.is_empty()) {
            Some(q) => self.conn.query_row(
                "SELECT COUNT(*) FROM entries \
                 WHERE raw_text LIKE '%' || ?1 || '%' ESCAPE '\\' \
                    OR final_text LIKE '%' || ?1 || '%' ESCAPE '\\'",
                params![escape_like(q)],
                |r| r.get(0),
            )?,
            None => self
                .conn
                .query_row("SELECT COUNT(*) FROM entries", [], |r| r.get(0))?,
        };
        Ok(n as u64)
    }

    /// Delete one entry; `false` when the id did not exist.
    pub fn delete_entry(&mut self, id: i64) -> Result<bool, StoreError> {
        let n = self
            .conn
            .execute("DELETE FROM entries WHERE id = ?1", params![id])?;
        Ok(n > 0)
    }

    /// "Clear history": deletes all entries, never touches stats.
    pub fn clear_entries(&mut self) -> Result<usize, StoreError> {
        Ok(self.conn.execute("DELETE FROM entries", [])?)
    }

    pub fn stats(&self) -> Result<Stats, StoreError> {
        Ok(self.conn.query_row(
            "SELECT dictations, words, audio_ms, stt_ms, cleanup_ms, total_ms, since_ts_ms \
             FROM stats WHERE id = 1",
            [],
            |r| {
                Ok(Stats {
                    dictations: r.get(0)?,
                    words: r.get(1)?,
                    audio_ms: r.get(2)?,
                    stt_ms: r.get(3)?,
                    cleanup_ms: r.get(4)?,
                    total_ms: r.get(5)?,
                    since_ts_ms: r.get(6)?,
                })
            },
        )?)
    }

    /// "Reset stats": zeroes the counters and restarts the since-date,
    /// never touches entries.
    pub fn reset_stats(&mut self, now_ms: i64) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE stats SET dictations = 0, words = 0, audio_ms = 0, \
             stt_ms = 0, cleanup_ms = 0, total_ms = 0, since_ts_ms = ?1 WHERE id = 1",
            params![now_ms],
        )?;
        Ok(())
    }
}

/// Stats word counting: whitespace-separated tokens of the injected text.
/// Deliberately simple; it feeds a lifetime counter, not billing.
fn word_count(text: &str) -> i64 {
    text.split_whitespace().count() as i64
}

/// Escape LIKE wildcards so user search text matches literally
/// (pattern uses `ESCAPE '\'`).
fn escape_like(query: &str) -> String {
    let mut out = String::with_capacity(query.len());
    for c in query.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn unix_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_count_splits_on_any_whitespace() {
        assert_eq!(word_count(""), 0);
        assert_eq!(word_count("   "), 0);
        assert_eq!(word_count("one"), 1);
        assert_eq!(word_count("two  words\tand\nmore"), 4);
    }

    #[test]
    fn escape_like_neutralizes_wildcards() {
        assert_eq!(escape_like("plain"), "plain");
        assert_eq!(escape_like("100%"), "100\\%");
        assert_eq!(escape_like("a_b"), "a\\_b");
        assert_eq!(escape_like("back\\slash"), "back\\\\slash");
    }
}

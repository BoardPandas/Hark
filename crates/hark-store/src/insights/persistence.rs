use super::INSIGHTS_RETENTION_DAYS;
use crate::{spoken_word_count, word_count, NewDictation, StoreError};
use rusqlite::{params, Connection};

pub(super) const DAY_MS: i64 = 86_400_000;

pub(crate) fn record(conn: &Connection, d: &NewDictation) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO insight_events (ts_ms, words, audio_ms, total_ms, stt_provider, \
         voice, invocation, expanded_words, spellbook_replacements, foreground_app) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            d.ts_ms,
            spoken_word_count(d),
            d.audio_ms,
            d.total_ms,
            d.stt_provider,
            d.voice,
            d.invocation.is_some(),
            if d.invocation.is_some() {
                word_count(&d.final_text)
            } else {
                0
            },
            d.spellbook_replacements,
            d.foreground_app,
        ],
    )?;
    Ok(())
}

pub(crate) fn prune(conn: &Connection, now_ms: i64) -> Result<usize, StoreError> {
    Ok(conn.execute(
        "DELETE FROM insight_events WHERE ts_ms < ?1",
        [now_ms.saturating_sub(INSIGHTS_RETENTION_DAYS * DAY_MS)],
    )?)
}

pub(crate) fn backfill(conn: &Connection, now_ms: i64) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO insight_tracking (id, since_ts_ms) VALUES (1, ?1)",
        [now_ms],
    )?;
    // Do not resurrect entries from before a previous stats reset. History
    // deliberately survives that reset, while the numbers do not.
    let mut read = conn.prepare(
        "SELECT ts_ms, raw_text, final_text, stt_provider, voice, invocation, total_ms \
         FROM entries WHERE ts_ms >= ?1 \
         AND ts_ms >= COALESCE((SELECT since_ts_ms FROM stats WHERE id = 1), ?2)",
    )?;
    let mut insert = conn.prepare(
        "INSERT INTO insight_events (ts_ms, words, audio_ms, total_ms, stt_provider, \
         voice, invocation, expanded_words, spellbook_replacements, foreground_app, legacy) \
         VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7, NULL, NULL, 1)",
    )?;
    let mut rows = read.query(params![
        now_ms.saturating_sub(INSIGHTS_RETENTION_DAYS * DAY_MS),
        now_ms,
    ])?;
    while let Some(row) = rows.next()? {
        let raw: String = row.get(1)?;
        let final_text: String = row.get(2)?;
        let invocation = row.get::<_, Option<String>>(5)?.is_some();
        insert.execute(params![
            row.get::<_, i64>(0)?,
            word_count(if invocation { &raw } else { &final_text }),
            row.get::<_, i64>(6)?.max(0),
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            invocation,
            if invocation {
                word_count(&final_text)
            } else {
                0
            },
        ])?;
    }
    Ok(())
}

//! Meetings, their transcript segments, renamed speakers and notes
//! (migration 004; plan `tasks/2026-09-26-plan-meeting-transcription.md`
//! §4.5), plus the final-pass transcript status (migration 006), and speaker
//! names keyed by speaker key, per-line speaker corrections and the
//! notes-stale stamp (migration 007).
//!
//! - Search is FTS5 over segment text OR a case-insensitive title substring.
//!   FTS5 is compiled into the bundled SQLite Hark ships (asserted in
//!   `tests/meetings.rs`), so there is no LIKE fallback path to maintain.
//! - Cascading deletes (segments, speaker names) rely on `PRAGMA
//!   foreign_keys = ON`, set once per connection in `Store::init`.
//! - Speaker keys (`me`, `them`, `them:<n>`, `added:<n>`) are opaque text
//!   here; `hark_meeting::speakers` owns the vocabulary. Names are keyed by
//!   key, never by name, so two people called "Sam" cannot collide.
//! - Meeting records carry meeting content (transcript text, titles): none
//!   of the types below derive `Debug`, so a stray `{:?}` cannot leak a
//!   transcript into a log line. `MeetingSegment.speaker` and `.channel` are
//!   the only fields safe to log ad hoc, and even those go through explicit
//!   `id`/count fields in call sites, never a `Debug` dump of the struct.

use crate::{escape_like, Store, StoreError};
use rusqlite::params;

/// A new meeting, ready to open. No `Debug`: `app_hint` and `stt_provider`
/// are not transcript content but travel with types that are, and the rule
/// is simplest applied uniformly.
pub struct NewMeeting {
    pub id: String,
    pub started_ms: i64,
    /// `manual | ask | auto`.
    pub trigger: String,
    pub app_hint: Option<String>,
    pub stt_provider: String,
}

/// One transcript segment. Carries `text`: no `Debug`.
pub struct MeetingSegment {
    pub start_ms: i64,
    pub end_ms: i64,
    /// 0 = Me, 1 = Them.
    pub channel: u8,
    pub speaker: Option<u32>,
    pub text: String,
}

/// List-view row: everything the Meetings page needs without loading the
/// transcript. `title` is user content: no `Debug`.
pub struct MeetingSummary {
    pub id: String,
    pub started_ms: i64,
    pub ended_ms: Option<i64>,
    pub title: Option<String>,
    pub app_hint: Option<String>,
    pub trigger: String,
    pub audio_bytes: i64,
    pub audio_evicted_ms: Option<i64>,
    pub has_notes: bool,
    pub refined: bool,
    pub segment_count: i64,
    /// Migration 006: `pending[:<unix ms>]`, `failed:<reason>`, or `None`.
    /// A label, never content; `hark-pipeline` owns the vocabulary.
    pub transcript_status: Option<String>,
}

/// One stored transcript segment, as read back. Carries `text`: no `Debug`.
pub struct StoredSegment {
    /// The row id: what a reassignment names.
    pub id: i64,
    pub start_ms: i64,
    pub end_ms: i64,
    /// 0 = Me, 1 = Them.
    pub channel: u8,
    pub speaker: Option<u32>,
    /// Migration 007: the speaker key the user moved this line to; `None`
    /// means as transcribed.
    pub speaker_override: Option<String>,
    pub text: String,
}

/// Who named a speaker. A guess is replaced by a later guess; a user name
/// never is. No `Debug`, like every type in this module.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NameSource {
    User,
    Guess,
}

impl NameSource {
    fn as_str(self) -> &'static str {
        match self {
            NameSource::User => "user",
            NameSource::Guess => "guess",
        }
    }

    /// Unknown text (a newer build's source) reads as `User`, the one that is
    /// never overwritten.
    fn parse(s: &str) -> NameSource {
        if s == "guess" {
            NameSource::Guess
        } else {
            NameSource::User
        }
    }
}

/// One speaker's name in one meeting. `display_name` is content: no `Debug`.
pub struct SpeakerName {
    /// A `hark_meeting::SpeakerKey` as text.
    pub key: String,
    pub display_name: String,
    pub source: NameSource,
    /// A guess only: start of the line the name was heard in.
    pub evidence_ms: Option<i64>,
}

/// Full detail view: summary plus transcript, notes and speaker names.
pub struct MeetingDetail {
    pub summary: MeetingSummary,
    pub stt_provider: String,
    pub notes_json: Option<String>,
    /// Migration 007: when a reassignment left the notes crediting the old
    /// speaker; `None` when the notes match the speakers.
    pub notes_stale_ms: Option<i64>,
    pub segments: Vec<StoredSegment>,
    /// Ordered by key text.
    pub speakers: Vec<SpeakerName>,
}

const SUMMARY_COLS: &str = "m.id, m.started_ms, m.ended_ms, m.title, m.app_hint, m.trigger, \
                             m.audio_bytes, m.audio_evicted_ms, m.notes_json, m.refined, \
                             (SELECT COUNT(*) FROM meeting_segments s WHERE s.meeting_id = m.id), \
                             m.transcript_status";

fn map_summary(r: &rusqlite::Row<'_>) -> rusqlite::Result<MeetingSummary> {
    let notes_json: Option<String> = r.get(8)?;
    Ok(MeetingSummary {
        id: r.get(0)?,
        started_ms: r.get(1)?,
        ended_ms: r.get(2)?,
        title: r.get(3)?,
        app_hint: r.get(4)?,
        trigger: r.get(5)?,
        audio_bytes: r.get(6)?,
        audio_evicted_ms: r.get(7)?,
        has_notes: notes_json.is_some(),
        refined: r.get::<_, i64>(9)? != 0,
        segment_count: r.get(10)?,
        transcript_status: r.get(11)?,
    })
}

/// Build an FTS5 MATCH expression that treats `query` as literal words
/// AND'ed together. FTS5 operators (`AND`, `OR`, `NOT`, `*`, parens) only
/// cause a syntax error when unquoted, so every whitespace-separated token
/// is wrapped in a quoted string (embedded `"` doubled, the FTS5 string
/// escape) before it reaches `sqlite3_prepare`. A quoted string is always
/// valid FTS5 syntax regardless of its contents, so this never errors.
fn fts_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(|tok| format!("\"{}\"", tok.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

impl Store {
    /// Open a new meeting. `ended_ms`, `title`, `notes_json` and
    /// `audio_bytes`/`audio_evicted_ms` start unset/zero.
    pub fn create_meeting(&mut self, m: &NewMeeting) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO meetings (id, started_ms, trigger, app_hint, stt_provider) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![m.id, m.started_ms, m.trigger, m.app_hint, m.stt_provider],
        )?;
        Ok(())
    }

    /// Append one live segment (the rolling transcript while recording).
    pub fn append_meeting_segment(
        &mut self,
        id: &str,
        s: &MeetingSegment,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO meeting_segments (meeting_id, start_ms, end_ms, channel, speaker, text) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, s.start_ms, s.end_ms, s.channel, s.speaker, s.text],
        )?;
        Ok(())
    }

    /// Replace every segment (the Deepgram final pass) in one transaction:
    /// delete the old rows (and their FTS index entries, via the delete
    /// trigger), insert the new ones, and mark the meeting refined.
    pub fn replace_meeting_segments(
        &mut self,
        id: &str,
        segments: &[MeetingSegment],
        refined: bool,
    ) -> Result<(), StoreError> {
        self.replace_segments(id, segments, refined, None)
    }

    /// A new diarization may number people differently. Reset names atomically
    /// with replacement, and clear the notes-stale stamp. `baked_notes`, when
    /// given, replaces the notes in the same transaction: the notes with the
    /// old names written in, so they still read right once the names are
    /// gone. The title and the action checkboxes are kept.
    pub fn reprocess_meeting_segments(
        &mut self,
        id: &str,
        segments: &[MeetingSegment],
        baked_notes: Option<&str>,
    ) -> Result<(), StoreError> {
        self.replace_segments(id, segments, true, Some(baked_notes))
    }

    /// `reset_speakers`: `Some(baked notes)` for a reprocess, `None` to keep
    /// names and notes as they are.
    fn replace_segments(
        &mut self,
        id: &str,
        segments: &[MeetingSegment],
        refined: bool,
        reset_speakers: Option<Option<&str>>,
    ) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        if let Some(baked_notes) = reset_speakers {
            tx.execute(
                "DELETE FROM meeting_speaker_names WHERE meeting_id = ?1",
                params![id],
            )?;
            // Frozen since 007 and read only by a build rolled back to 006;
            // cleared so that build does not show names from the old
            // diarization either.
            tx.execute(
                "DELETE FROM meeting_speakers WHERE meeting_id = ?1",
                params![id],
            )?;
            tx.execute(
                "UPDATE meetings SET notes_stale_ms = NULL, \
                 notes_json = COALESCE(?2, notes_json) WHERE id = ?1",
                params![id, baked_notes],
            )?;
        }
        tx.execute(
            "DELETE FROM meeting_segments WHERE meeting_id = ?1",
            params![id],
        )?;
        for s in segments {
            tx.execute(
                "INSERT INTO meeting_segments (meeting_id, start_ms, end_ms, channel, speaker, text) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, s.start_ms, s.end_ms, s.channel, s.speaker, s.text],
            )?;
        }
        tx.execute(
            "UPDATE meetings SET refined = ?1 WHERE id = ?2",
            params![refined, id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Mark a meeting ended. A no-op (not an error) if `id` does not exist.
    pub fn finish_meeting(&mut self, id: &str, ended_ms: i64) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE meetings SET ended_ms = ?1 WHERE id = ?2",
            params![ended_ms, id],
        )?;
        Ok(())
    }

    /// Set a meeting's title. `false` when `id` does not exist.
    pub fn set_meeting_title(&mut self, id: &str, title: &str) -> Result<bool, StoreError> {
        let n = self.conn.execute(
            "UPDATE meetings SET title = ?1 WHERE id = ?2",
            params![title, id],
        )?;
        Ok(n > 0)
    }

    /// Set or clear (`None`) a meeting's notes JSON.
    pub fn set_meeting_notes(
        &mut self,
        id: &str,
        notes_json: Option<&str>,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE meetings SET notes_json = ?1 WHERE id = ?2",
            params![notes_json, id],
        )?;
        Ok(())
    }

    /// Set or clear (`None`) the final-pass transcript status. A no-op (not an
    /// error) if `id` does not exist.
    pub fn set_meeting_transcript_status(
        &mut self,
        id: &str,
        status: Option<&str>,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE meetings SET transcript_status = ?1 WHERE id = ?2",
            params![status, id],
        )?;
        Ok(())
    }

    /// Record the audio-on-disk size for the storage-cap planner.
    pub fn set_meeting_audio_bytes(&mut self, id: &str, bytes: i64) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE meetings SET audio_bytes = ?1 WHERE id = ?2",
            params![bytes, id],
        )?;
        Ok(())
    }

    /// Mark a meeting's audio evicted by the storage cap; also zeroes
    /// `audio_bytes` since the files are gone.
    pub fn mark_meeting_audio_evicted(&mut self, id: &str, at_ms: i64) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE meetings SET audio_evicted_ms = ?1, audio_bytes = 0 WHERE id = ?2",
            params![at_ms, id],
        )?;
        Ok(())
    }

    /// Name a speaker for one meeting, as the user (a guess there becomes the
    /// user's). An empty (after trim) `name` removes the name, reverting to
    /// the default label. A no-op for an unknown meeting.
    pub fn set_speaker_name(&mut self, id: &str, key: &str, name: &str) -> Result<(), StoreError> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            self.conn.execute(
                "DELETE FROM meeting_speaker_names WHERE meeting_id = ?1 AND speaker_key = ?2",
                params![id, key],
            )?;
        } else {
            self.conn.execute(
                "INSERT INTO meeting_speaker_names (meeting_id, speaker_key, display_name, source) \
                 SELECT ?1, ?2, ?3, 'user' WHERE EXISTS (SELECT 1 FROM meetings WHERE id = ?1) \
                 ON CONFLICT (meeting_id, speaker_key) DO UPDATE SET \
                 display_name = excluded.display_name, source = 'user', evidence_ms = NULL",
                params![id, key, trimmed],
            )?;
        }
        Ok(())
    }

    /// Apply names from the pipeline (guesses, and the "Your name" row) in one
    /// transaction. A row is written only where the key has no name yet or
    /// its name is itself a guess: a user's name is never overwritten. Blank
    /// names are skipped. A no-op for an unknown meeting. Returns how many
    /// rows were written.
    pub fn apply_speaker_names(
        &mut self,
        id: &str,
        names: &[SpeakerName],
    ) -> Result<usize, StoreError> {
        let tx = self.conn.transaction()?;
        let mut written = 0;
        for n in names {
            let name = n.display_name.trim();
            if name.is_empty() {
                continue;
            }
            written += tx.execute(
                "INSERT INTO meeting_speaker_names \
                 (meeting_id, speaker_key, display_name, source, evidence_ms) \
                 SELECT ?1, ?2, ?3, ?4, ?5 WHERE EXISTS (SELECT 1 FROM meetings WHERE id = ?1) \
                 ON CONFLICT (meeting_id, speaker_key) DO UPDATE SET \
                 display_name = excluded.display_name, source = excluded.source, \
                 evidence_ms = excluded.evidence_ms \
                 WHERE meeting_speaker_names.source = 'guess'",
                params![id, n.key, name, n.source.as_str(), n.evidence_ms],
            )?;
        }
        tx.commit()?;
        Ok(written)
    }

    /// Move one line to another speaker (`Some(key)`) or back to its
    /// transcribed speaker (`None`), and, in the same transaction, stamp the
    /// notes stale at `now_ms` if the meeting has notes. `None` when the
    /// segment is not in this meeting (nothing changes); otherwise whether
    /// the notes are now stale.
    pub fn assign_segment_speaker(
        &mut self,
        id: &str,
        segment_id: i64,
        key: Option<&str>,
        now_ms: i64,
    ) -> Result<Option<bool>, StoreError> {
        let tx = self.conn.transaction()?;
        let moved = tx.execute(
            "UPDATE meeting_segments SET speaker_override = ?3 WHERE id = ?2 AND meeting_id = ?1",
            params![id, segment_id, key],
        )?;
        if moved == 0 {
            return Ok(None);
        }
        let stale = tx.execute(
            "UPDATE meetings SET notes_stale_ms = ?2 WHERE id = ?1 AND notes_json IS NOT NULL",
            params![id, now_ms],
        )?;
        tx.commit()?;
        Ok(Some(stale > 0))
    }

    /// Notes rewritten after a reassignment. Clears the stale stamp only when
    /// it is not newer than `requested_ms`: a line moved while the refresh
    /// ran keeps the notes stale. Never touches the title. `false` when `id`
    /// does not exist.
    pub fn set_refreshed_notes(
        &mut self,
        id: &str,
        notes_json: &str,
        requested_ms: i64,
    ) -> Result<bool, StoreError> {
        let n = self.conn.execute(
            "UPDATE meetings SET notes_json = ?2, notes_stale_ms = \
             CASE WHEN notes_stale_ms <= ?3 THEN NULL ELSE notes_stale_ms END \
             WHERE id = ?1",
            params![id, notes_json, requested_ms],
        )?;
        Ok(n > 0)
    }

    /// Delete a meeting and (via `ON DELETE CASCADE`) its segments and
    /// speaker names. `false` when `id` did not exist. Does not touch
    /// audio files on disk; that is the caller's (`hark-meeting`) job.
    pub fn delete_meeting(&mut self, id: &str) -> Result<bool, StoreError> {
        let n = self
            .conn
            .execute("DELETE FROM meetings WHERE id = ?1", params![id])?;
        Ok(n > 0)
    }

    /// List meetings newest first. `search`, when non-blank, matches
    /// segment text via FTS5 OR the title as a case-insensitive substring.
    /// Hostile input (quotes, `AND`, `*`, parentheses) never produces an
    /// FTS syntax error: see [`fts_query`].
    pub fn meetings(&self, search: Option<&str>) -> Result<Vec<MeetingSummary>, StoreError> {
        let rows = match search.map(str::trim).filter(|s| !s.is_empty()) {
            Some(q) => {
                // meeting_segments_fts MATCH must reference the virtual
                // table's real name, not an alias: SQLite resolves the
                // implicit whole-row match column only against it.
                let mut stmt = self.conn.prepare(&format!(
                    "SELECT {SUMMARY_COLS} FROM meetings m WHERE \
                     m.title LIKE '%' || ?2 || '%' ESCAPE '\\' \
                     OR m.id IN (\
                       SELECT meeting_id FROM meeting_segments WHERE id IN (\
                         SELECT rowid FROM meeting_segments_fts WHERE meeting_segments_fts MATCH ?1\
                       )\
                     ) \
                     ORDER BY m.started_ms DESC, m.id DESC"
                ))?;
                let found = stmt.query_map(params![fts_query(q), escape_like(q)], map_summary)?;
                found.collect::<Result<Vec<_>, _>>()?
            }
            None => {
                let mut stmt = self.conn.prepare(&format!(
                    "SELECT {SUMMARY_COLS} FROM meetings m ORDER BY m.started_ms DESC, m.id DESC"
                ))?;
                let found = stmt.query_map([], map_summary)?;
                found.collect::<Result<Vec<_>, _>>()?
            }
        };
        Ok(rows)
    }

    /// Full detail for one meeting: segments ordered by `(start_ms,
    /// channel, id)`, speaker names ordered by key. `None` when `id` does not
    /// exist.
    pub fn meeting(&self, id: &str) -> Result<Option<MeetingDetail>, StoreError> {
        let found = self.conn.query_row(
            &format!(
                "SELECT {SUMMARY_COLS}, m.stt_provider, m.notes_json, m.notes_stale_ms \
                 FROM meetings m WHERE m.id = ?1"
            ),
            params![id],
            |r| {
                Ok((
                    map_summary(r)?,
                    r.get::<_, String>(12)?,
                    r.get::<_, Option<String>>(13)?,
                    r.get::<_, Option<i64>>(14)?,
                ))
            },
        );
        let (summary, stt_provider, notes_json, notes_stale_ms) = match found {
            Ok(v) => v,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(e.into()),
        };

        let mut seg_stmt = self.conn.prepare(
            "SELECT id, start_ms, end_ms, channel, speaker, speaker_override, text \
             FROM meeting_segments WHERE meeting_id = ?1 ORDER BY start_ms, channel, id",
        )?;
        let segments = seg_stmt
            .query_map(params![id], |r| {
                Ok(StoredSegment {
                    id: r.get(0)?,
                    start_ms: r.get(1)?,
                    end_ms: r.get(2)?,
                    channel: r.get(3)?,
                    speaker: r.get(4)?,
                    speaker_override: r.get(5)?,
                    text: r.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let mut spk_stmt = self.conn.prepare(
            "SELECT speaker_key, display_name, source, evidence_ms FROM meeting_speaker_names \
             WHERE meeting_id = ?1 ORDER BY speaker_key",
        )?;
        let speakers = spk_stmt
            .query_map(params![id], |r| {
                Ok(SpeakerName {
                    key: r.get(0)?,
                    display_name: r.get(1)?,
                    source: NameSource::parse(&r.get::<_, String>(2)?),
                    evidence_ms: r.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Some(MeetingDetail {
            summary,
            stt_provider,
            notes_json,
            notes_stale_ms,
            segments,
            speakers,
        }))
    }

    /// `(id, ended_ms)` of every meeting whose audio has not been evicted,
    /// for the storage-cap planner (`plan_eviction` in `hark-meeting`).
    pub fn meeting_audio_index(&self) -> Result<Vec<(String, Option<i64>)>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, ended_ms FROM meetings WHERE audio_evicted_ms IS NULL")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Ids with `ended_ms IS NULL` (the app died mid-meeting), for startup
    /// recovery.
    pub fn unfinished_meetings(&self) -> Result<Vec<String>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM meetings WHERE ended_ms IS NULL")?;
        let rows = stmt
            .query_map([], |r| r.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

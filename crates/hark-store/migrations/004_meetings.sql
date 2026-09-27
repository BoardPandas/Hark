-- 004: meetings, their transcript segments, renamed speakers and notes.
-- Audio itself lives on disk under <data_dir>/meetings/<id>/ (hark-meeting
-- owns that); this table tracks size and eviction state, not bytes.
--
-- Cascades (segments, speakers) rely on `PRAGMA foreign_keys = ON`, which
-- Store::init now sets on every connection.

CREATE TABLE meetings (
  id               TEXT PRIMARY KEY,          -- also the audio directory name, e.g. "20260927-143012"
  started_ms       INTEGER NOT NULL,
  ended_ms         INTEGER,                   -- NULL while recording or if the app died mid-meeting
  title            TEXT,
  app_hint         TEXT,
  trigger          TEXT NOT NULL,             -- manual | ask | auto
  stt_provider     TEXT NOT NULL,
  notes_json       TEXT,
  audio_bytes      INTEGER NOT NULL DEFAULT 0,
  audio_evicted_ms INTEGER,
  refined          INTEGER NOT NULL DEFAULT 0 -- 1 once the Deepgram final pass replaced the live segments
);

CREATE TABLE meeting_segments (
  id         INTEGER PRIMARY KEY,
  meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  start_ms   INTEGER NOT NULL,
  end_ms     INTEGER NOT NULL,
  channel    INTEGER NOT NULL,                -- 0 Me, 1 Them
  speaker    INTEGER,
  text       TEXT NOT NULL
);
CREATE INDEX idx_meeting_segments_meeting_start ON meeting_segments(meeting_id, start_ms);

CREATE TABLE meeting_speakers (
  meeting_id   TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  speaker      INTEGER NOT NULL,
  display_name TEXT NOT NULL,
  PRIMARY KEY (meeting_id, speaker)
);

-- External-content FTS5 index over segment text (rowid-aligned with
-- meeting_segments.id), kept in sync by the standard insert/delete/update
-- trigger trio. FTS5 is compiled into the bundled SQLite Hark ships
-- (verified in crates/hark-store/tests/meetings.rs); this migration does
-- not need a runtime fallback.
CREATE VIRTUAL TABLE meeting_segments_fts USING fts5(
  text,
  content='meeting_segments',
  content_rowid='id'
);

CREATE TRIGGER meeting_segments_ai AFTER INSERT ON meeting_segments BEGIN
  INSERT INTO meeting_segments_fts(rowid, text) VALUES (new.id, new.text);
END;

CREATE TRIGGER meeting_segments_ad AFTER DELETE ON meeting_segments BEGIN
  INSERT INTO meeting_segments_fts(meeting_segments_fts, rowid, text) VALUES ('delete', old.id, old.text);
END;

CREATE TRIGGER meeting_segments_au AFTER UPDATE ON meeting_segments BEGIN
  INSERT INTO meeting_segments_fts(meeting_segments_fts, rowid, text) VALUES ('delete', old.id, old.text);
  INSERT INTO meeting_segments_fts(rowid, text) VALUES (new.id, new.text);
END;

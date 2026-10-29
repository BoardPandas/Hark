-- 007: speakers keyed by speaker key, so "Me" and user-added speakers can be
-- named; per-line speaker corrections; and a stamp for notes that still credit
-- the old speaker. hark-meeting owns the key vocabulary:
--   'me' | 'them' | 'them:<u32>' | 'added:<u32>'
--
-- meeting_speakers is kept, frozen and unread by 007+ builds, so a build
-- rolled back to 006 still finds the table it queries. A later migration
-- drops it. The copy runs here, inside the migration transaction, so dormant
-- meetings migrate too (never lazily on read).
CREATE TABLE meeting_speaker_names (
  meeting_id   TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  speaker_key  TEXT NOT NULL,
  display_name TEXT NOT NULL,
  source       TEXT NOT NULL DEFAULT 'user',   -- 'user' | 'guess'
  evidence_ms  INTEGER,                        -- guess only: start of the line the name was heard in
  PRIMARY KEY (meeting_id, speaker_key)
);
INSERT INTO meeting_speaker_names (meeting_id, speaker_key, display_name, source)
  SELECT meeting_id, 'them:' || speaker, display_name, 'user' FROM meeting_speakers;

ALTER TABLE meeting_segments ADD COLUMN speaker_override TEXT;  -- NULL = as transcribed
ALTER TABLE meetings ADD COLUMN notes_stale_ms INTEGER;          -- NULL = notes match the speakers

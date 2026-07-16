-- 001: initial schema. Immutable once applied anywhere (BP FOUNDATIONAL
-- never-renumber-applied-migrations): fixes go in a new numbered file.

CREATE TABLE entries (
  id            INTEGER PRIMARY KEY,
  ts_ms         INTEGER NOT NULL,
  raw_text      TEXT NOT NULL,
  final_text    TEXT NOT NULL,        -- equals raw_text when no cleanup ran
  voice         TEXT NOT NULL,
  stt_provider  TEXT NOT NULL,
  stt_model     TEXT NOT NULL,
  cleanup_model TEXT,                 -- NULL when cleanup did not run
  stt_ms        INTEGER NOT NULL,
  cleanup_ms    INTEGER,              -- NULL when cleanup did not run
  total_ms      INTEGER NOT NULL
);
CREATE INDEX idx_entries_ts ON entries(ts_ms);

-- Exactly one row, keyed on the fixed id 1 (LL-G sqlite/upsert-by-name-collision:
-- upserts key on stable identifiers, never display names). Lifetime counters
-- survive history pruning and clears by design. The row itself is seeded at
-- open time because since_ts_ms needs the wall clock.
CREATE TABLE stats (
  id            INTEGER PRIMARY KEY CHECK (id = 1),
  dictations    INTEGER NOT NULL DEFAULT 0,
  words         INTEGER NOT NULL DEFAULT 0,
  audio_ms      INTEGER NOT NULL DEFAULT 0,
  stt_ms        INTEGER NOT NULL DEFAULT 0,
  cleanup_ms    INTEGER NOT NULL DEFAULT 0,
  since_ts_ms   INTEGER NOT NULL
);

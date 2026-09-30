-- Numeric dictation facts have their own lifecycle: transcript capture,
-- deletion and retention never remove them. Reset stats clears both tables.
-- No transcripts, invocation phrases, document titles or audio live here.
CREATE TABLE insight_events (
  id                     INTEGER PRIMARY KEY,
  ts_ms                  INTEGER NOT NULL,
  words                  INTEGER NOT NULL CHECK (words >= 0),
  audio_ms               INTEGER CHECK (audio_ms >= 0),
  total_ms               INTEGER NOT NULL CHECK (total_ms >= 0),
  stt_provider           TEXT NOT NULL,
  voice                  TEXT NOT NULL,
  invocation             INTEGER NOT NULL CHECK (invocation IN (0, 1)),
  expanded_words         INTEGER NOT NULL CHECK (expanded_words >= 0),
  spellbook_replacements INTEGER CHECK (spellbook_replacements >= 0),
  foreground_app         TEXT,
  legacy                 INTEGER NOT NULL DEFAULT 0 CHECK (legacy IN (0, 1))
);
CREATE INDEX idx_insight_events_ts ON insight_events(ts_ms);

-- Coverage starts when full numeric tracking begins, not at the oldest
-- surviving transcript. A partial history backfill cannot establish coverage.
CREATE TABLE insight_tracking (
  id          INTEGER PRIMARY KEY CHECK (id = 1),
  since_ts_ms INTEGER NOT NULL
);
-- Rust backfills retained entries inside this migration's transaction so its
-- Unicode whitespace counting exactly matches the existing lifetime counter.

-- 008: entries gains `delivered`, so History can say which dictations never
-- reached the cursor: the words were transcribed but the paste failed, or the
-- clip was retried later from the History page. 1 = pasted (every row written
-- before this migration was, because a row used to exist only after a
-- successful paste), 0 = kept so the words are not lost. Not a stats input:
-- undelivered rows are excluded from the lifetime counters and insights
-- (see `Store::record`).

ALTER TABLE entries ADD COLUMN delivered INTEGER NOT NULL DEFAULT 1;

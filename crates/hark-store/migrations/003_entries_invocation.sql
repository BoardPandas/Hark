-- 003: entries gains the invocation trigger phrase that produced the row,
-- so History can badge a dictation whose text was pasted from an invocation
-- rather than transcribed. Nullable with no DEFAULT and no backfill: every
-- pre-003 row is genuinely "not an invocation", which is exactly NULL.

ALTER TABLE entries ADD COLUMN invocation TEXT;

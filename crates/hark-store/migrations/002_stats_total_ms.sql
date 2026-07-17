-- 002: stats gains a total_ms sum (release-to-inject wall time) so the UI
-- derives average latency from real totals instead of an stt+cleanup
-- approximation that would silently omit encode and inject time.
-- Rows recorded before this migration contribute 0; the average is derived
-- from sums, so it reads slightly low on a pre-002 database and converges
-- as new dictations land.

ALTER TABLE stats ADD COLUMN total_ms INTEGER NOT NULL DEFAULT 0;

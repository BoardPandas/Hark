-- 006: meetings gains the state of its final-pass transcript, so a pass that
-- could not reach the provider survives a restart and the detail page can say
-- why a meeting has no transcript.
--   NULL               nothing outstanding (done, never configured, or unknown)
--   'pending'          waiting to retry; 'pending:<unix ms>' names the next try
--   'failed:<reason>'  stopped trying; <reason> is a short slug, never content
-- Nullable with no DEFAULT and no backfill: every pre-006 meeting is NULL.

ALTER TABLE meetings ADD COLUMN transcript_status TEXT;

# Specification: saved-meeting final-pass rerun

Status: Approved scope, implementing Polish item 1
Authority: [intent](intent.md) and [Meetings plan](../../tasks/2026-09-26-plan-meeting-transcription.md#polish)

1. Offer an explicit, confirmed Deepgram final pass on a completed meeting with retained audio. Name the upload, provider charges, transcript replacement, and speaker-name reset before starting.
2. Send the retained stereo MP3 unchanged, preserving Me-left/Them-right separation; when uncompressed, stream the original WAV tracks. Perform network work on a worker and protect the recording from eviction.
3. Prevent duplicate work and conflicting deletion or speaker edits. Missing credentials/audio, provider errors, and empty output preserve the current transcript.
4. Replace transcript segments and reset speaker names atomically; preserve notes and title. Report successful completion only after the storage worker confirms the commit.
5. Exercise path guards, unchanged upload bytes and content type, SQLite rollback/FTS, and storage acknowledgement with fixtures. Run the repository gates independently for this feature-only snapshot; native Windows UI behavior remains a separate validation step.

No config migration, dependency addition, detector change, hotkey, new export format, Gemini final pass, or AEC implementation is part of this diff.

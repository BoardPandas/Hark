# Intent: share useful meeting content in portable formats

Status: Approved scope
Approved by: product owner, 2026-09-28, as Polish item 4 in the request to complete items 1–6.

A person needs to share a useful part of a meeting, subtitle a recording, or open its notes in Word and other Windows apps. They should be able to select the relevant content without accidentally including unrelated notes or damaging the saved recording.

This delivery covers item 4 of the approved [Meetings plan](../../tasks/2026-09-26-plan-meeting-transcription.md#polish). The product owner explicitly approved decoding/re-encoding excerpts and docx-rs 0.4.22 with default features off, and authorized commit/push/tag operations owned by the parent task.

## Lessons Learned / Gotchas

See the plan's export notes: segment boundaries differ from word boundaries, companion files need their own overwrite protection, and native share sources require lifetime ownership.

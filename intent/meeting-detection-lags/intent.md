# Intent: react promptly to meeting microphone changes

Status: Approved scope
Approved by: product owner, 2026-09-28, as Polish item 2 in the request to complete items 1–6.

Meeting detection should react promptly when an app begins or ends microphone use, while preserving the existing deliberate start and stop delays and the ability to exit Hark reliably.

This delivery covers only item 2 of the approved [Meetings plan](../../tasks/2026-09-26-plan-meeting-transcription.md#polish). The product owner subsequently authorized committing, pushing, and tagging all prepared features; the parent task owns those operations.

## Lessons Learned / Gotchas

See the plan's registry-driven detection notes: notifications need separate timing deadlines and explicit shutdown when a watcher retains a command sender.

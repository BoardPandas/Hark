# Specification: notification-driven Windows meeting detection

Status: Approved scope, implementing Polish item 2
Authority: [intent](intent.md) and [Meetings plan](../../tasks/2026-09-26-plan-meeting-transcription.md#polish)

1. Observe recursive ConsentStore key and value changes without modifying that store. Rearm before queueing a coalesced coordinator wakeup.
2. Preserve timestamp-based five-second debounce and configurable auto-stop. Request fresh snapshots at their deadlines even when no further notification arrives.
3. Keep a ten-second backstop for browser title changes and missed notifications, two-second fallback polling when watching is unavailable, and 30-second watcher retries. The recording drain remains bounded by 100 ms.
4. Stop the watcher and coordinator explicitly with bounded waits; retaining a callback sender must not prevent shutdown. Keep unsupported-platform behavior explicit.
5. Test deadline edges, fresh snapshots, scheduling, sender retirement, and native registry rearming with isolated HKCU test keys. Run repository gates for this snapshot; real-call capture and detection remain separate native validation.

No meeting toggle chord, config migration, new export, Gemini final pass, or AEC change is included.

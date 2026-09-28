# Specification: optional meeting toggle shortcut

Status: Approved scope, implementing Polish item 3
Authority: [intent](intent.md) and [Meetings plan](../../tasks/2026-09-26-plan-meeting-transcription.md#polish)

1. Offer an optional Windows meeting start/stop chord, unassigned by default, editable and clearable in Settings → Meetings. A press toggles once; repeats, synthetic input, and release recovery do not toggle.
2. Share the existing Windows hook and chord trackers with dictation. Reject equal chords and either-direction subsets regardless of key order; permit distinct chords sharing modifiers. Preserve capture-tap, watchdog, and dictation lock-key invariants.
3. Route meeting events independently of a missing or busy dictation worker, wake the root UI while hidden, and decide start/stop on the meeting coordinator. Disabled or unsupported Meetings binds no toggle.
4. Migrate config v3 to v4 with the normal backup/save flow, adding only the optional chord. Preserve provider selection and user-chosen auto-stop values. Do not include the later Gemini enum, model, or schema 5.
5. Verify routing, invalid/conflicting/reordered chords, repeats, synthetic input, release healing, missing dictation, migration backup, and round trips with focused tests; run repository gates for this feature snapshot.

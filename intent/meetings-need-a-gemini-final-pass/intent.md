# Meetings need a Gemini final pass

Users with a Gemini key need an explicit alternative for improving a just-recorded
meeting without obtaining a Deepgram key. A cloud request must not silently change
providers, and independent five-minute responses must not imply reliable speaker
identity across an entire call.

Polish item 5 adds an opt-in Gemini Files path after Stop while preserving Deepgram
as the default and as the explicit saved-recording re-run action. User authorization
covers implementing, committing, pushing, and tagging all six Polish items; this
artifact scopes the independent 0.55.0 commit. The parent task owns delivery.

See the [specification](spec.md) and [meeting plan](../../tasks/2026-09-26-plan-meeting-transcription.md).

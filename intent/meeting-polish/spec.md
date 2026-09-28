# Specification: Meetings Polish

Status: Approved scope; explicitly open choices remain pending
Authority: product owner's 2026-09-28 handoff, "complete them all 1-6", and later "commit and push and tag all" authorization
Plan: `tasks/2026-09-26-plan-meeting-transcription.md`, section 5, Polish

1. A saved meeting can run Deepgram refinement again using its retained stereo
   MP3 unchanged (Me left, Them right). Perform uploads on a worker. Prevent
   duplicate work and protect in-use audio. Missing audio, missing credentials,
   provider errors, and empty output preserve the existing transcript. Clearly
   disclose the upload and replacement; preserve notes unless explicitly changed.
2. Windows detection responds to ConsentStore registry changes, with a slow
   polling backstop. Observation timestamps, the five-second debounce, and the
   configurable auto-stop delay retain their meanings. Shutdown is bounded;
   unsupported platforms keep their existing behavior.
3. A configurable meeting toggle chord uses the existing chord tracker, never
   conflicts with push-to-talk, fires once per press, and ignores synthetic
   events. Migrate persisted settings with a backup when changing schema.
4. Add tested SRT/VTT renderers, selected transcript/audio excerpts, DOCX export,
   and the native Windows share sheet. File/network work stays off the main
   thread; Windows share UI stays on it. The user approved decoding/re-encoding
   excerpts and `docx-rs` 0.4.22 with default features disabled on 2026-09-28.
5. Offer a Gemini Files final pass without a Deepgram key. Process one channel
   per request in at most five-minute windows; use explicit per-window speaker
   identities unless reconciliation is demonstrated. Clamp times to audio,
   validate responses, and attempt remote file deletion after success or failure,
   reporting cleanup failures and crash/retention limits.
   Preserve the old transcript unless the complete pass succeeds.
6. Measure both `aec3` and `webrtc-audio-processing` with the same reproducible
   fixtures and far-end reference. Deliver a real-speakers test procedure and
   results; do not select or enable a production engine before the user's choice.

## Verification and delivery

Use meaningful pure/fixture and HTTP-boundary tests; run the two npm guards,
format check, WSL workspace clippy and tests. Windows-specific behavior requires
Windows CI compilation and native user validation; neither is inferred from
Linux results. Deliver each feature in a separate minor-version commit after
the existing blanket authorization. Do not stage either AGENTS filename.

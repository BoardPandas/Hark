# Specification: Meetings Polish

Status: Approved scope; AEC choice delegated and selected on 2026-09-28
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
6. The reproducible comparison is complete. The owner's later instruction
   delegates engine selection and waives further real-speaker testing before
   shipping. Use pinned Rust `aec3` 0.4.0 behind an opt-in "Reduce speaker echo"
   setting (schema 6, default false, applied at the next meeting). Process only
   the microphone on the meeting worker in 16 kHz / 10 ms frames before both
   spool and live transcription. Keep the captured system track unchanged.
   Bound microphone waiting to 250 ms of audio and reference retention to two
   seconds. Preserve original microphone audio on missing reference or engine
   failure, reset adaptation after discontinuities, and preserve every final
   partial frame. Compensate the verified 128-sample processing latency and
   retain those original microphone samples for stop/failure fallback; equal
   output length alone is not proof that ending audio survives. Reuse existing session sample placement and the engine's
   automatic delay estimator; do not claim common hardware-clock alignment or
   active drift correction. Synthetic attenuation and headphone listening are
   supporting evidence, not verified real-speaker intelligibility.

## Verification and delivery

Use meaningful pure/fixture and HTTP-boundary tests; run the two npm guards,
format check, WSL workspace clippy and tests. Windows-specific behavior requires
Windows CI compilation and native user validation; neither is inferred from
Linux results. Deliver each feature in a separate minor-version commit after
the existing blanket authorization. Do not stage either AGENTS filename.

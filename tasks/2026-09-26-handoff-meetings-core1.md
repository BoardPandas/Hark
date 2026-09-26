# Handoff: Meetings feature, Core step 1 (capture + spool + session logic)

**Date:** 2026-09-26. Load this as the only starting context for the next session.
**Canonical plan:** `tasks/2026-09-26-plan-meeting-transcription.md`. Read §3 (decisions, all
locked), §4.1–§4.2, §4.9 rule 3, §5 Foundation "CP0 results", and §8 "Learned in CP0" before
starting. Do not re-open locked decisions.

## Decisions that changed in CP0 (both approved 2026-09-26)

- **D1:** Windows system audio = **per-process loopback**, in Core. A detected meeting captures
  the meeting app's process tree (include mode); a manual start captures everything except Hark
  (exclude mode). Endpoint loopback + QPC gap padding is only the fallback for builds < 19041.
- **D2:** the final pass is **Deepgram** (one multichannel+diarize request), using its own key
  (keychain account `deepgram`), independent of the dictation provider. The product owner
  dictates with Gemini. No Deepgram key → keep the live Me/Them transcript.

## Repo state

- `main` @ `0.47.2` plus whatever the CP0 wrap-up commit adds (plan + this handoff).
- Branch `spike/meetings-cp0` holds the CP0 spike code. **It is never merged**, but it is the
  reference implementation for this step:
  - `crates/hark-audio/examples/process_loopback.rs`: per-process loopback activation
    (`ActivateAudioInterfaceAsync`, `#[implement]` completion handler, 16 kHz mono i16 via
    `AUTOCONVERTPCM`, event loop). Port it; do not re-derive it.
  - `crates/hark-audio/examples/meeting_capture.rs`: comms-mic capture, endpoint loopback, the
    QPC gap-event log and the drain-side splice, CPU/RSS probes.
  - `crates/hark-audio/examples/consent_dump.rs`, `mp3_smoke.rs`: for later steps (§4.8, §4.11).
  - `crates/hark-stt/examples/deepgram_multichannel.rs`: the final-pass request (Core step 3).
- `agents.md` has unrelated uncommitted changes from before this work. Leave them alone.

## Scope of Core step 1 (plan §5 Core, step 1)

Work on a new branch from `main` (e.g. `feat/meetings-core1`). Pure logic first, glue second:

1. **New crate `hark-meeting`** (pure, no I/O, `hark-pipeline::state` style):
   - Session state machine `Idle → Recording → Finalizing → Summarizing → Done | Failed`.
   - Chunker per channel: cut at the quietest 300 ms between 20 s and 30 s, hard cut at 30 s,
     chunks ≥ 10 s, skip no-energy chunks (loudest-100 ms-window rule from `hark-audio/CLAUDE.md`,
     never a whole-chunk mean).
   - Merge: segments ordered by chunk start sample offset on the shared session timeline.
   - Unit tests on synthetic PCM, asserting sample counts, never wall-clock time.
2. **`hark-audio` spool writer:** append 16 kHz mono i16 to `<data_dir>/meetings/<id>/{me,them}.wav`,
   patch the header on close, and a startup recovery pass that fixes the header of a spool whose
   meeting never finalized. Test against a temp dir.
3. **`hark-audio` per-process loopback** (`capture_win.rs` neighbour, e.g. `loopback_win.rs`):
   own thread + own MTA apartment; target = `IncludeTree(pid)` | `ExcludeTree(pid)`; frames into a
   ring the meeting drain reads. `UnsupportedPlatform` off Windows (Linux must still compile, D4).
4. **Endpoint fallback** (< 19041 only): cpal loopback on the default output device plus the QPC
   gap padding from the spike. Lowest priority; it can slip to a later step.

Mic for meetings = the eCommunications default (`communications_default_device()`), a second
shared-mode stream beside PTT's. CP0 proved the two coexist.

## Gotchas from CP0 (all in plan §8)

- **`PROPVARIANT` has a `Drop` in windows 0.62** (`PropVariantClear`). The activation params blob
  points at Rust memory: wrap the PROPVARIANT in `ManuallyDrop`, or the process dies silently.
- `#[implement]` needs `windows-core` as a direct dependency (already in the tree transitively).
- `GetMixFormat` is unsupported on the process-loopback virtual device: pass a 16 kHz mono i16
  `WAVEFORMATEX` with `AUTOCONVERTPCM | SRC_DEFAULT_QUALITY`.
- Endpoint loopback sends **no packets** during silence, and each resume raises
  `DATA_DISCONTINUITY` (cpal `Xrun`): not an error. cpal's capture timestamp is absolute QPC.
- Default render ≠ communications render on the product owner's box (Realtek vs LG monitor):
  why per-process loopback won.
- Later steps: `mp3lame-encoder` `*_to_vec` must be preceded by `reserve(max_required_buffer_size)`
  (LAME treats 0 as unbounded); Deepgram diarizes channel 0 too, so ignore `speaker` there.

## Before writing code (repo rules)

- RULE 1: LL-G `kb/rust` + `kb/windows` HIGH entries (checked 2026-09-26: no WASAPI/COM/registry
  entries exist; apply `join-on-drop-sender-field-order` to any session struct with worker threads,
  and `gui-subsystem-console-child-window` if anything spawns a process).
- Capture threads own their COM apartment; never block or allocate in a cpal callback.
- Never log raw audio, transcripts or keys. Meeting records get no `Debug` impl.
- Done = `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `npm run check:claude`,
  and a Linux compile check of the new code paths.
- Commits need the CHANGELOG + version bump staged **before** `git commit` (PreToolUse hook).

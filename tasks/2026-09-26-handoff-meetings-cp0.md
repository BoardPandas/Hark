# Handoff: Meetings feature, Foundation CP0 (Windows capture spike)

**Date:** 2026-09-26. Load this as the only starting context for the next session.
**Canonical plan:** `tasks/2026-09-26-plan-meeting-transcription.md`. It was APPROVED on
2026-09-26 with decisions D1–D9 locked. Read its §3 (decisions), §4.1 (capture), §4.8
(detection), §5 Foundation and §7–§8 (risks, gotchas) before starting. Do not re-open locked
decisions.

## Repo state

- `main` @ `0.47.2`: the plan and this handoff were committed and pushed as a docs commit on
  2026-09-26 (parent `c94e4bd`).
- `agents.md` had uncommitted changes from before this planning session. They are not part of
  this work and were left out of the commit; leave them alone unless the user says otherwise.
- This machine builds, tests and lints only. The app runs on real hardware; this is the Windows box.

## What CP0 must answer (go/no-go gate for Core)

CP0 is a **throwaway spike** on its own branch (e.g. `spike/meetings-cp0`). It is not merged. Its
output is the numbers, written back into the plan's §5 Foundation and §8 Lessons.

1. **Dual capture for 60 min.** A throwaway binary (an example under `crates/hark-audio/examples/`
   is fine) opens the mic (the eCommunications default) and **WASAPI loopback** on the default
   render device via cpal 0.18.2: `build_input_stream` on the *output* device with
   `default_output_config()`. Resample both to 16 kHz mono i16 and append to `me.wav` / `them.wav`.
2. **Measure:** CPU %, RSS, drift between the two channels over 60 min, and above all **whether
   loopback delivers no packets while nothing is playing**. If it doesn't, the Them timeline
   silently compresses and needs gap padding from the device position / wall clock (plan §7 row 1).
3. **PTT coexistence:** with the spike running, run Hark normally and dictate. Confirm a second
   shared-mode stream on the same mic does not break or starve either consumer.
4. **Deepgram multichannel:** interleave the two WAVs into one stereo WAV and send one
   pre-recorded request with `model=nova-3&multichannel=true&diarize=true&utterances=true&smart_format=true`.
   Check the Me/Them split and the speaker split within Them. **Re-verify Deepgram's current
   pre-recorded pricing and diarization billing on deepgram.com**: the plan's figures came
   partly from third-party roundups.
5. **ConsentStore dump** during a real Teams call, a Zoom call and a Google Meet tab:
   `HKCU\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone`.
   Confirm:
   - new Teams sits under its package family (`MSTeams_8wekyb3d8bbwe`), and Zoom/Chrome/Edge under `NonPackaged\<path with #>`;
   - `LastUsedTimeStop == 0` while in a call;
   - how many seconds it takes to flip at call end;
   - **Hark's own entry reads as permanently in use** (its pre-roll stream is always open).
   Read the registry in-process (`windows` crate `Win32_System_Registry`, or `winreg`, which
   `hark-autostart` already uses). **Never shell out to `reg.exe`/PowerShell**: the GUI-subsystem
   binary flashes a console (LL-G HIGH `gui-subsystem-console-child-window`).
6. **MP3 smoke test (small):** add `mp3lame-encoder` 0.2.4 to the spike only and confirm it builds on
   this machine and in the Windows CI toolchain (vendored LAME via `cc`). Encode 1 min of
   `them.wav` at 32 kbps mono and 1 min stereo at 64 kbps with mode `STEREO`. Note the build time
   and binary size delta. Watch for WDAC os error 4551 on build scripts (see gotchas below).

**Exit:** write the numbers into the plan, then go/no-go on D1–D3. On go, the next session starts
Core step 1.

## Before writing code (repo rules)

- RULE 1: fetch the LL-G index + `kb/rust` + `kb/windows` and read the HIGH entries. The ones already
  known to apply: GUI-subsystem console child window; `JoinHandle` joined in `Drop` deadlocks with
  sender fields (declare senders before thread handles); reqwest multipart masks transport errors;
  blocking `std::fs` on Tokio.
- Capture threads own their COM apartment (`crates/hark-audio/CLAUDE.md`). Loopback follows the
  same rule on its own thread.
- Never log raw audio, transcripts or API keys. Spike WAVs contain real speech: keep them out of
  git (gitignored scratch path) and delete them after measuring. Do **not** commit a Deepgram
  response fixture unless the speech content is scrubbed.

## Gotchas carried from earlier sessions

- **WDAC blocks stale proc-macro/build-script DLLs** on this box (`os error 4551`). Don't
  `cargo clean` casually. If needed, clean the leaf crate too. Release builds happen on the CI
  Windows runner.
- cpal resolves the default capture device by the eConsole role only. Teams uses
  **eCommunications**. `hark-audio` already queries it (`crates/hark-audio/Cargo.toml:22-26`).
- A commit needs the CHANGELOG + `package.json` bump staged **before** `git commit` (PreToolUse
  hook). The spike branch is not merged, so this only matters if the plan/handoff docs get committed to `main`.

# Meetings: stop recording silence, survive slow networks

Status: **capture half implemented** (Foundation, Core: capture, Settings hint; the
`Docs/features/MEETINGS.md` capture bullets and the `mic_device` row in
`Docs/core/CONFIGURATION.md` were corrected with it), released as 0.62.0. Hardware row 1
passed on the owner's Teams call (2026-10-14: both tracks had audio); rows 2-4 are still to
run. Slow networks and the detail page belong to a parallel session.
Baseline `8ab1abc` (0.61.6). Written 2026-10-14.
Target release: 0.62.0 (Minor: new behaviour plus fixes; one additive DB column).

## Why: what actually happened (diagnosed 2026-10-14)

The owner reported that a 40-minute Teams meeting (`20261014-151225`) got no transcript
"because my internet cut out". The cause was **not** the network. Evidence:

- The recording was kept (19.5 MB MP3, full 40 m 34 s) but it is silence. Both tracks
  logged "82/83 empty chunks skipped", so nothing was sent live, and the Deepgram re-run
  correctly returned nothing.
- In the call still in progress (`20261014-160352`), `me.wav` was **100% zero samples** and
  `them.wav` peaked at 1 LSB.
- **Every** empty Teams meeting (Oct 7 ×2, Oct 12 ×2, Oct 14 ×2) opened the
  *Audeze Maxwell2 Chat* mic. Every Teams meeting that worked (Sep 27 to Oct 8) opened the
  *Shure MV7+*.
- A live probe during the call (Core Audio sessions plus 6 s level reads) found:
  - Teams' capture session is on the **Shure MV7+**. Hark opened the Windows
    *communications default* (Audeze Chat), which is muted: −120 dBFS.
  - Teams' render session (`ms-teams.exe` 25124, a child of root 22700) is on
    **Speakers (Chat-Audeze)**, the communications default render. The console default is
    *Realtek Digital Output*.
  - Process loopback `IncludeTree(22700)` measured −96.7 dBFS, which is idle.
    **Endpoint loopback of Speakers (Chat-Audeze) peaked at −5 dBFS**: the call audio is
    there.
  - Microsoft documents process loopback as endpoint-independent, but measured on this
    machine it hears nothing from Teams on the communications endpoint. This is
    undocumented; treat it as empirical.

So there are two capture bugs: the wrong mic and a deaf loopback. There is also one gap
that made them invisible: nothing warns the user that a track has been silent for
40 minutes. The slow-network concerns the owner raised are real too (live chunks are
dropped on timeout, and the first final pass uploads ~115 MB/h of WAV with no retry), so
they are in scope.

Silent recordings cannot be recovered. Six past meetings have no usable audio.

## Foundation

- [x] **Probe reports endpoints per app** (`hark-meeting/src/probe/win.rs`, `detect.rs`).
  - `capturing_pids` keeps the endpoint: id plus friendly name via
    `PKEY_Device_FriendlyName`, the same pattern as `capture_win::query_communications_default`.
  - `MicUse` gains `capture_endpoints`.
  - Add a render-side twin: active render sessions per endpoint for a PID *tree*. Do the
    descendant walk over `processes()` in pure, fixture-tested code in `detect.rs`.
  - Linux and macOS fill the fields empty.
- [x] **Endpoint loopback** (`hark-audio/src/loopback/`).
  - Add `LoopbackTarget::Endpoint { id }` in raw WASAPI inside `loopback/win.rs`:
    `IMMDeviceEnumerator::GetDevice` → `IAudioClient`, `AUDCLNT_STREAMFLAGS_LOOPBACK`,
    same 16 kHz mono i16 plus `AUTOCONVERTPCM`. No PROPVARIANT is involved.
  - **Pad gaps from QPC capture timestamps** (LL-G HIGH "WASAPI loopback sends nothing during silence").
    Endpoint loopback sends no packets while nothing plays; without padding, the Them
    track drifts out of sync with Me.
  - Linux and macOS match arms return `LoopbackError::Unsupported`.
  - Hand check: extend `examples/loopback_smoke.rs` with `--endpoint NAME`.
- [x] Turn this session's throwaway probes into one committed hand-check example,
  `examples/meeting_devices.rs`: endpoints, roles, sessions per PID, and levels only (no
  samples).

## Core: capture

- [x] **Mic choice** (`coordinator.rs` ~484). Order:
  1. explicit `meeting.mic_device`;
  2. the endpoint the detected meeting app is capturing from (prefer its active session);
  3. the communications default, as today.

  Log which rule chose it. For a manual start that later adopts a detected call, re-check
  only at start; do not hot-swap mid-recording.
- [x] **Them choice** (`loopback_target`, `coordinator.rs` ~586).
  - If the app's active render session is on a non-console-default endpoint, use
    `Endpoint { id }`.
  - Otherwise use `IncludeTree` as today. Process loopback is kept where it works because
    it excludes other apps' sounds.
  - Log the decision.
- [x] **Silence watchdog** (`recorder.rs` `Track::place` → `pump()` → `notice`).
  - Track the loudest 100 ms window per track (LL-G HIGH "Per-process loopback idles near -96 dBFS": use a level floor, never exact
    zero; process loopback idles at ~−96 dBFS).
  - Mic below −85 dBFS for 45 s → one live notice: "Hark hears nothing from <mic>. Teams
    is using <other mic>; switch in Settings > Meetings."
  - Them below −85 dBFS for 120 s while the app has an active render session → one notice.
  - Name the right device when the probe knows it.

## Core: slow networks

- [ ] **Smaller upload**: in `finish.rs`, compress to MP3 *before* the final pass and
  upload the archive, as `rerun.rs::open_audio` already does. A 40-minute call drops from
  155 MB to 19 MB, 8× less on cellular. Keep the WAV path when `compress_audio` is off.
- [ ] **Silent-recording precheck** before any upload, in both finish and rerun: if both
  channels stay under the floor, skip the request and say "This recording is silent; Hark
  captured no audio from either side." Never let it read as "Deepgram returned no
  transcript" again.
- [ ] **Pending transcript survives failure** (`hark-store`, additive migration).
  - Add `transcript_status TEXT NULL` (`pending` | `failed:<reason>` | NULL).
  - On a network or timeout failure, mark it `pending` and retry with backoff
    (1, 5, 15, 60 min, then daily, max 6) from a worker. Also retry on app start, reusing
    the `refinish` / `recover()` path.
  - Classify errors through the io source chain (`meeting.rs:189-236`); never retry 4xx.
- [ ] **Connect timeout**: the final pass uses its own client with a 15 s connect timeout.
  `shared_client` has 3 s, too short for a cellular TLS handshake. The 900 s total stays.
- [ ] **Live chunks back off** (`live.rs`).
  - After 3 consecutive failures, pause live uploads for 2 min, so Hark stops competing
    with the call for a weak uplink.
  - Hold up to 5 min of failed chunks and resend them after the next success.
  - Show the notice "Live transcript paused (slow connection). The full transcript is
    made after the call."

## Polish

- [ ] **Detail page** (`ui/meetings/detail.rs`).
  - With no transcript, the primary button is **Transcribe recording**; with one, it stays
    **Re-run final pass**.
  - Show the persisted status ("Waiting for a connection, retrying at 4:35 PM", "Silent
    recording", "No Deepgram key") instead of the in-memory one-shot string.
  - Keep the real `SttError` category instead of discarding it (`rerun.rs:91`).
- [x] **Settings hint**: replace "The communications default is the microphone Teams and
  Zoom use" (`ui/settings/meetings.rs`), which is false on this machine. The default entry
  becomes "Automatic: the mic your call app is using".
- [ ] Docs (`Docs/` AUTOGEN pages for meetings) plus the Unreleased changelog.

## Verification

- [ ] `npm run check:claude`, `cargo fmt --all -- --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test --workspace`.
- [ ] Unit tests: endpoint matching on fixtures, watchdog thresholds with
  `Source::Test`, gap padding by sample count, retry and backoff state machine, migration.
  (Capture half done: `detect.rs` them_source / process_tree / capture_endpoint fixtures,
  `loopback/gaps.rs`, `meeting/watchdog.rs`, and the recorder watchdog test.)
- [ ] **Hardware matrix on this machine (owner on a real call):**

  | # | Setup | Expect |
  |---|---|---|
  | 1 | Teams, Audeze Chat out, Shure in | both tracks have audio |
  | 2 | Teams with every device on default | process loopback still chosen and working |
  | 3 | Slack or Chrome call | unchanged |
  | 4 | Mute the mic for 60 s | watchdog notice appears |
  | 5 | Drop Wi-Fi mid-call, then end the call | `pending`, retried, transcript arrives |
  | 6 | Quit Hark while `pending`, relaunch | retried on start |

## Workaround until this ships

1. Hark **Settings > Meetings > Microphone** → `Microphone (Shure MV7+)`. This fixes Me.
2. For Them, either set Teams' speaker to the Windows default output, or make
   *Speakers (Chat-Audeze)* the Windows default output. This is likely but untested,
   because the loopback cause is empirical.

## Lessons Learned / Gotchas

- **Endpoint loopback's silence margin must not become leading silence.** The gap filler
  places silence 300 ms behind the clock (a packet still in the 200 ms buffer must never
  land on silence already placed). The recorder aligns a late track from "captured" frames,
  so endpoint loopback reports frames due by the clock (`captured_frames`), not the ring
  count; otherwise every Them line would be shifted 300 ms late.
- **A resumed endpoint stream flags `DATA_DISCONTINUITY` on its first packet.** Nothing
  was lost (the silence was placed), so it is not counted: it would reset the echo
  reducer after every pause.

- **The communications-default mic is not "the mic Teams uses."** Teams has its own device
  setting. Ask Core Audio which endpoint the app's capture session is on.
- **Process loopback can be deaf to an app on a non-default endpoint** even though the docs
  say otherwise (Windows 11 26300, new Teams, 2026-10-14). Endpoint loopback of that device
  heard the call at −5 dBFS. → `/add-lesson` (rust or windows shelf).
- **A silent track must be loud in the UI.** Six meetings over a week recorded nothing, and
  the only trace was an INFO log line. Any capture path needs a live level watchdog.
- **"Provider returned nothing" usually means "we sent nothing."** Check the audio before
  blaming the provider or the network.

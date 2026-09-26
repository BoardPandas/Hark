# Plan — Meeting Transcription ("Hark Meetings")

**Created:** 2026-09-26
**Status:** APPROVED by the product owner on 2026-09-26, including D1–D9. Next: Foundation CP0.
Handoff: `tasks/2026-09-26-handoff-meetings-cp0.md`.
**Model:** Krisp AI Meeting Assistant (<https://krisp.ai/meeting-transcription/>,
<https://help.krisp.ai/hc/en-us/articles/8214720684956-AI-Meeting-Assistant-overview>)

---

## 1. Goal

Record a meeting in any app (Teams, Zoom, Meet in a browser, Slack huddle, in person) without
adding a bot, and produce:

1. A transcript with timestamps, labelled **Me** and **Them**, and later **Speaker 1/2/3** within Them.
2. A summary, key points, decisions and action items from the user's own BYOK LLM.
3. A local, searchable **Meetings** page. Nothing leaves the machine except the calls to the
   user's own STT and LLM providers.
4. **Auto-detect from the first version:** when Teams, Zoom, etc. start using the mic, Hark offers
   to take notes (or starts on its own, per setting) and stops when the call ends.
5. **Recordings kept under a user-set storage cap (default 5 GB).** Once the cap is reached, the
   oldest recording's audio is deleted to make room. Transcripts and notes are never evicted.
6. **Share a meeting with other people:** export the notes and transcript as Markdown or plain
   text (or copy them), and export the audio as a small file anyone can double-click to play.

Push-to-talk dictation keeps working, unchanged, while a meeting is being recorded.

## 2. How Krisp does it vs how Hark will

| Concern | Krisp | Hark |
|---|---|---|
| Remote audio capture | Its own **virtual mic + speaker driver**. The user must select "Krisp Speaker/Microphone" inside Teams, or Krisp hears nothing. That is the whole point of the Teams setup article. | **OS loopback capture** (WASAPI loopback on Windows, Core Audio process tap on macOS; Linux deferred). No driver and no per-app setup. Works on whatever device the meeting app uses. |
| Me vs Them | Diarization | **Free and exact:** mic = Me, loopback = Them. Diarization only has to split *Them*. |
| Start/stop | Manual "Note Taker" toggle, plus calendar one-click join | **Auto-detect in Core:** "Teams is using your mic. Take notes?" or auto-start, then auto-stop when the app releases the mic. Manual start via tray / Meetings page is always available. |
| STT | Krisp's cloud or on-device model | User's BYOK provider (Deepgram recommended) or on-device Parakeet |
| Summary | Krisp's cloud LLM, templates | User's BYOK LLM via `hark-voice` transport |
| Storage / sharing | Krisp account, cloud sync, share links, calendar | Local SQLite only. **Share by file:** notes + transcript as `.md` / `.txt` / clipboard, audio as `.mp3` (§4.10). The user sends the file over email, Teams or Slack. **No calendar, sync or share links:** they need a hosted backend Hark does not have. |

## 3. Decisions

| # | Decision | Proposal | Status |
|---|---|---|---|
| D1 | Capture mechanism | OS loopback, two independent streams (mic, system). No virtual driver. | proposed |
| D2 | Default transcription path | **Live:** rolling ~20–30 s chunks per channel through the existing `SttProvider`, so Me/Them lines appear during the call. **After stop:** if the provider is Deepgram, one full-file `multichannel=true&diarize=true&utterances=true` pass replaces the live Them lines with Speaker 1/2/3. | proposed |
| D3 | Raw audio on disk | **Keep recordings under a circular storage cap** set in Settings (default **5 GB**). When a new recording would push the total over the cap, delete the **oldest recordings' audio** until it fits. Transcripts/notes stay. A cap of **0 = don't keep audio**, meaning audio is deleted as soon as the transcript is saved. Full rules in §4.9. | **locked 2026-09-26** |
| D4 | Platform order | **Windows first, macOS once Windows is good; Linux deferred** (parity is not a goal for now). macOS needs a signed build + TCC key and a real-Mac spike. Meeting mode does *not* need the unimplemented macOS hotkey seam, so macOS can get Meetings before PTT. Linux must still **compile** (release CI builds .deb/.rpm/PKGBUILD): meeting capture and detection return `UnsupportedPlatform` there, the same way `hark-hotkey` does on macOS, and the UI hides Meetings. | **locked 2026-09-26** |
| D5 | Auto-detect in MVP? | **Yes, it ships in Core.** Mode `off / ask / auto`, default **ask**, with auto-stop. Full rules in §4.8. | **locked 2026-09-26** |
| D6 | Mic bleed (user on speakers) | Core: recommend headphones and flag it in the UI. Polish: AEC with the loopback as far-end reference (`aec3` pure-Rust vs `webrtc-audio-processing` bake-off). | proposed |
| D7 | Summary call shape | One long-context call (60 min ≈ 9–10k words ≈ 13k tokens fits every current model). No map-reduce. | proposed |
| D8 | Shareable audio format | **MP3, 32 kbps mono** (~14 MB per hour: a 1-hour meeting fits under a 25 MB email limit) via `mp3lame-encoder`. Plus **WAV** as the "original quality" option, which needs no new dependency (`hound` is already locked). The product owner only requires "a file people can easily play" (2026-09-26); the format is Claude's pick. Rationale in §4.10. | approved 2026-09-26 |
| D9 | Compress kept recordings | **In Core.** Once the final pass and summary are saved, re-encode the WAV spools to **one stereo MP3** (L = Me, R = Them, 64 kbps, ~29 MB/h), verify it, then delete the WAVs. A 5 GB cap then holds **~170 h instead of ~21 h**. The MP3 encoder is already coming in for D8. Rules in §4.11. | **locked 2026-09-26** |

### 3.1 Provider fit for a 60-min, 2-channel meeting

Research on 2026-09-26. Several price points came from third-party roundups, so **re-verify
them on the vendors' pricing pages before quoting them in the UI.**

| Provider / mode | ~Cost | Diarization | Live | Length limit | Verdict |
|---|---|---|---|---|---|
| Deepgram pre-recorded nova-3, multichannel | ~$0.31 | native, no surcharge on batch | no | 2 GB file | **Default final pass** |
| Deepgram / any adapter, rolling chunks | ~2× per-minute rate | none (Me/Them from channel) | yes (~30 s lag) | per chunk | **Default live path** |
| On-device Parakeet, rolling chunks | $0 | none | yes | per chunk | Works today; CPU cost to measure in CP1 |
| Gemini Files API, prompted diarization | < $0.10 | prompted, weaker | no | 9.5 h | Polish: "cheap BYOK" final pass |
| AssemblyAI Universal-Streaming | ~$0.42 | native, live | yes | session | Later: only if true streaming is wanted |
| OpenAI gpt-4o-transcribe-diarize | $1–2 | yes, but identity breaks across chunks | — | **~1400 s hard cap** | **Do not build on it** |
| Gemini Live / 3.5 Transcribe Live | — | prompted | yes | 10–15 min per session | Needs stitching; not for meetings |
| Groq Whisper | — | none | — | 25 MB | Out of scope for meetings |

## 4. Architecture

Threading rule unchanged: the UI owns the main thread, and every capture/STT/LLM/disk task runs
on worker threads. Meeting mode gets its **own coordinator and channels** and never shares the
PTT pipeline's state machine (`hark-pipeline/src/state.rs` is a one-shot, 4-state
press→release machine and should stay that way).

```
mic thread ─┐                       ┌─ chunker(Me) ──► SttProvider ─┐
            ├─► 16 kHz PCM spool ───┤                               ├─► merge by t ─► UI live pane
loopback ───┘   (disk, per channel) └─ chunker(Them) ► SttProvider ─┘        │
                                                                              ▼
detector (poll 2 s) ─► ask / auto-start ─► Recording ─► mic released > N s ─► Stop
Stop ─► finalize spool ─► [Deepgram multichannel+diarize pass] ─► segments ─► summarize ─► hark-store
                                                                                       └► enforce storage cap (§4.9)
```

### 4.1 Capture: `hark-audio`

- **New `loopback` module per OS**, beside `capture_win.rs`:
  - **Windows:** cpal 0.18.2 (the version already locked) opens WASAPI loopback when
    `build_input_stream` is called on an *output* device. Take the format from
    `default_output_config()`, because `supports_input()` is false there. Polish: per-process
    loopback (`AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK`, Win10 2004+) through the `windows`
    crate `hark-audio` **already depends on** (`Win32_Media_Audio`). That captures only the
    meeting app, not YouTube or notification sounds.
  - **macOS:** `AudioHardwareCreateProcessTap` + private aggregate device via `objc2-core-audio`,
    macOS 14.4+. Needs `NSAudioCaptureUsageDescription` in Info.plist and a stably signed binary.
    ScreenCaptureKit is the fallback, but it carries the heavier screen-recording prompt.
  - **Linux (deferred per D4, notes kept for later):** the PipeWire/Pulse monitor source of the default sink. Verify then whether
    cpal 0.18's pipewire/pulseaudio host features expose it. If not, use `libpulse-binding`.
- **Mic for meetings follows the eCommunications default.** That is the device Teams uses.
  `hark-audio` already queries it for the device picker; see `Cargo.toml:22-26`.
- **Continuous spool, not the ring.** The ring (`ring.rs`, capacity from `window.rs:62-65`,
  `max_hold_s: 120`) is sized once for a bounded press. Meeting mode drains resampled 16 kHz
  i16 frames to append-only files at `<data_dir>/meetings/<meeting_id>/{me,them}.wav`, plus an
  in-memory tail for the live chunker. The WAV header is patched on close; on startup a
  crash-recovery pass fixes the header of any spool whose meeting never finalized. Each channel
  costs 115 MB/h, both together 230 MB/h. That is well under Deepgram's 2 GB cap. The WAVs only
  live until the meeting is processed; they are then compressed to one ~29 MB/h stereo MP3 (§4.11).
- **Separate streams from PTT.** WASAPI/CoreAudio/Pulse shared mode allows two input streams on
  one device in one process, so PTT keeps its own stream and ring untouched. Confirm this in CP0.

### 4.2 Session: new crate `hark-meeting`

- Pure state machine, tested without I/O like `hark-pipeline::state`:
  `Idle → Recording → Finalizing → Summarizing → Done | Failed(reason)`.
- **Chunker:** per channel, cut at the quietest 300 ms window between 20 s and 30 s, hard cut at
  30 s. Chunks are ≥10 s, so Groq's 10 s minimum is not wasted. Chunks with no energy (the
  loopback during your own monologue) are skipped. Each chunk goes through the configured
  `SttProvider` on a small worker pool, sequential per channel.
- **Merge:** segments ordered by the chunk's start sample offset. Both channels are timestamped
  from a shared session `Instant`, so clock drift between devices only matters for AEC, not for
  ordering.
- **Final pass (D2):** interleave the two spools into one stereo 16 kHz WAV and send one
  Deepgram request. Keep the live segments if it fails; a failed refine never loses the meeting.
- Spelling fixes in the transcript reuse `hark-spellbook` phonetic correction. **Invocations
  never fire in meeting mode:** they are a dictation-only control-flow feature.

### 4.3 STT: `hark-stt`

- Add `Segment { start_ms, end_ms, channel: Me|Them, speaker: Option<u8>, text }` and an
  optional `segments` field on `Transcript`. The PTT path ignores it.
- New `MeetingTranscriber` trait (long-form, full-file) with only a Deepgram implementation in
  Core. It needs its **own timeouts**: `TOTAL_TIMEOUT_MS=15000` (`lib.rs:26-28`) is a PTT budget,
  and an hour of audio takes 20–60 s to process. Keep the one-retry rule, applied per request.
- Live chunks reuse `SttProvider::transcribe` unchanged. Gemini Live is not used for meetings
  (session caps).

### 4.4 Summary: `hark-voice`

- Add `summarize(transcript, template) -> MeetingNotes { summary, key_points, decisions, action_items[] }`
  over the same BYOK client and key resolution as `clean()`. Ask for JSON output and validate it
  at the boundary. Use a 120 s timeout, not `CLEANUP_TIMEOUT_MS=10_000`.
- Templates live in config: default, 1:1, stand-up, customer call. User-editable text.

### 4.5 Storage: `hark-store`

- `migrations/004_meetings.sql`: `meetings(id, started_ms, ended_ms, title, app_hint, trigger, stt_provider, notes_json, audio_bytes, audio_evicted_ms NULL)`,
  where `trigger` is `manual | ask | auto` and `audio_evicted_ms` is set when §4.9 removes the audio,
  `meeting_segments(meeting_id, start_ms, end_ms, channel, speaker, text)`,
  `meeting_speakers(meeting_id, speaker, display_name)` for renaming "Speaker 2 → Dana",
  and an FTS5 table over segment text for search.
- The existing content-hygiene rule applies: meeting records get **no `Debug` impl and are never logged.**

### 4.6 Config: `hark-config`

`[meeting]` goes before `[[invocations]]` (`lib.rs:493-495`) and is additive, so no migration step:
`enabled`, `mic_device` (default = communications device), `system_source = "all" | "app"`,
`live_transcript = true`, `final_pass = "deepgram" | "none"`, `summary = true`,
`summary_template`, `consent_reminder = true`, plus:

| Key | Default | Meaning |
|---|---|---|
| `audio_cap_mb` | `5120` (5 GB) | Circular storage cap for recordings. `0` = don't keep audio. Integer MB avoids float round-trips in TOML; the UI edits it in GB. Validated at load: clamp to 0–1,048,576 (1 TB) and log once if clamped. |
| `compress_audio` | `true` | Compress kept recordings to stereo MP3 after processing (§4.11). |
| `auto_detect` | `"ask"` | `"off"`, `"ask"` (non-modal prompt), or `"auto"` (start silently, with the indicator always visible). |
| `auto_stop_after_s` | `60` | Stop once the detected app has released the mic for this long. `0` = never auto-stop. |
| `detect_apps` | built-in list (§4.8) | User-editable list of app identifiers treated as meeting apps. |

### 4.7 UI: `hark-app`

- **Tray:** "Start meeting notes" / "Stop meeting notes (12:04)" via `TrayAction`
  (`tray/mod.rs:54-60`). The recording state must be visible in the tray icon at all times.
- **Meetings page** (`ui/meetings/`, shaped like `ui/history`): list with title/date/duration;
  detail view with transcript (speaker chips, timestamps), notes, action-item checkboxes,
  speaker rename, a **Share** menu (§4.10), delete.
- **Live pane** during recording: the rolling transcript plus elapsed time and a Stop button.
  It talks to a `MeetingController` with its own channel and `wake_ui` (never
  `request_repaint()`, per `hark-app/CLAUDE.md`).
- **Consent:** a first-run notice explains that some jurisdictions need every party's consent.
  An optional button pastes a canned "I'm using Hark to transcribe this meeting" line into the
  focused chat through the existing `hark-inject`.
- **Detection prompt:** "Teams is using your mic. Take meeting notes?" with **Start**,
  **Not this meeting** and **Settings**. It must work while the main window is hidden in the tray,
  so it follows `overlay.rs`: **one persistent deferred viewport, created hidden and only
  shown/hidden, never a window per prompt**. A window per event is what lost the GPU device and
  flashed before; see the `overlay.rs` header. It is registered from `App::logic`, not `ui`, so
  it runs while the app is minimized to the tray. The prompt is non-modal, never takes focus
  from the meeting app, and dismisses itself after 30 s.
- **Settings → Meetings → Storage:** a usage bar ("3.2 GB of 5 GB, 14 recordings"), a GB input
  for the cap (0 = don't keep audio), "Open folder", and "Delete all meeting audio" (confirm).
  **Lowering the cap below current usage shows what will be deleted** ("This removes audio from
  your 6 oldest meetings. Transcripts stay.") and only acts on confirm.
- **Settings → Meetings → Detection:** the off/ask/auto choice, auto-stop delay, and the
  editable app list.
- Meeting detail shows "Audio removed to stay under your 5 GB cap" once a recording is evicted.

### 4.8 Auto-detect (Core)

A detector thread in `hark-meeting` polls every 2 s. It is a pure `Detector` state machine fed by
a per-OS `MicUsers` probe, so the logic is unit-tested on fixture snapshots with no OS calls.

**Probe: who is using the mic right now?**
- **Windows:** enumerate `HKCU\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone`.
  Packaged apps are direct subkeys named by package family (new Teams = `MSTeams_8wekyb3d8bbwe`).
  Desktop apps sit under `NonPackaged\<exe path with # for \>`. A value of `LastUsedTimeStart > 0`
  with `LastUsedTimeStop == 0` means the mic is in use now. Read it with the `windows` crate
  (`Win32_System_Registry`) on the detector thread. It is cheap and needs no permission.
- **macOS 14+ (macOS phase):** `kAudioHardwarePropertyProcessObjectList` + per-process `IsRunningInput`,
  **polled**, because per-process listeners are reported not to fire reliably.

**Rules**
1. **Exclude Hark itself.** Hark keeps a mic stream open for pre-roll, so its own ConsentStore
   entry reads "in use" permanently. Match on `std::env::current_exe()`. Without this, every
   launch looks like a meeting.
2. **Match against `detect_apps`.** The built-in list is new Teams (packaged), classic Teams,
   Zoom, Webex, Slack, Discord, GoTo, RingCentral, plus browsers (Chrome, Edge, Firefox, Brave)
   for Meet/Teams-web. A browser only counts if a top-level window title contains a meeting marker
   ("Meet –", "Microsoft Teams", "Zoom"), because a browser can hold the mic for anything.
   Window titles are read in memory and never logged.
3. **Debounce:** the app must hold the mic for ≥ 5 s before the prompt, so a device test or voice
   message does not trigger it.
4. **One prompt per session.** "Not this meeting" suppresses re-prompting until that app has
   released the mic. The same applies when the user dismisses the prompt or stops manually.
5. **auto mode** starts without asking but always shows the tray recording state and the live
   pane's indicator. It is never invisible.
6. **Auto-stop** when the triggering app has released the mic for `auto_stop_after_s`. Manual
   stop always works. A manually started meeting is **never** auto-stopped by the detector.
7. PTT dictation never counts as a meeting and never stops one.

### 4.9 Circular audio storage cap (Core)

A `storage` module in `hark-meeting`. The eviction decision is a pure function, and all file
deletion happens in one small I/O layer.

```rust
/// Oldest-first ids whose audio must go so `used <= cap`. Never returns `protected`.
fn plan_eviction(recordings: &[Recording /* id, bytes, ended_ms */], cap: u64, protected: Option<MeetingId>) -> Vec<MeetingId>
```

**Rules**
1. **The filesystem is the source of truth for size.** Sum `metadata().len()` of every file
   under `<data_dir>/meetings/`, not the DB's `audio_bytes`. Otherwise the cap drifts after a
   crash or a manual delete.
2. **Evict whole recordings, oldest `ended_ms` first,** until `used <= cap`. Delete both channel
   files and the directory, set `audio_evicted_ms`, and keep the transcript, segments and notes.
3. **The recording in progress is never evicted.** While recording, the storage layer re-checks
   every 60 s and evicts older recordings as the spool grows, so the total stays under the cap.
   Only when the current meeting *alone* exceeds the cap does usage go over it. Hark keeps
   recording, never deletes the newest recording, and shows "This meeting is larger than your
   storage cap" once. Transcription never depends on the cap.
4. **A recording still being transcribed or summarized is protected too.** Eviction waits
   until its final pass has finished.
5. **When to run:** after each meeting finalizes, every 60 s while recording, at startup, and when
   the cap is changed in Settings. A lower cap only deletes after the §4.7 confirmation.
6. **`audio_cap_mb = 0`:** delete a meeting's audio as soon as its final pass and summary are saved.
7. **Hard path guard:** only ever delete canonicalized paths *inside*
   `<data_dir>/meetings/`, and only directories named by a meeting id that exists in the DB.
   Stray files there are reported in Settings, never auto-deleted.
8. Deletions are logged by meeting id and bytes only, never by title or content.

### 4.10 Sharing and export (Core)

Hark has no backend, so sharing means **producing a file or clipboard text the user sends
themselves** (email, Teams, Slack, OneDrive). Rendering and encoding live in
`hark-meeting::export` as pure functions over a `Meeting` value. Dialogs and file writes stay in
`hark-app` on worker threads.

**Share menu** on the meeting detail view:

| Action | Output |
|---|---|
| Copy notes | Summary, decisions and action items as Markdown to the clipboard. Pastes cleanly into Teams, Slack, Outlook and Notion. |
| Copy transcript | The transcript as plain text to the clipboard |
| Export notes + transcript… | `.md` (default) or `.txt`, via a save dialog |
| Export audio… | `.mp3` (default) or `.wav`, via a save dialog |
| Show in folder | After any export, via `opener::reveal()` |

**Transcript export**
- Options, remembered in `[meeting] export_*`: include notes / transcript / timestamps.
- Markdown layout: `# Title`, a meta line (date · duration · participants), `## Summary`,
  `## Decisions`, `## Action items` as `- [ ] owner: task`, `## Transcript` with lines like
  `**[00:12:34] Dana:** …`. Plain text is the same content without markup.
- **Uses the user's speaker renames** ("Speaker 2 → Dana") and "Me" becomes the user's display
  name if one is set.
- Default filename is `2026-09-26 Weekly sync.md`, sanitized for Windows (strip `<>:"/\|?*` and
  control chars, avoid `CON`/`PRN`/`NUL`/`COM1`…, trim trailing dots/spaces, cap the length).
  The last-used folder is remembered.
- Writes go to a temp file in the target folder, then rename, so a crash never leaves a
  half-written export.

**Audio export (D8)**
- **Source:** the archived stereo MP3 (§4.11), or the WAV spools if the meeting has not been
  compressed yet. Decoding the archive uses `symphonia` (pure Rust, MPL-2.0, `mp3` feature only).
- **Default mix: everyone, mono.** Decode, sum Me + Them (time-aligned, thanks to the §4.1 gap
  padding), peak-normalize so the sum never clips, then encode. Option: **stereo, Me left / Them
  right**, which is a straight file copy of the archive with no re-encode.
- **MP3 32 kbps mono, 16 kHz** via `mp3lame-encoder` 0.2.4 / `mp3lame-sys` 0.1.11 (vendors
  LAME 3.100 and builds with `cc` on Windows, so no cmake). ID3 title/date tags are set.
  Encoding runs on a worker with a progress bar and a Cancel button, and it is fast: an hour
  encodes in seconds.
- **WAV** export writes the decoded mix with `hound` (115 MB/h mono). After compression the audio
  is MP3-quality, so WAV here means "universally editable", not "lossless".
- **Only available while the audio exists.** Once evicted (§4.9) or with cap `0`, the item is
  disabled with the reason ("Audio removed to stay under your 5 GB cap").
- The first audio export shows a one-time reminder: "Only share recordings everyone agreed to."

**Why MP3 and not M4A or Opus**

| Format | Plays by double-click everywhere? | Size, 1 h speech | Cost to Hark |
|---|---|---|---|
| **MP3** | Yes: Windows, macOS, iPhone, Android, Outlook/Gmail/Teams/Slack previews | ~14 MB | LAME is **LGPL**: ship its notice and satisfy the relink clause (§7) |
| M4A (AAC) | Yes | similar | No maintained Rust wrapper for the Media Foundation / AudioToolbox encoders. Hand-written COM on Windows plus a separate macOS path. Revisit if the LGPL obligation proves a problem. |
| Opus | **No**: weak Outlook/Gmail/iOS Mail previews | ~10 MB | Rejected: fails "easily play" |
| WAV | Yes | ~115 MB | Kept as the "original quality" option |

**Dialogs: no Linux pull, no second runtime**
- `rfd` 0.17.2 is a **target-specific dependency for Windows and macOS only**, with
  `default-features = false`. Its Linux portal backend *requires* rfd's `tokio` or `async-std`
  feature, which would break the no-global-runtime rule. Meetings are hidden on Linux (D4), so
  Linux never compiles rfd. Revisit (`gtk3` backend on the tray's GTK thread, or
  `egui-file-dialog`) when Linux is picked up.
- **Windows:** call the dialog from a worker thread with Hark's window as parent (modal to the
  window). The egui loop and the dictation pill keep running. A dialog on the main thread would
  freeze both.
- **macOS:** sync dialogs must run on the main thread, so use `AsyncFileDialog`. Decide in the
  macOS phase.
- `opener` 0.8.5 `reveal` has no async runtime. `explorer.exe` is a GUI-subsystem process, so there
  is no console flash (the `CREATE_NO_WINDOW` rule does not apply).
- **Clipboard:** set it directly. Do not reuse `hark-inject`'s stash → paste → restore
  sequence; that would restore the old clipboard and undo the copy.

### 4.11 Compressing kept recordings (Core, D9)

1. **When:** after the meeting's final pass and summary are saved. The final pass always gets the
   lossless WAVs. It runs on a worker at below-normal priority so it never competes with a live
   meeting or a dictation.
2. **What:** one file, `<data_dir>/meetings/<id>/audio.mp3`, **stereo** with L = Me and R = Them,
   16 kHz, 64 kbps, LAME mode **`STEREO` (not joint stereo)** so the sides stay independent.
   ~29 MB/h. The same file can go to Deepgram multichannel for a re-run and serves as the stereo
   export.
3. **Crash-safe order:** encode to `audio.mp3.tmp` → decode it with `symphonia` and check its
   duration against the WAVs (±1 s) → rename to `audio.mp3` → delete the WAVs → update
   `audio_bytes`. If any step fails, the WAVs stay and the meeting is retried on the next startup.
   After 3 failures, keep the WAVs and log it.
4. **Storage cap interplay (§4.9):** the meeting stays protected until compression finishes. For a
   moment the WAVs and the MP3 both exist and both count, so the cap is enforced *after* the WAVs
   are deleted.
5. **Cap `0`:** nothing is compressed; the audio is deleted as in §4.9 rule 6.
6. **Config:** `compress_audio = true` (default). Setting it to `false` keeps WAVs, for users who
   want lossless archives and accept ~8× the space.
7. **Settings → Storage** shows the effect in plain terms: "5 GB holds about 170 hours of meetings."

## 5. Phases

### Foundation: CP0 spike on Windows (gate; standalone branch)
1. A throwaway binary captures mic + WASAPI loopback for 60 min to two 16 kHz WAVs.
2. Measure CPU, memory, drift between channels, and **gaps while nothing is playing** (see §7).
3. Prove PTT still works with a second mic stream open.
4. Run both WAVs through Deepgram multichannel+diarize and eyeball the speaker split.
5. Dump the ConsentStore microphone tree during a real Teams call, a Zoom call and a Meet tab.
   Confirm the packaged/NonPackaged paths, the `LastUsedTimeStop == 0` signal, how fast it flips
   at call end, and that Hark's own entry shows as permanently in use.
Exit: numbers recorded here; go/no-go on D1–D3. *Estimate: one session, 2–3 h.*

### Core: MVP on Windows
1. `hark-audio` loopback + spool; `hark-meeting` state machine + chunker + merge (unit tests on synthetic PCM).
2. `hark-meeting` detector (§4.8) + storage cap (§4.9), both pure-logic first with unit tests.
3. `hark-stt` `Segment` + Deepgram `MeetingTranscriber`.
4. `hark-store` migration 004; `hark-config` `[meeting]`.
5. Tray start/stop, detection prompt viewport, live pane, Meetings page (list + detail + copy/export),
   Settings → Meetings (storage + detection).
6. `hark-voice` `summarize` + default template.
7. Sharing (§4.10): Markdown/TXT renderers, clipboard copy, MP3/WAV audio export, save dialog,
   Show in folder, and LAME notice in About + installer.
8. Compression (§4.11): stereo MP3 archive after processing, verify, delete WAVs, startup recovery.
*Estimate: 7 sessions. Detection and the storage cap add about 2, sharing about 1, compression
under 1 (it reuses the sharing encoder).*

### Polish
Per-process loopback, AEC bake-off, speaker rename + FTS search, Gemini Files final pass,
more share formats (`.srt`/`.vtt` captions to pair with the audio, `.docx`), export an excerpt
(a selected range of transcript lines + the matching audio), the Windows share sheet
(`IDataTransferManagerInterop::ShowShareUIForWindow`),
meeting start/stop chord, re-run the final pass on a kept recording (send the stereo MP3
archive to Deepgram multichannel as-is), and registry change notification
(`RegNotifyChangeKeyValue`) instead of the 2 s poll.
*Estimate: 3–5 sessions.*

### Ship: Windows
`Docs/features/MEETINGS.md`, privacy section in the README, CHANGELOG, LL-G lessons from CP0 and Core.
Confirm Linux still builds with Meetings hidden. *Estimate: 1 session.*

### macOS (starts once Windows ships and is stable in daily use)
1. Real-Mac spike: process tap + aggregate device, TCC prompt, 60-min dual capture.
2. Port the capture and the mic-users probe (§4.8) behind the existing per-OS seams.
3. Info.plist `NSAudioCaptureUsageDescription`, a stable signing identity, and all-zero-buffer
   permission detection (§7).
*Estimate: 2–3 sessions plus Mac access.*

### Linux: deferred (D4)
Not scheduled. The §4.1 notes are kept so it can be picked up later without re-research.

## 6. Testing

- Pure units: state machine transitions, chunker cut points, merge ordering, Deepgram
  multichannel JSON → `Segment` parsing (fixture from CP0, **scrubbed of speech content**),
  notes JSON validation, migration 004 on a copy of a 003 DB.
- Export renderers: golden-file Markdown and TXT for a fixture meeting (renamed speakers,
  no notes, no timestamps, empty transcript). Filename sanitizer: reserved names, illegal
  characters, trailing dots, length cap. Mixdown: two full-scale channels never clip.
  MP3 smoke test: 1 s of synthetic tone encodes, the output starts with a valid frame header,
  and its duration matches.
- Compression: synthetic L-only and R-only tones survive a stereo MP3 round trip with no
  audible crosstalk (proves `STEREO`, not joint stereo). Verification rejects a truncated MP3.
  An interrupted compression (WAV + partial `.mp3.tmp`) recovers on the next startup without
  losing the WAV.
- `plan_eviction`: under cap → nothing; over cap → oldest first; the protected id is never
  returned even when it alone exceeds the cap; cap 0; ties on `ended_ms`; empty list.
- Storage I/O layer against a temp dir: the path guard refuses anything outside
  `meetings/`, symlinks included; stray files are reported, not deleted.
- `Detector`: fixture ConsentStore snapshots for Hark-only (no meeting), Teams packaged, Zoom
  NonPackaged, a browser with and without a meeting title, the 5 s debounce, "Not this meeting"
  suppression, auto-stop timing, and a manual start never auto-stopped.
- No live network or audio devices in `cargo test`. The device/loopback paths are verified by hand on each OS.

## 7. Risks

| Risk | Mitigation |
|---|---|
| WASAPI loopback delivers **no packets while nothing is rendering**, so the Them timeline silently compresses | Pad gaps from the device position / wall clock. Prove it in CP0. |
| Mic bleed duplicates remote speech into Me | Headphone hint (Core), AEC (Polish), dedupe only as a last resort |
| Recording-consent law | First-run notice, persistent recording indicator, optional announce line |
| Raw audio on disk is a new privacy surface (D3) | User-set cap, `0` = don't keep; data dir only; never logged; documented |
| LAME is LGPL (static link via `mp3lame-sys`) | Ship LAME's notice in About + installer + `THIRD_PARTY_NOTICES`. Relink clause: `BoardPandas/Hark` is **public** (checked 2026-09-26), so users can rebuild against a modified LAME from source. If the repo ever goes private, switch to M4A via OS encoders or dynamic linking. |
| Exported files leave Hark's control | They sit outside `meetings/`: never counted against the cap and never deleted by Hark. State this in Settings → Storage and the docs. |
| The storage cap deletes user data | Only audio, never transcripts. Oldest first. Newest/in-progress recordings are protected. Lowering the cap needs confirmation. Hard path guard (§4.9 rule 7). |
| Detection false positives (browser mic for a non-meeting site, voice memo) | Window-title check for browsers, 5 s debounce, `ask` as the default, "Not this meeting" |
| Detection misses (an app not on the list, or the ConsentStore format changes in a Windows update) | Editable `detect_apps`, manual start always available, CP0 dump re-checked each Windows feature update |
| macOS TCC fails silently (`noErr` + zero-filled buffers) when the Info.plist key is missing | Detect an all-zero loopback for 10 s and surface a permission error |
| Live chunking cost on per-minute providers doubles with two channels | Skip silent chunks; show estimated cost/hour in Settings |

## 8. Lessons Learned / Gotchas (pre-seeded from research; extend after CP0)

- Krisp's "bot-free" capture is a **virtual audio driver** the user must select in Teams, not
  loopback. Hark can do better with OS loopback and no per-app setup.
- `gpt-4o-transcribe-diarize` rejects audio past ~1400 s, and speaker identity does not carry
  across chunks. Do not base long-form diarization on it.
- Gemini Live / 3.5 Transcribe Live sessions cap at 10–15 min, so they are unsuitable for meetings.
- cpal WASAPI loopback: call `build_input_stream` on an **output** device with
  `default_output_config()`. `supports_input()` lies (false) there.
- macOS process taps need `NSAudioCaptureUsageDescription`. Without it, creating the tap returns
  `noErr` and yields silence. The TCC prompt fires on `AudioDeviceStart`, not on tap creation,
  and the grant is keyed to the code signature.
- macOS 14+ per-process `IsRunningInput` listeners are unreliable, so poll.
- New Teams is a packaged app. Its ConsentStore key is under the package family name, not `NonPackaged`.
- Hark's own always-open pre-roll mic stream makes Hark look permanently "in use" in the
  ConsentStore. The detector must exclude its own exe, or every launch is a "meeting".
- Any new always-available window (the detection prompt) follows the `overlay.rs` rule: one
  persistent hidden viewport, toggled and never created per event.
- `rfd`'s Linux portal backend needs rfd's `tokio` or `async-std` feature to compile, which is a
  hidden second async runtime. Keep rfd off Linux until Linux is scheduled.
- A native save dialog opened on the egui main thread freezes the event loop, the recording
  pill included. On Windows, open it from a worker with the main window as parent.
- Clipboard "Copy" must not go through the injection stash/restore path; that restores the
  previous clipboard and silently undoes the copy.
- Archive MP3 must be encoded as LAME `STEREO`, not the default `JOINT_STEREO`. Mid/side coding at
  low bitrate leaks one side into the other, which would corrupt the Me/Them split on a re-run.
- Storage cap: size comes from the filesystem, not the DB. The newest/in-progress recording is
  never the eviction victim.
- LL-G `JoinHandle joined in Drop deadlocks with sender fields`: declare sender fields before
  thread handles in the session struct.
- LL-G `reqwest multipart streams mask transport errors`: applies to the large final-pass upload.
- Route to LL-G after CP0: WASAPI COM apartment for loopback, loopback silence gaps, the macOS
  tap silent failure, and the ConsentStore packaged vs non-packaged paths.

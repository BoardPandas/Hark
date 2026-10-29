<!-- PAGE_ID: hark_14_glossary -->
<details>
<summary>Relevant source files</summary>

The following files were used as evidence for this page:

- [README.md](../README.md)
- [CLAUDE.md](../CLAUDE.md)
- [crates/hark-stt/src/lib.rs](../crates/hark-stt/src/lib.rs)
- [crates/hark-audio/src/ring.rs](../crates/hark-audio/src/ring.rs)
- [crates/hark-hotkey/src/edges.rs](../crates/hark-hotkey/src/edges.rs)
- [crates/hark-pipeline/src/events.rs](../crates/hark-pipeline/src/events.rs)
- [crates/hark-stt/src/meeting.rs](../crates/hark-stt/src/meeting.rs)
- [crates/hark-app/src/theme/appearance.rs](../crates/hark-app/src/theme/appearance.rs)
- [crates/hark-app/src/theme/lettering.rs](../crates/hark-app/src/theme/lettering.rs)
- [crates/hark-app/src/theme.rs](../crates/hark-app/src/theme.rs)
- [crates/hark-app/src/ui/book.rs](../crates/hark-app/src/ui/book.rs)
- [crates/hark-meeting/src/speakers.rs](../crates/hark-meeting/src/speakers.rs)
- [crates/hark-store/src/meetings.rs](../crates/hark-store/src/meetings.rs)

</details>

# Glossary

**Window-scoped speaker:** a Gemini final-pass speaker identifier valid only within
one five-minute audio window. Matching numbers across windows do not identify the
same person ([meeting labels](features/MEETINGS.md#gemini-files-final-pass)).

> **Related Pages**: [Overview](OVERVIEW.md), [Architecture](core/ARCHITECTURE.md), [Meetings](features/MEETINGS.md)

---

<!-- BEGIN:AUTOGEN hark_14_glossary_terms -->
## Terms

- **AEC (acoustic echo cancellation):** Uses captured playback as a reference to reduce speaker sound picked up by a microphone. Hark's optional **Reduce speaker echo** applies Rust AEC3 only to the meeting microphone, before recording and live transcription; it defaults off ([Meetings](features/MEETINGS.md#reduce-speaker-echo)).
- **BYOK (bring your own key):** The user supplies credentials for cloud transcription, optional cleanup, and meeting processing. Keys live in the OS keychain, not `config.toml`; local-primary STT needs no cloud key, but meeting final-pass and notes calls have independent settings ([README: Privacy](../README.md#privacy)).
- **Cleanup pass:** An optional OpenAI-compatible chat request that applies the selected voice after transcription. Verbatim mode, a fired invocation, or a Gemini Smart transcript skips this call ([Architecture](core/ARCHITECTURE.md#the-release-to-inject-pipeline)).
- **Final pass:** The after-call Deepgram `multichannel=true&diarize=true` request over a whole meeting recording that replaces the live Me/Them transcript with diarized "Speaker N" labels within Them. It uploads the meeting's MP3 archive when audio compression is on (about 8x smaller than the WAVs), skips the upload for a recording that is silent on both channels, and uses its own client with a 15-second connect bound. A pass that cannot reach Deepgram is kept as a pending **transcript status** and retried. Runs only with a Deepgram key configured for meetings; otherwise the live transcript stands ([Meetings](features/MEETINGS.md#deepgram-final-pass), [meeting.rs](../crates/hark-stt/src/meeting.rs)).
- **Fused cleanup:** Transcription and tidying performed by one provider turn. Gemini Smart returns one tidied string in both transcript fields and marks it so Hark does not clean it a second time ([Transcription](features/TRANSCRIPTION.md#deepgram-and-gemini-live)).
- **Invocation:** A guarded spoken trigger that replaces matching transcript text with user-authored canned text. A fired invocation is injected without a cleanup model rewriting it ([Invocations](features/INVOCATIONS.md)).
- **Live STT:** A session opened on key-down and fed while the user speaks. Two adapters stream: Gemini Live and OpenAI live. Each declares its own input rate (`LiveStt::input_rate`: 16 kHz for Gemini, 24 kHz for OpenAI), and the stream is resampled straight from the device rate to it. The finished ring-buffer window remains available for gates and fallback ([lib.rs](../crates/hark-stt/src/lib.rs)).
- **Live transcribe:** The OpenAI provider's opt-in toggle (`[provider] kind = "openai"`, `live = true`). It streams dictation to `gpt-live-transcribe` over OpenAI's realtime WebSocket with the same OpenAI key, at about 4× gpt-transcribe's per-minute price. A failed or empty stream falls back to gpt-transcribe; meeting chunks ignore the toggle ([Transcription](features/TRANSCRIPTION.md#openai-live-transcribe)).
- **Local primary / local backup:** For dictation, on-device STT can be the primary engine or a fallback after a bounded cloud attempt, chosen with **Use on-device transcription** in Settings → Dictation → Speech to text. Meetings uses local Primary for live chunks but has separate final-pass and summary settings; disable both to keep meeting content entirely on-device. Model weights are downloaded explicitly and stored locally ([On-Device STT](features/ON_DEVICE_STT.md), [README: Privacy](../README.md#privacy)).
- **OpenAI-compatible:** The Whisper-family multipart `/audio/transcriptions` contract, including compatible third-party endpoints and the Groq configs schema 7 migrated to it. OpenAI's newer gpt-transcribe models use a separate adapter because their bias fields differ ([Transcription](features/TRANSCRIPTION.md#openai-compatible-and-gpt-transcribe-adapters)).
- **Per-process loopback:** Windows WASAPI capture (`AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK`) of one process tree's audio. Meeting mode's default "Them" source. It was measured deaf to new Teams on whichever output it played to, so a call app seen playing is captured with **endpoint loopback** of the output it plays to instead: everything that output plays, with the silent gaps WASAPI skips filled from packet timestamps ([Audio Capture](features/AUDIO_CAPTURE.md#meeting-capture)).
- **Provider-cleaned transcript:** A `Transcript` with `cleaned: Some(...)`, signaling that the STT provider already formatted the text and Hark should suppress the separate cleanup call ([lib.rs](../crates/hark-stt/src/lib.rs)).
- **Safe provider diagnostic:** An error description restricted to status codes, fixed or allowlisted categories, and structural JSON line/column information. Response snippets, arbitrary reason fields, request URLs, and parser messages that quote user text are excluded ([Transcription](features/TRANSCRIPTION.md#errors-and-metrics), [Voice Cleanup](features/VOICE_CLEANUP.md#error-handling)).
- **Push-to-talk chord:** One or more configured keys that must all be held to record. The first release ends the hold; injected keys are ignored, and missed releases are reconciled against physical state ([edges.rs](../crates/hark-hotkey/src/edges.rs), [edges.rs](../crates/hark-hotkey/src/edges.rs)).
- **Release-to-inject:** The product latency between releasing the chord and text appearing at the cursor. Live streaming (Gemini Live, OpenAI Live transcribe) can upload most audio before this interval begins ([README: Design principles](../README.md#design-principles), [Architecture](core/ARCHITECTURE.md#retry-and-latency-discipline)).
- **Ring buffer:** A fixed-size, allocation-free audio buffer written at the input device's native rate. Absolute sample indexes support pre-roll, tail, a live reader, and finished-window assembly ([ring.rs:1-12](../crates/hark-audio/src/ring.rs#L1-L12)).
- **Static trust roots:** Mozilla's root certificate set compiled into Hark (`webpki-root-certs`) and passed to every HTTP client with `tls_certs_only`, so TLS never consults the OS certificate store ([lib.rs](../crates/hark-stt/src/lib.rs)).
- **Spellbook:** Local terms used for provider-specific vocabulary bias and guarded phonetic correction after transcription. Bias wire formats include `prompt`, `keywords[]`, OpenAI live's `keywords` in `session.update`, `keyterm`, and Gemini custom vocabulary ([README: Tech stack](../README.md#tech-stack)).
- **Quiet chunk:** A meeting live-transcript chunk that passed the loudness gate but holds no speech (a cough, typing). Gemini Live answers it with nothing; for meetings that is read as an empty line, not a failed request ([Transcription](features/TRANSCRIPTION.md#deepgram-and-gemini-live)).
- **Meeting excerpt:** A selected audio time range exported with matching transcript text. Audio is cut after decoding; crossing transcript segments are included in full with clipped/rebased timestamps, and meeting-wide notes are omitted ([Meetings](features/MEETINGS.md#excerpts)).
- **Meeting deletion:** A confirmed request whose completion is acknowledged by the storage worker after audio and database removal. Audio-removal errors retain the database record and allow retry; this differs from audio-cap eviction, which always retains transcript and notes ([Meetings](features/MEETINGS.md#storage-and-the-audio-cap)).
- **Meeting toggle chord:** Optional Windows shortcut that starts or stops a meeting once per physical press. It shares the push-to-talk hook but cannot equal or contain the whole dictation chord, or be contained by it; no chord is assigned by default ([Meetings](features/MEETINGS.md#start--stop-shortcut)).
- **Re-run final pass:** An explicit, confirmed Deepgram upload of retained meeting audio, labelled **Transcribe recording** when the meeting has no transcript yet. A successful transaction replaces the transcript and resets speaker names while preserving notes and title; missing audio, a silent recording, provider failure, or an empty result leaves the old transcript intact, and the reason is kept as the meeting's transcript status ([Meetings](features/MEETINGS.md#re-run-a-saved-meeting)).
- **Silence watchdog:** The live check that a meeting track is hearing something. Each track's loudest 100 ms window is compared with a -85 dBFS floor (never exact zero); the microphone under it for 45 s, or the call under it for 2 minutes while the call app is playing, raises one notice during the call ([Meetings](features/MEETINGS.md#capture)).
- **Spool:** The append-only WAV file each meeting channel (`me.wav`, `them.wav`) is written to while recording. Its header's size fields are patched on close, or at startup by a crash-recovery pass if a meeting never finalized ([Audio Capture](features/AUDIO_CAPTURE.md#meeting-capture)).
- **Storage cap:** The circular limit on meeting audio kept on disk (`[meeting] audio_cap_mb = 5120`, 5 GiB displayed as 5 GB). It removes oldest completed recordings' audio while preserving transcripts and notes. Recording/processing meetings are protected; zero still allows temporary files until processing finishes ([README: Meetings privacy](../README.md#meetings-windows), [Meetings](features/MEETINGS.md#storage-and-the-audio-cap)).
- **Transcript status:** A meeting's final-pass state in `meetings.transcript_status`: `pending` (with the next retry time) while a pass that could not reach Deepgram waits to retry, `failed:<reason>` once Hark stops trying, NULL when nothing is outstanding. A label, never transcript content; the retry schedule itself lives in a `.transcript-pending` marker in the meeting folder ([Meetings](features/MEETINGS.md#deepgram-final-pass), [Data Storage](core/DATA_STORAGE.md#schema)).
- **Tray daemon:** Hark's normal hidden state: the pipeline remains active while the settings, history, and stats window opens only on demand ([README: Architecture](../README.md#architecture)).
- **Verbatim / Smart:** Gemini rendering modes. Verbatim preserves the literal transcript and is the default; Smart removes disfluencies and formats within the same provider turn ([Transcription](features/TRANSCRIPTION.md#deepgram-and-gemini-live)).
- **Insights:** Local, period-based numeric dictation details retained for up to 366 days independently of transcript history, plus lifetime totals. Reset stats clears numeric details and counters; retained transcripts remain available for separately opted-in word analysis ([Data Storage](core/DATA_STORAGE.md#lifetime-stats-and-detailed-insights)).
- **Measured pace:** Words from dictations with known clip duration divided by that duration, including capture padding; older missing durations are excluded and coverage is shown ([Desktop UI](features/DESKTOP_UI.md#interpreting-insights)).
- **Words in invocation output:** The full inserted word count on rows where an invocation fired, including surrounding speech for anywhere-scope invocations; separate from dictated words ([Invocations](features/INVOCATIONS.md#edge-cases)).
- **Parchment:** One of six appearances (System, Light, Dark, Solarized Light, Solarized Dark, Parchment). It is light-only and fills both the light and dark palette slots, so it stays light when the OS resolves to dark ([appearance.rs:8-21](../crates/hark-app/src/theme/appearance.rs#L8-L21), [appearance.rs:57-69](../crates/hark-app/src/theme/appearance.rs#L57-L69)).
- **Wizard lettering:** A Settings → Appearance checkbox that sets text in Alegreya and titles and drop caps in Cinzel Decorative, instead of the default Inter. Until the user chooses, it follows the theme (on for Parchment, off elsewhere); once chosen, the choice holds on every theme ([lettering.rs:1-4](../crates/hark-app/src/theme/lettering.rs#L1-L4), [lettering.rs:9-14](../crates/hark-app/src/theme/lettering.rs#L9-L14), [fonts.rs:12-13](../crates/hark-app/src/theme/fonts.rs#L12-L13)).
- **Book view:** An optional List | Book renderer for the Spellbook and Invocations pages. Entries are packed onto facing pages and turned with controls or PageUp/PageDown/Home/End; List stays the default until the user chooses Book ([book.rs:1-4](../crates/hark-app/src/ui/book.rs#L1-L4), [book.rs:29-31](../crates/hark-app/src/ui/book.rs#L29-L31), [book.rs:43-50](../crates/hark-app/src/ui/book.rs#L43-L50)).
- **Drop cap:** The large colored first letter that opens each entry in the book view. It is shown as a capital even when the phrase is lowercase; only the display changes ([book.rs:398-415](../crates/hark-app/src/ui/book.rs#L398-L415), [spellbook/book.rs:1-4](../crates/hark-app/src/ui/spellbook/book.rs#L1-L4), [invocations/book.rs:1-4](../crates/hark-app/src/ui/invocations/book.rs#L1-L4)).
- **Speaker roster:** The per-meeting table of speaker labels and color slots, built once per view refresh, export or notes render. Slot order is Me first, then first transcribed appearance, then added speakers; two speakers sharing a label (ignoring case and surrounding spaces) are one person ([speakers.rs:135-142](../crates/hark-meeting/src/speakers.rs#L135-L142), [speakers.rs:247-248](../crates/hark-meeting/src/speakers.rs#L247-L248)).
- **Speaker key:** One speaker identity within a meeting: `Me` (any microphone line), `Them` (a call line with no diarized speaker), `Diarized(n)` (a Deepgram or window-encoded Gemini id), or `Added(n)` (a speaker the user created while reassigning a line). Unnamed, they read "Me", "Them", "Speaker 2", "Window 3 · Speaker 1" and "Other speaker 1" ([speakers.rs:26-37](../crates/hark-meeting/src/speakers.rs#L26-L37), [speakers.rs:98-112](../crates/hark-meeting/src/speakers.rs#L98-L112)).
- **Speaker color:** A speaker's hue from its roster slot. Slot 0 is Me and always Blue, and no one else is; the next five slots take Gold, Violet, Teal, Rose and Green, and past six speakers the hues repeat, while the name beside every mark never does ([theme.rs:225-236](../crates/hark-app/src/theme.rs#L225-L236)).
- **Guessed name:** A speaker name Hark filled in from a name said in the call, kept distinct from a user-typed one. A later guess may replace a guess; a user name is never replaced, and saving a guessed name unchanged confirms it as the user's ([meetings.rs:80-86](../crates/hark-store/src/meetings.rs#L80-L86), [guess.rs:1-10](../crates/hark-meeting/src/speakers/guess.rs#L1-L10), [speakers.rs:113-117](../crates/hark-app/src/ui/meetings/speakers.rs#L113-L117)).
<!-- END:AUTOGEN hark_14_glossary_terms -->

---

<!-- BEGIN:AUTOGEN hark_14_glossary_acronyms -->
## Acronyms

| Acronym | Expansion | Use in Hark |
|---|---|---|
| BYOK | Bring your own key | User-owned cloud provider credentials |
| PTT | Push to talk | Hold-to-record interaction |
| RMS | Root mean square | Audio loudness measurement used by the gate |
| STT | Speech to text | Cloud or on-device transcription |
| SPSC | Single producer, single consumer | Core ring-buffer ownership model; extra readers are immutable handles |
| UI | User interface | egui window, tray, overlay, and status surfaces |
| WAV | Waveform Audio File Format | Complete PCM16 container sent to batch adapters |
| WebSocket | WebSocket protocol | Bidirectional transport used by Gemini Live and OpenAI live |
<!-- END:AUTOGEN hark_14_glossary_acronyms -->

---

**AEC (acoustic echo cancellation):** processing that uses playback audio as a
reference to reduce its echo in microphone input. Hark's
[standalone comparison](../tools/meeting-aec-bakeoff/RESULTS.md) measures candidate
engines; production AEC remains disabled pending listening/build/alignment work.

## Mac native integration

- **CGEventTap:** Quartz’s keyboard event stream, used by Hark for held shortcuts, shortcut recording and meeting toggles. It runs on a dedicated Core Foundation run loop.
- **Apple Silicon / ARM64:** the supported Mac processor architecture. Hark publishes `-macos-arm64.dmg` installers for Apple Silicon Macs running macOS 14.2 or later.
- **Core Audio process tap:** macOS 14.2+ capture of selected processes or the system mix, used for the meeting’s Them track.
- **TCC permissions:** macOS user approvals for microphone access, global input observation, text insertion and system audio/screen capture. These are separate from an app’s code signature.
- **Developer ID and notarization:** the publisher signature and Apple assessment required by Hark’s production Mac package and self-update verification; ad-hoc development signatures are not an update trust anchor.

<!-- PAGE_ID: hark_01_overview -->
<details>
<summary>Relevant source files</summary>

The following files were used as evidence for this page:

- [README: Features and tech stack](../README.md#features)
- [README: Architecture](../README.md#architecture)
- [README: Project structure](../README.md#project-structure)
- [README: Configuration and on-device transcription](../README.md#configuration)
- [README: Privacy](../README.md#privacy)
- [package.json:1-10](../package.json#L1-L10)
- [Cargo.toml:1-31](../Cargo.toml#L1-L31)
- [CLAUDE.md:1-24](../CLAUDE.md#L1-L24)
- [CLAUDE.md:26-46](../CLAUDE.md#L26-L46)
- [AGENTS.md:1-80](../AGENTS.md#L1-L80)

</details>

# Hark, Overview

> **Related Pages**: [Architecture](core/ARCHITECTURE.md), [Getting Started](GETTING_STARTED.md), [Glossary](GLOSSARY.md), [Meetings](features/MEETINGS.md)

---

<!-- BEGIN:AUTOGEN hark_01_overview_introduction -->
## Introduction

Hark is a single-user, system-wide push-to-talk dictation app for Windows, macOS, and Linux. The user holds a configured chord, speaks, releases it, and Hark injects polished English text at the cursor in the focused application ([README](../README.md#hark)).

Windows and Linux currently implement the full push-to-talk path. macOS has native UI, tray, keychain, and injection work, but the CGEventTap hotkey hook remains an explicit platform seam, so it is not yet end-to-end ready ([README](../README.md#hark), [hark-hotkey/lib.rs:258-285](../crates/hark-hotkey/src/lib.rs#L258-L285)).

Cloud transcription is bring-your-own-key, with Deepgram, OpenAI, Groq, OpenAI-compatible endpoints, and Gemini Live supported. An optional Parakeet engine can instead run locally as a cloud backup or the primary recognizer ([README: Tech stack](../README.md#tech-stack), [README: On-device transcription](../README.md#on-device-transcription)). History, stats, settings, the spellbook, and invocations are stored on the machine; provider requests can include transcript text and vocabulary. Hark operates no server, account system, hosted database, or browser frontend ([README: Privacy](../README.md#privacy)).

Windows also supports **Meetings**: microphone and playback capture, live Me/Them transcripts, optional Deepgram speaker labels and LLM notes, transcript search, speaker renaming, explicit final-pass reruns from retained recordings, and text/audio exports. On-device Primary keeps live meeting chunks local; the final pass and notes generation have independent settings. Audio is kept under a configurable cap, while eviction preserves transcripts and notes ([README: Features](../README.md#features), [Meetings: Privacy](features/MEETINGS.md#privacy)).

Windows meeting detection uses registry change notifications with timed checks for debounce, auto-stop, and browser title changes ([Meetings: Auto-Detection](features/MEETINGS.md#auto-detection)).

The application is one native Rust process: an always-on tray daemon plus an egui window opened on demand. Optional cleanup uses the user's own provider key, and Gemini Live Smart mode may perform that formatting in the transcription turn itself ([CLAUDE.md:3-22](../CLAUDE.md#L3-L22)).

Sources: [README: Features](../README.md#features), [README: On-device transcription](../README.md#on-device-transcription), [CLAUDE.md:1-22](../CLAUDE.md#L1-L22)
<!-- END:AUTOGEN hark_01_overview_introduction -->

---

<!-- BEGIN:AUTOGEN hark_01_overview_principles -->
## Design Principles

Five stated principles shape implementation decisions in Hark ([README: Design principles](../README.md#design-principles)):

- **Speed is the product.** Perceived latency is measured from key release to text injection.
- **Local-first where it counts.** User state is stored locally; cloud audio, cleanup, and meeting processing use configured providers and keys.
- **Lean.** Hark is a native process without a webview, browser tab, or operated backend.
- **English done well.** Accuracy is prioritized over broad language support.
- **Data, not code, for tunable behavior.** Vocabulary, invocations, and voice choices are configuration rather than pipeline forks.

The latency rule now has two transport shapes: batch adapters reuse a long-lived HTTP client, while Gemini Live streams during the hold and finalizes after release. Both keep network work off the UI thread, allow at most one retry or replay, and defer history/stat writes until after injection ([CLAUDE.md:26-30](../CLAUDE.md#L26-L30)).

Sources: [README: Design principles](../README.md#design-principles), [CLAUDE.md:26-30](../CLAUDE.md#L26-L30)
<!-- END:AUTOGEN hark_01_overview_principles -->

---

<!-- BEGIN:AUTOGEN hark_01_overview_stack -->
## Technology Stack

Hark is a desktop app with no web infrastructure: no server, database service, auth service, or hosting platform ([README: Tech stack](../README.md#tech-stack)). The implementation is one Cargo workspace ([Cargo.toml:1-20](../Cargo.toml#L1-L20)):

| Layer | Choice | Source |
|---|---|---|
| Language / process model | Rust; single process, UI on main thread, pipeline on worker threads | ([README: Tech stack](../README.md#tech-stack), [CLAUDE.md:11](../CLAUDE.md#L11)) |
| Audio | `cpal`; device-rate capture, ring buffer, and per-clip 16 kHz mono resampling | ([README.md](../README.md#tech-stack), [CLAUDE.md](../CLAUDE.md#stack)) |
| Push-to-talk | `WH_KEYBOARD_LL` on Windows and evdev on Linux; CGEventTap is the pending macOS seam | ([README.md](../README.md#tech-stack), [hark-hotkey/lib.rs:258-285](../crates/hark-hotkey/src/lib.rs#L258-L285)) |
| STT (cloud, primary) | Deepgram, Whisper-family OpenAI-compatible endpoints, OpenAI `gpt-transcribe`, and Gemini Live | ([README: Tech stack](../README.md#tech-stack), [CLAUDE.md:14](../CLAUDE.md#L14)) |
| STT (on-device, optional) | `hark-local-stt`: sherpa-onnx Parakeet behind the `engine` feature, enabled by `hark-app`'s default `local-engine` feature. Opt-in at runtime via `[local_stt] mode` (Off / Backup / Primary) | ([hark-app/Cargo.toml:69-70](../crates/hark-app/Cargo.toml#L69-L70), [hark-local-stt/Cargo.toml:37](../crates/hark-local-stt/Cargo.toml#L37), [hark-config/src/local.rs:14](../crates/hark-config/src/local.rs#L14)) |
| STT transport | Blocking `reqwest` batch adapters plus a private current-thread Tokio/WebSocket runtime inside Gemini Live | ([README: Tech stack](../README.md#tech-stack), [CLAUDE.md:16](../CLAUDE.md#L16)) |
| Text processing | Spellbook correction, exact/fuzzy invocation matching, and optional cleanup voices | ([README: Tech stack](../README.md#tech-stack), [CLAUDE.md:17-19](../CLAUDE.md#L17-L19)) |
| Injection | Clipboard stash/set/paste/restore with platform key synthesis | ([README: Tech stack](../README.md#tech-stack), [CLAUDE.md:20](../CLAUDE.md#L20)) |
| Tray + UI | `tray-icon` + `eframe`/`egui` (native, no webview) | ([README: Tech stack](../README.md#tech-stack), [CLAUDE.md:21](../CLAUDE.md#L21)) |
| Persistence | `rusqlite`, TOML, and the operating-system keychain | ([README: Tech stack](../README.md#tech-stack), [CLAUDE.md:22](../CLAUDE.md#L22)) |

The workspace declares Rust 1.97 as its minimum supported toolchain ([Cargo.toml:22-31](../Cargo.toml#L22-L31)).

Sources: [README: Tech stack](../README.md#tech-stack), [CLAUDE.md:9-22](../CLAUDE.md#L9-L22), [Cargo.toml:1-31](../Cargo.toml#L1-L31)
<!-- END:AUTOGEN hark_01_overview_stack -->

---

<!-- BEGIN:AUTOGEN hark_01_overview_layout -->
## Crate Layout

Hark is one Cargo workspace with 16 member crates and one application binary ([Cargo.toml:1-20](../Cargo.toml#L1-L20)). The crates fall into nine responsibility groups:

| Area | Crates | Responsibility |
|---|---|---|
| Shell | `hark-app`, `hark-single-instance` | Native UI/tray orchestration and the one-process guard |
| Input | `hark-hotkey`, `hark-audio` | Chord observation, continuous microphone capture, and (for meetings) per-process system-audio loopback, WAV spooling, and MP3 archiving |
| Recognition | `hark-stt`, `hark-local-stt` | Cloud adapters, live streaming, optional Parakeet decoding, and the meeting-only Deepgram final pass |
| Text | `hark-spellbook`, `hark-voice` | Correction, invocations, cleanup, and meeting-notes summarization |
| Output | `hark-inject` | Clipboard and synthesized-key text injection |
| Orchestration | `hark-pipeline` | State machine, transport selection, retry/fallback, reporting, and (in `pipeline::meeting`) the meeting coordinator/live/finisher threads |
| Meetings | `hark-meeting` | Meeting logic: lifecycle state machine, live chunking, Me/Them transcript ordering, meeting auto-detection, the audio storage cap, and Share-menu export. Wired into `hark-app` on Windows; see [Meetings](features/MEETINGS.md) |
| State | `hark-config`, `hark-keychain`, `hark-store` | Settings, secrets, history, statistics, and meeting transcripts/notes |
| Desktop integration | `hark-autostart`, `hark-update` | Login startup and platform update behavior |

```mermaid
graph TD
    Hotkey["hark-hotkey"] --> Pipeline["hark-pipeline"]
    Audio["hark-audio"] --> Pipeline
    Config["hark-config"] --> Pipeline
    Pipeline --> Stt["cloud or local STT"]
    Stt --> Spellbook["spellbook + invocations"]
    Spellbook --> Voice["hark-voice"]
    Voice --> Inject["hark-inject"]
    Inject --> Store["hark-store"]
    Keychain["hark-keychain"] --> Stt
    Keychain --> Voice
    App["hark-app"] --> Pipeline
    App --> Store
    Autostart["hark-autostart"] --> App
    Update["hark-update"] --> App
```

`config/` holds the shipped TOML defaults, while `installer/` and `packaging/` contain platform distribution assets ([README: Project structure](../README.md#project-structure)).

Sources: [Cargo.toml:1-20](../Cargo.toml#L1-L20), [README: Project structure](../README.md#project-structure)
<!-- END:AUTOGEN hark_01_overview_layout -->

---

<!-- BEGIN:AUTOGEN hark_01_overview_hotpath -->
## The Hot Path

The release-to-inject flow is the app's latency-critical path. Capture begins before the user speaks, and Gemini Live may upload audio during the hold; all network, recognition, correction, cleanup, and injection work remains off the UI thread ([README: Architecture](../README.md#architecture), [CLAUDE.md:26-30](../CLAUDE.md#L26-L30)):

```mermaid
graph TD
    KeyDown["Key Down"] --> RingBuffer["cpal Ring Buffer + Pre-roll"]
    RingBuffer --> Live{"Live session?"}
    Live -->|"Yes"| Stream["Stream during hold"]
    Live -->|"No"| KeyUp["Key Up + Tail"]
    Stream --> KeyUp
    KeyUp --> Recognize["Finish live or batch STT"]
    Recognize --> Correct["Correct spellbook terms"]
    Correct --> Invoke{"Invocation fired?"}
    Invoke -->|"Yes"| Verbatim["Inject verbatim expansion"]
    Invoke -->|"No"| Cleanup["Optional cleanup"]
    Cleanup --> Inject["Inject at cursor"]
    Verbatim --> History
    Inject --> History["Write History and Stats"]
```

- Key-down marks the capture window and, for Gemini Live, opens the single-use session that streams while the chord is held ([README: Architecture](../README.md#architecture)).
- Key-up appends the configured tail, then either finalizes the live turn or sends the preserved finished clip through a batch adapter. A failed live attempt may replay the clip, but no dictation receives more than one retry or replay ([README: Architecture](../README.md#architecture), [CLAUDE.md:30](../CLAUDE.md#L30)).
- The spellbook corrects the transcript before invocation matching. A fired invocation injects its expansion verbatim and bypasses cleanup ([README: Architecture](../README.md#architecture)).
- Non-invocation text may receive optional provider cleanup before clipboard injection; provider-cleaned Gemini output is not cleaned twice ([README: Architecture](../README.md#architecture), [README: Privacy](../README.md#privacy)).
- History and lifetime statistics are written only after injection, keeping storage I/O outside perceived latency ([README: Architecture](../README.md#architecture), [CLAUDE.md:30](../CLAUDE.md#L30)).

Sources: [README: Architecture](../README.md#architecture), [CLAUDE.md:26-30](../CLAUDE.md#L26-L30)
<!-- END:AUTOGEN hark_01_overview_hotpath -->

---

<!-- BEGIN:AUTOGEN hark_01_overview_navigate -->
## Where to Start

| Goal | Start Here |
|---|---|
| Give an AI agent the fastest safe orientation | [Agent Guide](../AGENTS.md) |
| Understand process/thread structure and the pipeline in depth | [Architecture](core/ARCHITECTURE.md) |
| Install or build the app locally | [Getting Started](GETTING_STARTED.md) |
| Look up a domain term or acronym | [Glossary](GLOSSARY.md) |
| Understand settings, defaults, and the BYOK keychain | [Configuration and Secrets](core/CONFIGURATION.md) |
| Understand the SQLite history/stats schema | [Data Storage](core/DATA_STORAGE.md) |
| Understand a specific pipeline stage (audio, STT, spellbook, cleanup, injection) | see the `features/` pages |

Sources: navigation curated by doc-sync from [`Docs/_toc.yaml`](_toc.yaml)
<!-- END:AUTOGEN hark_01_overview_navigate -->

---
</content>

# Hark Documentation

> **Latest Updates (July 2026):**
> - **v0.20.0:** Invocations — say a trigger phrase, get a block of text you wrote, injected verbatim and never reworded by a cleanup voice.
> - **v0.19.1:** Groundwork for a Gemini adapter that transcribes and cleans in one request (not yet selectable in Settings).
> - **v0.19.0:** A Test button for the cleanup provider, and a pinned unsaved-changes bar in Settings.
> - **v0.18.0:** Opt-in on-device transcription with a downloadable Parakeet model, usable as a cloud backup or as the primary engine.
> - **v0.17.0:** Quieter microphones are no longer dropped; peak-window silence gating, per-room noise floor, and a live input meter.
> - **v0.16.0:** Only one Hark runs at a time; a second launch exits quietly instead of fighting for the push-to-talk key.
> - **v0.14.0:** Windows installer (per-user, no admin), launch-at-login toggle, and a microphone picker in Settings.
> - **v0.13.7:** Retired the internal `hark-cli` dev binary; `hark-app` is the sole entry point.
> - **v0.13.6:** In-app update checker with Windows self-update, plus per-device microphone selection.
> - **v0.13.0:** Hark moved into the system tray with a state-reflecting icon and voice selector.

This is the canonical wiki for Hark, a push-to-talk voice dictation desktop app for Windows and macOS, written in Rust with BYOK cloud transcription. Every page is generated from the source tree and cites the exact files and line ranges it describes.

## Quick Start

| Goal | Start Here |
|------|------------|
| **Understand the system** | [ARCHITECTURE.md](core/ARCHITECTURE.md) |
| **Run or install the app** | [GETTING_STARTED.md](GETTING_STARTED.md) |
| **What Hark is and how it is laid out** | [OVERVIEW.md](OVERVIEW.md) |
| **Configure settings and the BYOK key** | [CONFIGURATION.md](core/CONFIGURATION.md) |
| **How dictation turns speech into text** | [TRANSCRIPTION.md](features/TRANSCRIPTION.md) |
| **Transcribe offline, with no provider** | [ON_DEVICE_STT.md](features/ON_DEVICE_STT.md) |
| **Cut a signed release** | [RELEASE_AND_PACKAGING.md](operations/RELEASE_AND_PACKAGING.md) |
| **Look up a term** | [GLOSSARY.md](GLOSSARY.md) |

---

## Core

Foundational architecture, configuration, and data.

| Document | Description |
|----------|-------------|
| [ARCHITECTURE.md](core/ARCHITECTURE.md) | Process model, the main-thread/worker-thread split, the release-to-inject pipeline, and its state machine. |
| [CONFIGURATION.md](core/CONFIGURATION.md) | The TOML settings schema, defaults, override order, and how the BYOK key lives in the OS keychain. |
| [DATA_STORAGE.md](core/DATA_STORAGE.md) | The SQLite history and lifetime-stats schema, retention pruning, and the store API. |

---

## Features

One page per major subsystem of the dictation pipeline and the desktop shell.

| Document | Description |
|----------|-------------|
| [AUDIO_CAPTURE.md](features/AUDIO_CAPTURE.md) | The cpal ring buffer with pre-roll and tail, silence trimming, and the native push-to-talk key hooks. |
| [TRANSCRIPTION.md](features/TRANSCRIPTION.md) | The `SttProvider` trait, the OpenAI-compatible and Deepgram adapters, WAV encoding, and biasing. |
| [ON_DEVICE_STT.md](features/ON_DEVICE_STT.md) | Opt-in local Parakeet model: download manager, the cloud/local fallback policy, and the `local-engine` build feature. |
| [DICTIONARY.md](features/DICTIONARY.md) | Phonetic post-correction of transcripts plus per-provider biasing term generation. |
| [INVOCATIONS.md](features/INVOCATIONS.md) | User-authored trigger phrases that inject canned text verbatim, skipping the cleanup pass. |
| [VOICE_CLEANUP.md](features/VOICE_CLEANUP.md) | Voice presets and the optional BYOK LLM cleanup pass over the raw transcript. |
| [TEXT_INJECTION.md](features/TEXT_INJECTION.md) | Clipboard stash-set-paste-restore injection with an enigo keystroke fallback. |
| [DESKTOP_UI.md](features/DESKTOP_UI.md) | The tray daemon, the recording overlay, and the egui settings/history/stats window. |
| [UPDATES_AND_AUTOSTART.md](features/UPDATES_AND_AUTOSTART.md) | The in-app update checker with Windows self-update, and launch-at-login registration. |

---

## Operations

Release engineering and packaging.

| Document | Description |
|----------|-------------|
| [RELEASE_AND_PACKAGING.md](operations/RELEASE_AND_PACKAGING.md) | The Windows installer, the signed GitHub release workflow, and the version-lockstep rule. |

---

## Reference

| Document | Description |
|----------|-------------|
| [OVERVIEW.md](OVERVIEW.md) | What Hark is, its design principles, tech stack, and crate layout. |
| [GETTING_STARTED.md](GETTING_STARTED.md) | Prerequisites, installing on Windows, building from source, and first-run setup. |
| [GLOSSARY.md](GLOSSARY.md) | Domain terms and acronyms used across Hark's documentation. |

---

## Related Resources

| Resource | Location |
|----------|----------|
| Repo README | [../README.md](../README.md) |
| Project rules & stack | [../CLAUDE.md](../CLAUDE.md) |
| Agent registry | [../agents.md](../agents.md) |
| Full plan & rationale | [../tasks/plan-repo.md](../tasks/plan-repo.md) |

---

**Last Updated:** July 2026 · Generated from commit `6a33396`

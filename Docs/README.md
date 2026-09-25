# Hark Documentation

> **Latest Updates (September 2026):**
> - **v0.47.0:** Invocations can recognize exact alternate phrases for repeatable provider mishearings.
> - **v0.46.0:** Push-to-talk survives intercepted/remapped keys and surfaces an advisory warning naming the key.
> - **v0.45.x:** Bounded shutdown and live-session recovery prevent stuck dictations from freezing Hark; Gemini streaming now retains all finalized segments.
> - **v0.44.0:** Gemini Smart/Verbatim behavior, Gemini cleanup voices, and provider error diagnostics were completed.
> - **v0.41.0:** Gemini Live streams audio during the hold, with finished-clip fallback if the live path fails.
> - **v0.40.0:** Gemini Live and OpenAI `gpt-transcribe` became selectable STT providers.
> - **v0.39.0:** The Windows portable download is gone; the installer is the only one, and updates now run it instead of swapping the running program. One manual install is needed to cross this version.
> - **v0.20.0:** Invocations — say a trigger phrase, get a block of text you wrote, injected verbatim and never reworded by a cleanup voice.
> - **v0.18.0:** Opt-in on-device transcription with a downloadable Parakeet model, usable as a cloud backup or as the primary engine.

This is the canonical wiki for Hark, a push-to-talk voice dictation desktop app targeting Windows, macOS and Linux, written in Rust with BYOK cloud transcription and optional on-device STT. Windows and Linux have end-to-end push-to-talk today; the macOS CGEventTap hook remains an explicit implementation gap. Generated sections cite the source they describe; `_meta/SUMMARY.md` records known gaps rather than implying every page is current.

## Quick Start

| Goal | Start Here |
|------|------------|
| **Understand the system** | [ARCHITECTURE.md](core/ARCHITECTURE.md) |
| **Orient an AI coding agent** | [../AGENTS.md](../AGENTS.md) |
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
| [DATA_STORAGE.md](core/DATA_STORAGE.md) | The local SQLite schema, history and stats semantics, retention, worker integration, and bounded shutdown. |

---

## Features

One page per major subsystem of the dictation pipeline and the desktop shell.

| Document | Description |
|----------|-------------|
| [AUDIO_CAPTURE.md](features/AUDIO_CAPTURE.md) | The cpal ring, pre-roll/tail, loudness gates, shortcut capture, and native push-to-talk hooks. |
| [TRANSCRIPTION.md](features/TRANSCRIPTION.md) | Batch/live traits, Deepgram, OpenAI-compatible, gpt-transcribe, Gemini Live, WAV, and error contracts. |
| [ON_DEVICE_STT.md](features/ON_DEVICE_STT.md) | Opt-in local Parakeet model: download manager, the cloud/local fallback policy, and the `local-engine` build feature. |
| [SPELLBOOK.md](features/SPELLBOOK.md) | Canonical terms, exact aliases, guarded phonetic post-correction, and per-provider vocabulary hints. |
| [INVOCATIONS.md](features/INVOCATIONS.md) | User-authored trigger phrases that inject canned text verbatim, skipping the cleanup pass. |
| [VOICE_CLEANUP.md](features/VOICE_CLEANUP.md) | Voice presets, cleanup-provider resolution, guarded fail-open rewriting, and fused-provider behavior. |
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
| Tool-neutral agent guide | [../AGENTS.md](../AGENTS.md) |
| Project rules & stack | [../CLAUDE.md](../CLAUDE.md) |
| Claude subagent registry | [../agents.md](../agents.md) |
| Full plan & rationale | [../tasks/plan-repo.md](../tasks/plan-repo.md) |

---

**Last Updated:** September 24, 2026 · Source baseline `784272c`

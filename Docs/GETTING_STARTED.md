<!-- PAGE_ID: hark_03_getting_started -->
<details>
<summary>Relevant source files</summary>

The following files were used as evidence for this page:

- [README.md:3](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/README.md#L3)
- [README.md:59-63](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/README.md#L59-L63)
- [README.md:67-79](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/README.md#L67-L79)
- [README.md:81-94](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/README.md#L81-L94)
- [Cargo.toml:19-27](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/Cargo.toml#L19-L27)
- [config/default-config.toml:9-22](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/config/default-config.toml#L9-L22)

</details>

# Getting Started

> **Related Pages**: [Overview](OVERVIEW.md), [Configuration and Secrets](core/CONFIGURATION.md), [Release and Packaging](operations/RELEASE_AND_PACKAGING.md)

---

<!-- BEGIN:AUTOGEN hark_03_getting_started_prerequisites -->
## Prerequisites

Hark is a native Rust desktop app. Cloud transcription is BYOK by default; the optional on-device Parakeet model can be downloaded after install and used without a provider key ([README.md:1-10](../README.md#L1-L10)).

| Requirement | Details |
|---|---|
| Rust toolchain | Stable Rust via [rustup](https://rustup.rs), providing `cargo`, `rustfmt`, and `clippy` ([README.md:65-69](../README.md#L65-L69)) |
| Minimum Rust version | `1.97`, required by the workspace and bundled SQLite dependency ([Cargo.toml:21-29](../Cargo.toml#L21-L29)) |
| STT access | A key for Deepgram, OpenAI, Groq, Gemini, or another compatible endpoint; not required when local STT is primary ([README.md:67-69](../README.md#L67-L69)) |
| Platform build tools | Xcode command-line tools on macOS; MSVC on Windows; ALSA, GTK, AppIndicator, X11/XKB, and build packages listed in the README on Linux ([README.md:69-77](../README.md#L69-L77)) |

Sources: [README.md:1-10](../README.md#L1-L10), [README.md:65-77](../README.md#L65-L77), [Cargo.toml:21-29](../Cargo.toml#L21-L29)
<!-- END:AUTOGEN hark_03_getting_started_prerequisites -->

---

<!-- BEGIN:AUTOGEN hark_03_getting_started_install -->
## Install on Windows

The simplest way to run Hark on Windows is the signed setup executable published with each release.

- Download `Hark-<version>-windows-x64-setup.exe` from the [Releases page](https://github.com/BoardPandas/Hark/releases/latest) and run it ([README.md:82-83](../README.md#L82-L83)).
- The installer runs per-user with no admin prompt, and installs to `%LOCALAPPDATA%\Programs\Hark` ([README.md:84-85](../README.md#L84-L85)).
- It adds a Start Menu shortcut, and by default Hark starts hidden in the system tray at Windows sign-in; this is controlled by **Settings → General → "Launch Hark at startup"** ([README.md](../README.md#install-windows)).
- The installer is the **only** Windows download. A portable `Hark-<version>-windows-x64.exe` used to be attached to each release and no longer is: it produced an install Windows had no record of — no entry in Add or remove programs, no upgrade path — and it was also the file the in-app updater installed over itself ([README.md:89-94](../README.md#L89-L94)).
- To remove Hark, use **Add or remove programs**; settings and history in `%APPDATA%\hark` are left in place ([README.md:96-97](../README.md#L96-L97)).

Sources: [README.md:80-97](../README.md#L80-L97)
<!-- END:AUTOGEN hark_03_getting_started_install -->

---

<!-- BEGIN:AUTOGEN hark_03_getting_started_build -->
## Build from Source

Building from source needs `git`, stable Rust, and the platform packages from Prerequisites. Model weights are not downloaded by the build; the optional local model is an explicit in-app download.

```bash
git clone <this-repo> Hark
cd Hark

cargo build
cargo run

# Add a provider key in Settings, or download the optional local model and
# select it as the primary engine.
```

Sources: [README.md:134-147](../README.md#L134-L147), [README.md:188-190](../README.md#L188-L190)
<!-- END:AUTOGEN hark_03_getting_started_build -->

---

<!-- BEGIN:AUTOGEN hark_03_getting_started_firstrun -->
## First-Run Setup

On first run, Hark prompts for a speech-to-text provider key in Settings; the key is written to the OS keychain and is never persisted to `config.toml` ([README.md:62](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/README.md#L62); [config/default-config.toml:9-11](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/config/default-config.toml#L9-L11)).

The `[provider]` block in the default config selects which adapter handles transcription:

```toml
[provider]
kind = "deepgram"    # deepgram | openai | groq | gemini | openai-compatible
# base_url and model default per kind:
#   deepgram -> https://api.deepgram.com, nova-3
#   openai   -> https://api.openai.com/v1, gpt-transcribe
#   groq     -> https://api.groq.com/openai/v1, whisper-large-v3-turbo
#   gemini   -> Live API WebSocket, gemini-3.5-transcribe-live
# "openai-compatible" is the escape hatch for any other server speaking the
# multipart /audio/transcriptions contract; it requires an explicit base_url.
```

Each `kind` carries its own default endpoint and model, and `openai-compatible` is the escape hatch for any other server speaking the same multipart contract, at the cost of requiring an explicit `base_url` ([config/default-config.toml:13-20](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/config/default-config.toml#L13-L20)):

| `kind` | Default `base_url` | Default `model` |
|---|---|---|
| `deepgram` | `https://api.deepgram.com` | `nova-3` |
| `openai` | `https://api.openai.com/v1` | `gpt-transcribe` |
| `groq` | `https://api.groq.com/openai/v1` | `whisper-large-v3-turbo` |
| `gemini` | Live API WebSocket (fixed host) | `gemini-3.5-transcribe-live` |
| `openai-compatible` | none, must be set explicitly | none, must be set explicitly |

`gemini` is the only provider that streams: the session opens when you press the chord and audio goes up as you speak, so releasing the key leaves only the tail outstanding. If the session cannot open — or fails at any point during the hold — Hark falls back to the ordinary single-request path with no dictation lost. Streamed audio is sent at capture level, because the batch path's whole-clip gain normalization has no streaming equivalent; on a very quiet microphone the batch path may still transcribe slightly better.

Sources: [config/default-config.toml:9-22](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/config/default-config.toml#L9-L22)
<!-- END:AUTOGEN hark_03_getting_started_firstrun -->

---

<!-- BEGIN:AUTOGEN hark_03_getting_started_dev -->
## Development Workflow

Local development uses the same rustup toolchain called out in Prerequisites: `cargo` for building, `rustfmt` for formatting, and `clippy` for linting ([README.md:61](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/README.md#L61)).

- Build and run day-to-day with `cargo build` / `cargo run`, the same commands used for a from-source install ([README.md:87-88](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/README.md#L87-L88)).
- This machine is intentionally coding-only: build, test, lint, and typecheck here, then validate the actually-running app, mic permissions, the push-to-talk hotkey, text injection, and notarization/signing, on real macOS and Windows hardware ([README.md:94](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/README.md#L94)).

Sources: [README.md:61](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/README.md#L61), [README.md:87-88](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/README.md#L87-L88), [README.md:94](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/README.md#L94)
<!-- END:AUTOGEN hark_03_getting_started_dev -->

---

<!-- BEGIN:AUTOGEN hark_03_getting_started_next -->
## Where to Go Next

| Page | What it covers |
|---|---|
| [Configuration and Secrets](core/CONFIGURATION.md) | The full TOML settings schema, defaults, override order, and how the BYOK key lives in the OS keychain |
| [Architecture](core/ARCHITECTURE.md) | The main-thread/worker-thread process model and the release-to-inject pipeline |
| [Transcription (STT Providers)](features/TRANSCRIPTION.md) | Batch and live provider traits, Deepgram, OpenAI-compatible, gpt-transcribe, and Gemini Live |
| [On-Device STT](features/ON_DEVICE_STT.md) | Optional Parakeet model, downloads, and local-primary/cloud-backup modes |
<!-- END:AUTOGEN hark_03_getting_started_next -->

---

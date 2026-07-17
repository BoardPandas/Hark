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

Hark is a native Rust desktop app: there is no runtime to install and no model to download, since transcription runs on a BYOK cloud provider rather than on-device ([README.md:3](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/README.md#L3)).

| Requirement | Details |
|---|---|
| Rust toolchain | Stable Rust via [rustup](https://rustup.rs), providing `cargo`, `rustfmt`, and `clippy` ([README.md:61](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/README.md#L61)) |
| Minimum Rust version | `1.97`, required because `libsqlite3-sys` 0.38.1 (rusqlite's bundled SQLite) uses `cfg_select!`, which is unstable before that version ([Cargo.toml:25-27](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/Cargo.toml#L25-L27)) |
| STT provider key | An API key for OpenAI, Groq, or Deepgram, entered in Settings on first run and stored in the OS keychain ([README.md:62](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/README.md#L62)) |
| Platform build tools | Xcode command-line tools on macOS; MSVC build tools on Windows ([README.md:63](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/README.md#L63)) |

Sources: [README.md:3](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/README.md#L3), [README.md:59-63](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/README.md#L59-L63), [Cargo.toml:25-27](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/Cargo.toml#L25-L27)
<!-- END:AUTOGEN hark_03_getting_started_prerequisites -->

---

<!-- BEGIN:AUTOGEN hark_03_getting_started_install -->
## Install on Windows

The simplest way to run Hark on Windows is the signed setup executable published with each release.

- Download `Hark-<version>-windows-x64-setup.exe` from the [Releases page](https://github.com/BoardPandas/Hark/releases/latest) and run it ([README.md:69-70](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/README.md#L69-L70)).
- The installer runs per-user with no admin prompt, and installs to `%LOCALAPPDATA%\Programs\Hark` ([README.md:71-72](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/README.md#L71-L72)).
- It adds a Start Menu shortcut, and by default Hark starts hidden in the system tray at Windows sign-in; this is controlled by **Settings → Behavior → "Launch Hark at login"** ([README.md:72-74](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/README.md#L72-L74)).
- A portable `Hark-<version>-windows-x64.exe` is attached to each release for anyone who prefers not to install ([README.md:75-76](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/README.md#L75-L76)).
- To remove Hark, use **Add or remove programs**; settings and history in `%APPDATA%\hark` are left in place ([README.md:78-79](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/README.md#L78-L79)).

Sources: [README.md:67-79](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/README.md#L67-L79)
<!-- END:AUTOGEN hark_03_getting_started_install -->

---

<!-- BEGIN:AUTOGEN hark_03_getting_started_build -->
## Build from Source

Building from source only needs `git` and the Rust toolchain from Prerequisites; no model weights or other assets are downloaded, since transcription is BYOK cloud ([README.md:90-92](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/README.md#L90-L92)).

```bash
git clone <this-repo> Hark
cd Hark

cargo build
cargo run

# No model to download — transcription is BYOK cloud. Add your speech-to-text
# provider key (OpenAI, Groq, or Deepgram) in Settings on first run.
```

Sources: [README.md:81-94](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/README.md#L81-L94)
<!-- END:AUTOGEN hark_03_getting_started_build -->

---

<!-- BEGIN:AUTOGEN hark_03_getting_started_firstrun -->
## First-Run Setup

On first run, Hark prompts for a speech-to-text provider key in Settings; the key is written to the OS keychain and is never persisted to `config.toml` ([README.md:62](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/README.md#L62); [config/default-config.toml:9-11](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/config/default-config.toml#L9-L11)).

The `[provider]` block in the default config selects which adapter handles transcription:

```toml
[provider]
kind = "deepgram"    # deepgram | openai | groq | openai-compatible
# base_url and model default per kind:
#   deepgram -> https://api.deepgram.com, nova-3
#   openai   -> https://api.openai.com/v1, gpt-4o-mini-transcribe
#   groq     -> https://api.groq.com/openai/v1, whisper-large-v3-turbo
# "openai-compatible" is the escape hatch for any other server speaking the
# multipart /audio/transcriptions contract; it requires an explicit base_url.
```

Each `kind` carries its own default endpoint and model, and `openai-compatible` is the escape hatch for any other server speaking the same multipart contract, at the cost of requiring an explicit `base_url` ([config/default-config.toml:13-20](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/config/default-config.toml#L13-L20)):

| `kind` | Default `base_url` | Default `model` |
|---|---|---|
| `deepgram` | `https://api.deepgram.com` | `nova-3` |
| `openai` | `https://api.openai.com/v1` | `gpt-4o-mini-transcribe` |
| `groq` | `https://api.groq.com/openai/v1` | `whisper-large-v3-turbo` |
| `openai-compatible` | none, must be set explicitly | none, must be set explicitly |

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
| [Transcription (STT Providers)](features/TRANSCRIPTION.md) | The `SttProvider` trait and the OpenAI-compatible and Deepgram adapters |
<!-- END:AUTOGEN hark_03_getting_started_next -->

---

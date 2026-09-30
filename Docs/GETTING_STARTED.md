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

Hark is a native Rust desktop app. Cloud transcription is BYOK by default; the optional on-device Parakeet model can be downloaded after install and used without a provider key for dictation and live meeting chunks ([README: On-device transcription](../README.md#on-device-transcription)). Meetings is available on Windows and has separate Deepgram final-pass and LLM notes settings; local Primary alone does not disable those provider requests ([README: Privacy](../README.md#privacy)).

| Requirement | Details |
|---|---|
| Rust toolchain | Stable Rust via [rustup](https://rustup.rs), providing `cargo`, `rustfmt`, and `clippy` ([README: Prerequisites](../README.md#prerequisites)) |
| Minimum Rust version | `1.97`, required by the workspace and bundled SQLite dependency ([Cargo.toml:22-30](../Cargo.toml#L22-L30)) |
| STT access | A key for Deepgram, OpenAI, Groq, Gemini, or another compatible endpoint; not required for live STT when local Primary is selected ([README: Prerequisites](../README.md#prerequisites)) |
| Platform build tools | Xcode command-line tools on macOS; MSVC on Windows; ALSA, GTK, AppIndicator, X11/XKB, and build packages listed in the README on Linux ([README: Prerequisites](../README.md#prerequisites)) |

Sources: [README: Features](../README.md#features), [README: Prerequisites](../README.md#prerequisites), [Cargo.toml:22-30](../Cargo.toml#L22-L30)
<!-- END:AUTOGEN hark_03_getting_started_prerequisites -->

---

<!-- BEGIN:AUTOGEN hark_03_getting_started_install -->
## Install on Windows

The simplest way to run Hark on Windows is the signed setup executable published with each release.

- Download `Hark-<version>-windows-x64-setup.exe` from the [Releases page](https://github.com/BoardPandas/Hark/releases/latest) and run it ([README: Install (Windows)](../README.md#install-windows)).
- The installer runs per-user with no admin prompt, and installs to `%LOCALAPPDATA%\Programs\Hark` ([README: Install (Windows)](../README.md#install-windows)).
- It adds a Start Menu shortcut, and by default Hark starts hidden in the system tray at Windows sign-in; this is controlled by **Settings → General → "Launch Hark at startup"** ([README.md](../README.md#install-windows)).
- The installer is the **only** Windows download. A portable `Hark-<version>-windows-x64.exe` used to be attached to each release and no longer is: it produced an install Windows had no record of — no entry in Add or remove programs, no upgrade path — and it was also the file the in-app updater installed over itself ([README: Install (Windows)](../README.md#install-windows)).
- To remove Hark, use **Add or remove programs**; settings and history in `%APPDATA%\hark` are left in place ([README: Install (Windows)](../README.md#install-windows)).

After installation, open **Meetings** to start manually or accept a detected-call prompt. On Windows, Settings → Meetings → **Start / stop shortcut** optionally binds a chord such as `LCtrl+F11`; leave it blank to keep using the buttons. The shortcut has the same recording scope as manual Start and works independently of dictation credentials ([meeting shortcut](features/MEETINGS.md#start--stop-shortcut)). The default **Ask me** setting offers to record; **Start taking notes on its own** starts detected calls without another prompt. Manual Start records your mic and all playback except Hark, so check the [recording scope and privacy settings](../README.md#meetings-windows) before starting. The default audio cap keeps completed recordings under 5 GiB while preserving their transcripts and notes. Open a completed meeting with retained audio and choose **Re-run final pass** to request another Deepgram transcription. Confirmation explains the upload and charges; a successful save replaces the transcript and resets speaker names while preserving notes ([saved-meeting reruns](features/MEETINGS.md#re-run-a-saved-meeting)).

For automatic post-call refinement, Settings > Meetings also offers **Use Gemini after the call**. Add your Gemini key and choose its independent model; remote speaker numbers restart per five-minute window. This does not change the explicit Deepgram saved-recording re-run action ([Gemini processing and deletion](features/MEETINGS.md#gemini-files-final-pass)).

For speaker playback, optionally enable **Reduce speaker echo** in Settings →
Meetings and save before starting the next meeting. It defaults off and filters
only the microphone recording and its live transcript. Leave it off with
headphones, or turn it off if the local voice sounds distorted. It uses only the
captured playback reference; actual speakerphone quality remains unverified
([behavior and limits](features/MEETINGS.md#reduce-speaker-echo)).

The meeting detail view's **Share** menu can save Markdown/text, SRT/VTT subtitles, a Word document, or audio. **Save an excerpt with audio** lets you choose transcript lines or a time range and writes audio plus matching text; **Share with Windows** opens the native chooser for meeting text ([sharing details](features/MEETINGS.md#sharing-and-export)).

Sources: [README: Install (Windows)](../README.md#install-windows), [README: Meetings privacy](../README.md#meetings-windows)
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

Sources: [README: Build from source](../README.md#build-from-source), [README: On-device transcription](../README.md#on-device-transcription)
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
| [Meetings](features/MEETINGS.md) | Recording a call in any app with no bot, on Windows for now: capture, live transcript, notes, storage, and sharing. By default Hark asks when a call starts and stops the notes 15 s after it ends |
<!-- END:AUTOGEN hark_03_getting_started_next -->

---

## Mac setup

The Mac build targets macOS 14.2+ on Apple Silicon and Intel. Use the [Mac build and distribution instructions](../packaging/macos/README.md), install `Hark.app` in Applications, then grant Microphone, Input Monitoring and Accessibility from Hark’s Settings → General → Permissions. Retry dictation after changing access, or restart when macOS requires it. The default held shortcut is Control + Command. System-audio recording for meetings has a separate permission; browser call-title detection may also require Screen Recording. Development bundles are ad-hoc signed; distributed releases need Developer ID signing and notarization.

# Hark

A lean, system-wide, push-to-talk voice dictation tool for **Windows**, **macOS** and **Linux**. Hold a key, speak, release — polished English text is injected at your cursor in any app. Transcription is **bring-your-own-key cloud** by default (you supply your own speech-to-text provider key), with an **optional on-device model** that transcribes without the internet or a key at all; history, stats, the spellbook, and your invocations stay local on your machine; cleanup is optional and uses your own LLM key. Windows and Linux have end-to-end push-to-talk today; macOS has native UI/tray/keychain/injection paths, but its CGEventTap hotkey hook is still a planned seam.

> Wispr Flow-style dictation, scoped to one user, English-only, and local-first.

## Features

- **Push-to-talk dictation:** hold a shortcut, speak, and release to type polished English in the focused app.
- **Your providers or an on-device model:** bring your own cloud keys, or use Parakeet for local dictation.
- **Spellbook and invocations:** correct your vocabulary and expand spoken phrases into text you wrote.
- **Meetings (Windows):** record your microphone and meeting audio without a bot, follow a live Me/Them transcript, and get speaker labels and notes with your own provider keys. Start and stop with an optional Windows shortcut, search transcripts, rename speakers, re-run the final pass on retained recordings, and share text, Word, subtitle, or audio files, including selected excerpts. See [Meetings](Docs/features/MEETINGS.md) and the [privacy details](#privacy) below.

## Design principles

- **Speed is the product.** All perceived latency lives in the release-to-inject window; everything is structured to keep it small.
- **Local-first where it counts.** History, stats, and settings are stored on your machine. Cloud transcription, optional cleanup, and meeting processing use your own provider keys. On-device Primary keeps dictation audio local; Meetings has separate final-pass and notes settings. No Hark-operated servers, ever.
- **Lean.** No webview, no browser tab, no JS toolchain. A single Rust process: an always-on tray daemon plus a native window opened on demand.
- **English done well.** Accuracy over language breadth.
- **Data, not code, for anything you tune.** The spellbook and voice presets are config, so editing them never touches the pipeline.

## Tech stack

Desktop app — **no web infrastructure** (no server, database service, auth, or hosting platform).

| Layer | Choice |
|---|---|
| Language | Rust (UI on main thread, pipeline on worker threads) |
| Audio | `cpal` (device-rate mono ring buffer, resampled to 16 kHz per clip) |
| Push-to-talk | `WH_KEYBOARD_LL` (Windows) and `evdev` (Linux — X11 and Wayland); CGEventTap remains the planned macOS seam |
| STT | BYOK cloud via an `SttProvider` trait: Deepgram, Whisper-family OpenAI-compatible endpoints, OpenAI `gpt-transcribe`, and Gemini Live |
| STT transport | Blocking `reqwest` adapters on worker threads plus a Gemini-only private current-thread Tokio/WebSocket runtime; no global runtime |
| Spellbook | Phonetic post-correction (primary, provider-agnostic) + per-provider biasing (`prompt`, `keywords[]`, `keyterm`, or Gemini `customVocabulary`) |
| Invocations | Trigger phrase → canned text, matched by the same guarded phonetic matcher at a tighter confirm threshold; injected verbatim, cleanup skipped |
| Cleanup / voices | Bring-your-own-key, OpenAI-compatible chat endpoint (optional) |
| Injection | Clipboard paste, `enigo` keystroke fallback (a `uinput` virtual keyboard on Wayland, which has no XTEST) |
| Tray + UI | `tray-icon` + `eframe`/`egui` (native, no webview) |
| Storage | `rusqlite` (history + stats), TOML (settings + spellbook) |
| Key storage | `keyring` → macOS Keychain / Windows Credential Manager / Secret Service (Linux) |

See [`tasks/plan-repo.md`](tasks/plan-repo.md) for the full rationale and the current-as-of-2026-07-15 research corrections.

## Architecture

```
key down ─▶ cpal ring buffer (with ~200–300 ms pre-roll)
              ├── Gemini Live selected? ─▶ stream audio during the hold
              │
key up  ─────▶ append ~150 ms tail; finish the live turn if one survived
              │
              ▼  gate too-short/too-quiet audio, normalize, encode WAV
       otherwise send the finished clip to the STT provider
       (or replay it after a failed live stream; at most one retry total)
              │
              ▼  phonetic post-correction against spellbook
              │
              ▼  invocation trigger matched? ── yes ─▶ inject canned text verbatim
              │ no                                     (cleanup skipped entirely)
              ▼
        voice == Verbatim? ── yes ─▶ inject raw transcript
              │ no
              ▼
     single BYOK LLM call (low temp): voice template + spellbook terms + transcript
              │
              ▼  inject via clipboard paste (stash → set → paste → restore)
              │
              ▼  write history (if capture enabled) + increment lifetime stats
```

The tray daemon owns the hot path (hotkey, audio, STT, injection). The settings/history window opens on demand. On macOS the main thread owns the event loop (tray + window); the pipeline runs on worker threads. On Linux the tray is the one exception: libappindicator builds it out of GTK widgets, which need a GTK main loop that cannot share a thread with winit's, so it runs on a thread of its own.

## Prerequisites

- **Rust** (stable) via [rustup](https://rustup.rs) — `cargo`, `rustfmt`, `clippy`.
- **A speech-to-text provider key** (Deepgram, OpenAI, Groq, Gemini, or an OpenAI-compatible endpoint) — entered in Settings on first run, stored in the OS keychain. Not required if you set the on-device model as your primary engine (see below).
- Platform build tools: Xcode command-line tools (macOS); MSVC build tools (Windows); on Linux, the dev headers Hark links against:

  ```bash
  # Debian / Ubuntu
  sudo apt install libasound2-dev libgtk-3-dev libayatana-appindicator3-dev \
    libxdo-dev libxkbcommon-dev libx11-dev pkg-config cmake clang
  # Arch
  sudo pacman -S alsa-lib gtk3 libappindicator-gtk3 xdotool libxkbcommon cmake clang pkgconf
  ```

## Getting started

### Install (Windows)

Download the latest **`Hark-<version>-windows-x64-setup.exe`** from the
[Releases page](https://github.com/BoardPandas/Hark/releases/latest) and run it.
The installer is per user (no admin prompt), installs to
`%LOCALAPPDATA%\Programs\Hark`, and adds a Start Menu shortcut. Unless you turn
it off, Hark starts hidden in the system tray when you sign in to Windows;
toggle that under **Settings → General → "Launch Hark at startup"**.

**Settings → General** also includes **Always on top** and **Exit when the
window is closed**. By default, the X hides Hark in the system tray; enable
the exit option and save to make it shut down Hark instead. **Close Program**
always fully exits Hark, including background dictation.

The installer is the only Windows download. A portable
`Hark-<version>-windows-x64.exe` used to ship alongside it and no longer does:
it produced an install Windows knew nothing about — no entry in Add or remove
programs, and no upgrade path — while being the very file the in-app updater
installed over itself. Updates now run the installer, so the version Windows
reports and the version you are running stay the same thing.

To remove Hark, use **Add or remove programs**. Your settings and history in
`%APPDATA%\hark` are left in place.

### Install (Linux)

Download the package for your distribution from the
[Releases page](https://github.com/BoardPandas/Hark/releases/latest):

```bash
sudo apt install ./Hark-<version>-linux-x64.deb          # Debian, Ubuntu, Mint, Pop!_OS
sudo dnf install ./Hark-<version>-linux-x64.rpm          # Fedora, RHEL, openSUSE
sudo pacman -U Hark-<version>-linux-x64.pkg.tar.zst      # Arch, Manjaro, EndeavourOS
```

Then grant Hark permission to see the push-to-talk chord and to paste, and log
back in so the group takes effect:

```bash
sudo usermod -aG input $USER
```

That single step is the whole Linux-specific setup. Hark reads `/dev/input`
directly rather than going through the display server, which is what makes
push-to-talk work identically on X11 and Wayland — the X11 grab APIs other
tools use are invisible to a Wayland compositor. Hark never grabs a device and
never swallows a keystroke; every key still reaches the app you are typing in.

**[packaging/LINUX.md](packaging/LINUX.md)** has the full story: why the
permission is needed, a portable tarball with manual install steps, the glibc
floor, troubleshooting, and the handful of places Linux cannot match Windows
exactly.

### Build from source

```bash
git clone <this-repo> Hark
cd Hark

cargo build
cargo run

# Add a speech-to-text provider key in Settings on first run, or download the
# optional on-device model and select it as the primary engine.
```

> **Note:** this machine is a coding-only environment. Build, test, lint, and typecheck here; run and validate the running app (mic, hotkey, injection, notarization) on real macOS, Windows and Linux.

## Project structure

Cargo workspace; single binary. See [`tasks/plan-repo.md`](tasks/plan-repo.md) §5.

```
crates/
  hark-app/          # main-thread event loop, worker orchestration, single-instance
                     #   guard, and the egui settings/history/stats window (src/ui/)
  hark-hotkey/       # Windows/Linux hooks + shared chord tracker; macOS seam pending
  hark-audio/        # cpal ring buffer, pre-roll + tail
  hark-stt/          # cloud adapters, including Gemini Live streaming
  hark-local-stt/    # optional sherpa-onnx Parakeet engine
  hark-spellbook/   # phonetic post-correction, invocation trigger matching,
                     #   and per-provider biasing terms
  hark-voice/        # voice presets + BYOK cleanup adapter
  hark-inject/       # clipboard paste + enigo fallback
  hark-pipeline/     # release-to-inject orchestration across worker threads
  hark-meeting/      # meeting lifecycle, detection, transcript, storage cap, exports
  hark-store/        # rusqlite (history + stats)
  hark-config/       # TOML settings + spellbook load/save
  hark-keychain/     # keyring wrapper (BYOK key in the OS keychain)
  hark-autostart/    # launch-at-login (Windows registry / XDG autostart / macOS login item)
  hark-update/       # update checker + Windows installer handoff
  hark-single-instance/ # one-process guard
config/              # default config.toml + spellbook
installer/           # Inno Setup script for the Windows installer
packaging/           # Linux: .desktop, icon, udev rule, PKGBUILD, LINUX.md
```

## Configuration

No web env vars. Settings and secrets live in OS-standard locations:

| Item | Location |
|---|---|
| `config.toml` (hotkey, default voice, BYOK provider/model, spellbook, invocations, capture toggle, retention cap) | OS config dir (`~/Library/Application Support/hark/`, `%APPDATA%\hark\`) |
| `hark.db` (history, stats, meeting transcripts, speaker names, and notes) | OS data dir (`%APPDATA%\hark\` on Windows) |
| Meeting recordings (Windows) | `%APPDATA%\hark\meetings\<id>\`; WAV while processing, stereo MP3 by default afterward |
| BYOK API key | OS keychain — never written to `config.toml` |
| On-device model weights (~670 MB, only if you download them) | OS data dir, under `models/` |

## On-device transcription

Optional, off by default, and included in every stock build. Under **Settings →
On-device** you can download a Parakeet model (~670 MB) and then run it in
one of two modes:

- **Backup** — the cloud provider stays primary; Hark falls back to the local
  model when the network or the provider fails, so you do not lose the sentence
  you just spoke.
- **Primary** — dictation and live meeting chunks are transcribed on this machine,
  with no speech-to-text provider key. Meetings' optional Deepgram/Gemini final pass and
  notes generation have separate settings and can still contact cloud providers.

The spellbook's phonetic correction applies to local transcripts exactly as it
does to cloud ones, so your custom vocabulary works either way.

The native engine is compiled in by the `local-engine` Cargo feature, which is
**on by default**. `cargo build --no-default-features` produces a slimmer
cloud-only binary (~28 MB smaller); that build says so in Settings instead of
offering a toggle that could not work.

See [`Docs/features/ON_DEVICE_STT.md`](Docs/features/ON_DEVICE_STT.md) for the
download manager, the fallback policy, and the model catalogue.

## Development phases

- **Phase 1 — Foundation:** core loop, Verbatim only. Native hotkey, ring buffer, one STT provider call, clipboard injection. Prove latency + hotkey reliability on both OSes. Spike the `SttProvider` adapter ↔ multipart upload ↔ release-to-inject timing first.
- **Phase 2 — Spellbook:** phonetic post-correction (primary, provider-agnostic) + per-provider biasing (`prompt` / `keyterm`).
- **Phase 3 — Voice layer + BYOK:** OpenAI-compatible adapter, keychain key storage, voice presets, tray selector (Clean default).
- **Phase 4 — Settings/history UI + storage:** SQLite, retention pruning, lifetime stats, egui window.
- **Phase 5 — Ship:** processing indicator, packaging + notarization/signing, first-run permissions, launch-at-login, single-instance guard.

For calls played through speakers, enable **Reduce speaker echo** in Settings →
Meetings and save. It applies from the next meeting and is off by default. Hark
uses captured playback to reduce its echo in the microphone recording and live
transcript; the remote channel and push-to-talk dictation are unchanged. Only
sound present in the captured playback can serve as a reference. Turn it off if
your local voice sounds worse; headphones remain the low-echo option.

The [meeting AEC comparison](tools/meeting-aec-bakeoff/README.md) records the
synthetic evidence behind the Rust AEC3 choice. It does not establish real-speaker
quality or long-call clock stability; the production integration uses estimated
initial track alignment and automatic echo-delay estimation.

## Privacy

### Dictation

- Dictation audio is sent to **your chosen** speech-to-text provider under **your own key** to be transcribed; nothing goes to any Hark-operated server. With the on-device model set as your primary engine, dictation audio stays on your machine. History and stats are stored locally; configured vocabulary may accompany provider requests to improve recognition.
- Any non-Verbatim voice additionally sends the transcript to **your chosen** LLM provider for cleanup, unless the selected transcription mode already returned provider-cleaned text. The UI identifies the selected model and tradeoff.
- The SQLite file is plaintext on disk (normal for a local single-user tool); delete-one, clear-all, disable-capture, and a retention cap are provided. Lifetime stats survive history clears and have a separate reset control.

### Meetings (Windows)

- **What and when:** a meeting records your microphone and playback audio. With the default settings, accepting a detected-call prompt captures that meeting app's process tree; pressing **Start** manually or using the configured meeting shortcut captures all playback except Hark, including unrelated apps. The default detection mode only offers to record: recording starts after Start or an accepted prompt. If you explicitly choose **Start taking notes on its own** in Settings → Meetings (`auto_detect = "auto"`), detected calls start recording automatically. Detection can see a Meet lobby before you join. The system-audio source is configurable; failure to resolve a detected app's process falls back to all playback except Hark. If playback capture itself fails, Hark reports it and continues with the microphone if available.
- **Local storage:** transcripts, speaker names, and notes are stored in `%APPDATA%\hark\hark.db`; recordings are in `%APPDATA%\hark\meetings\<id>\`. Hark writes microphone and playback WAVs during recording and processing, then normally replaces them with a stereo MP3. These files are not encrypted by Hark. The default audio cap is **5 GB in the UI** (`audio_cap_mb = 5120`, 5 GiB). The oldest completed recordings lose their audio first; transcripts and notes stay. Active or processing meetings are protected, so usage can temporarily exceed the cap. A cap of **0** deletes audio after processing, rather than preventing temporary recording files.
- **Provider requests:** live chunks from both channels go to your configured STT provider, or are transcribed locally when on-device Primary is selected. The selected final pass sends both recorded channels to **Deepgram**, or uploads each track in five-minute windows to **Gemini** when explicitly selected. Gemini file deletion is attempted after processing; failed cleanup or a crash can leave files until provider expiry. **Re-run final pass** explicitly uploads the retained recording to Deepgram again after confirmation, even if the automatic final pass is off; it preserves notes but replaces the transcript and resets speaker names on success. If notes are enabled and a text provider is configured, the transcript goes to that **LLM provider** for summarization, even when dictation uses Verbatim. For meeting content to stay entirely on-device, select local Primary, choose **Keep the Me / Them transcript only**, and turn off **Write notes after the call (uses your text provider)** in Settings → Meetings. No Hark-operated server receives the content.
- **Deletion:** deleting a meeting removes its database record, transcript, speaker names, and notes, and attempts to remove its audio folder. A filesystem error can leave audio behind. Settings → Meetings → **Delete all meeting audio** removes eligible recordings while keeping transcripts and notes; active or processing meetings are protected. Deleting local data does not delete exported copies or data already sent to a provider.
- **Other participants:** recording laws may require you to tell other participants or obtain their consent. Hark provides a reminder and an announcement you can copy; it does not notify participants for you.

## License

MIT

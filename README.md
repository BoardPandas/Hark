# Hark

A lean, system-wide, push-to-talk voice dictation tool for **Windows** and **macOS**. Hold a key, speak, release — polished English text is injected at your cursor in any app. Transcription is **bring-your-own-key cloud** by default (you supply your own speech-to-text provider key), with an **optional on-device model** that transcribes without the internet or a key at all; history, stats, the spellbook, and your invocations stay local on your machine; cleanup is optional and uses your own LLM key.

> Wispr Flow-style dictation, scoped to one user, English-only, and local-first.

## Design principles

- **Speed is the product.** All perceived latency lives in the release-to-inject window; everything is structured to keep it small.
- **Local-first where it counts.** History, stats, and the spellbook are local-only and never leave your machine. Transcription goes to the speech-to-text provider *you* choose, under *your* key — or, if you turn on the on-device model, to no provider at all. The optional cleanup pass uses your LLM key. No Hark-operated servers, ever.
- **Lean.** No webview, no browser tab, no JS toolchain. A single Rust process: an always-on tray daemon plus a native window opened on demand.
- **English done well.** Accuracy over language breadth.
- **Data, not code, for anything you tune.** The spellbook and voice presets are config, so editing them never touches the pipeline.

## Tech stack

Desktop app — **no web infrastructure** (no server, database service, auth, or hosting platform).

| Layer | Choice |
|---|---|
| Language | Rust (UI on main thread, pipeline on worker threads) |
| Audio | `cpal` (16 kHz mono ring buffer) |
| Push-to-talk | Native low-level key hooks: CGEventTap (macOS), `WH_KEYBOARD_LL` (Windows) |
| STT | BYOK cloud via an `SttProvider` trait: OpenAI-compatible `/audio/transcriptions` adapter (OpenAI, Groq) + Deepgram nova-3 adapter |
| STT transport | `reqwest` blocking + multipart + rustls on worker threads; one long-lived client, no global tokio |
| Spellbook | Phonetic post-correction (primary, provider-agnostic) + per-provider biasing (OpenAI/Groq `prompt`, Deepgram `keyterm`) |
| Invocations | Trigger phrase → canned text, matched by the same guarded phonetic matcher at a tighter confirm threshold; injected verbatim, cleanup skipped |
| Cleanup / voices | Bring-your-own-key, OpenAI-compatible chat endpoint (optional) |
| Injection | Clipboard paste, `enigo` keystroke fallback |
| Tray + UI | `tray-icon` + `eframe`/`egui` (native, no webview) |
| Storage | `rusqlite` (history + stats), TOML (settings + spellbook) |
| Key storage | `keyring` → macOS Keychain / Windows Credential Manager |

See [`tasks/plan-repo.md`](tasks/plan-repo.md) for the full rationale and the current-as-of-2026-07-15 research corrections.

## Architecture

```
key down ─▶ cpal ring buffer (with ~200–300 ms pre-roll)
              │
key up  ─────▶ append ~150 ms tail
              │
              ▼  trim leading/trailing silence, encode WAV
       one HTTPS POST to your STT provider  ◀── biasing terms (prompt / keyterm)
       (reused keep-alive client, at most one retry on timeout)
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

The tray daemon owns the hot path (hotkey, audio, STT, injection). The settings/history window opens on demand. On macOS the main thread owns the event loop (tray + window); the pipeline runs on worker threads.

## Prerequisites

- **Rust** (stable) via [rustup](https://rustup.rs) — `cargo`, `rustfmt`, `clippy`.
- **A speech-to-text provider key** (OpenAI, Groq, or Deepgram) — entered in Settings on first run, stored in the OS keychain. Not required if you set the on-device model as your primary engine (see below).
- Platform build tools: Xcode command-line tools (macOS); MSVC build tools (Windows).

## Getting started

### Install (Windows)

Download the latest **`Hark-<version>-windows-x64-setup.exe`** from the
[Releases page](https://github.com/BoardPandas/Hark/releases/latest) and run it.
The installer is per user (no admin prompt), installs to
`%LOCALAPPDATA%\Programs\Hark`, and adds a Start Menu shortcut. Unless you turn
it off, Hark starts hidden in the system tray when you sign in to Windows;
toggle that under **Settings → Behavior → "Launch Hark at login"**. A portable
`Hark-<version>-windows-x64.exe` is attached to each release too, if you prefer
not to install.

To remove Hark, use **Add or remove programs**. Your settings and history in
`%APPDATA%\hark` are left in place.

### Build from source

```bash
git clone <this-repo> Hark
cd Hark

cargo build
cargo run

# No model to download — transcription is BYOK cloud. Add your speech-to-text
# provider key (OpenAI, Groq, or Deepgram) in Settings on first run.
```

> **Note:** this machine is a coding-only environment. Build, test, lint, and typecheck here; run and validate the running app (mic, hotkey, injection, notarization) on real macOS and Windows.

## Project structure

Cargo workspace; single binary. See [`tasks/plan-repo.md`](tasks/plan-repo.md) §5.

```
crates/
  hark-app/          # main-thread event loop, worker orchestration, single-instance
                     #   guard, and the egui settings/history/stats window (src/ui/)
  hark-hotkey/       # native push-to-talk key hooks (WH_KEYBOARD_LL / CGEventTap)
  hark-audio/        # cpal ring buffer, pre-roll + tail
  hark-stt/          # SttProvider trait + adapters (OpenAI-compatible, Deepgram)
  hark-spellbook/   # phonetic post-correction, invocation trigger matching,
                     #   and per-provider biasing terms
  hark-voice/        # voice presets + BYOK cleanup adapter
  hark-inject/       # clipboard paste + enigo fallback
  hark-pipeline/     # release-to-inject orchestration across worker threads
  hark-store/        # rusqlite (history + stats)
  hark-config/       # TOML settings + spellbook load/save
  hark-keychain/     # keyring wrapper (BYOK key in the OS keychain)
  hark-autostart/    # launch-at-login (Windows registry / macOS login item)
  hark-update/       # in-app update checker + Windows self-update
config/              # default config.toml + spellbook
```

## Configuration

No web env vars. Settings and secrets live in OS-standard locations:

| Item | Location |
|---|---|
| `config.toml` (hotkey, default voice, BYOK provider/model, spellbook, invocations, capture toggle, retention cap) | OS config dir (`~/Library/Application Support/Hark/`, `%APPDATA%\Hark\`) |
| `hark.db` (history + stats) | OS data dir |
| BYOK API key | OS keychain — never written to `config.toml` |
| On-device model weights (~670 MB, only if you download them) | OS data dir, under `models/` |

## On-device transcription

Optional, off by default, and included in every stock build. Under **Settings →
On-device model** you can download a Parakeet model (~670 MB) and then run it in
one of two modes:

- **Backup** — the cloud provider stays primary; Hark falls back to the local
  model when the network or the provider fails, so you do not lose the sentence
  you just spoke.
- **Primary** — everything is transcribed on this machine. No provider, no API
  key, no audio leaving the computer.

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

## Privacy

- Audio is sent to **your chosen** speech-to-text provider under **your own key** to be transcribed; nothing goes to any Hark-operated server. With the on-device model set as your primary engine, audio never leaves your machine at all. History, stats, and the spellbook stay local and are never transmitted.
- Any non-Verbatim voice additionally sends the transcript to **your chosen** LLM provider for cleanup — surfaced honestly in the UI, with the selected model always visible.
- The SQLite file is plaintext on disk (normal for a local single-user tool); delete-one, clear-all, disable-capture, and a retention cap are provided. Lifetime stats survive history clears and have a separate reset control.

## License

MIT

# Hark

A lean, system-wide, push-to-talk voice dictation tool for **Windows** and **macOS**. Hold a key, speak, release — polished English text is injected at your cursor in any app. Transcription runs entirely on-device; history never leaves your machine; cleanup is optional and uses your own LLM key.

> Wispr Flow-style dictation, scoped to one user, English-only, and local-first.

## Design principles

- **Speed is the product.** All perceived latency lives in the release-to-inject window; everything is structured to keep it small.
- **Local-first.** Transcription is on-device. History is local-only. The sole network dependency is your optional LLM key.
- **Lean.** No webview, no browser tab, no JS toolchain. A single Rust process: an always-on tray daemon plus a native window opened on demand.
- **English done well.** Accuracy over language breadth.
- **Data, not code, for anything you tune.** The dictionary and voice presets are config, so editing them never touches the pipeline.

## Tech stack

Desktop app — **no web infrastructure** (no server, database service, auth, or hosting platform).

| Layer | Choice |
|---|---|
| Language | Rust (UI on main thread, pipeline on worker threads) |
| Audio | `cpal` (16 kHz mono ring buffer) |
| Push-to-talk | Native low-level key hooks: CGEventTap (macOS), `WH_KEYBOARD_LL` (Windows) |
| STT runtime | sherpa-onnx via the official `sherpa-onnx` crate (v1.13.4+) |
| STT model | Parakeet TDT 0.6B v2 (English) ONNX |
| Dictionary | Phonetic post-correction (primary) + sherpa-onnx hotword biasing (experimental) |
| Cleanup / voices | Bring-your-own-key, OpenAI-compatible chat endpoint (optional) |
| Injection | Clipboard paste, `enigo` keystroke fallback |
| Tray + UI | `tray-icon` + `eframe`/`egui` (native, no webview) |
| Storage | `rusqlite` (history + stats), TOML (settings + dictionary) |
| Key storage | `keyring` → macOS Keychain / Windows Credential Manager |

See [`tasks/plan-repo.md`](tasks/plan-repo.md) for the full rationale and the current-as-of-2026-07-15 research corrections.

## Architecture

```
key down ─▶ cpal ring buffer (with ~200–300 ms pre-roll)
              │
key up  ─────▶ append ~150 ms tail
              │
              ▼  trim leading/trailing silence
       Parakeet TDT 0.6B batch decode  ◀── hotword bias list (experimental)
       (model kept warm in memory)
              │
              ▼  phonetic post-correction against dictionary
        voice == Verbatim? ── yes ─▶ inject raw transcript (offline, instant)
              │ no
              ▼
     single BYOK LLM call (low temp): voice template + dictionary terms + transcript
              │
              ▼  inject via clipboard paste (stash → set → paste → restore)
              │
              ▼  write history (if capture enabled) + increment lifetime stats
```

The tray daemon owns the hot path (hotkey, audio, STT, injection). The settings/history window opens on demand. On macOS the main thread owns the event loop (tray + window); the pipeline runs on worker threads.

## Prerequisites

- **Rust** (stable) via [rustup](https://rustup.rs) — `cargo`, `rustfmt`, `clippy`.
- **ONNX Runtime** — bundled by the `sherpa-onnx` crate, or system-provided per its build config (verify CoreML/DirectML feature flags in the crate source before enabling GPU inference).
- **Parakeet TDT 0.6B v2 (English) ONNX model** — placed in `models/` (bundled at package time or downloaded on first run).
- Platform build tools: Xcode command-line tools (macOS); MSVC build tools (Windows).

## Getting started

```bash
git clone <this-repo> Hark
cd Hark

# Fetch the STT model into models/ (script TBD in Phase 1)
# ./scripts/fetch-model.sh

cargo build
cargo run
```

> **Note:** this machine is a coding-only environment. Build, test, lint, and typecheck here; run and validate the running app (mic, hotkey, injection, notarization) on real macOS and Windows.

## Project structure (planned)

Cargo workspace; single binary. See [`tasks/plan-repo.md`](tasks/plan-repo.md) §5.

```
crates/
  hark-app/          # main-thread event loop, worker orchestration, single-instance guard
  hark-hotkey/       # native push-to-talk key hooks
  hark-audio/        # cpal ring buffer, pre-roll + tail
  hark-stt/          # sherpa-onnx wrapper, warm model, batch decode
  hark-dictionary/   # hotword list + phonetic post-correction
  hark-voice/        # voice presets + BYOK adapter
  hark-inject/       # clipboard paste + enigo fallback
  hark-store/        # rusqlite (history + stats), TOML settings
  hark-keychain/     # keyring wrapper
  hark-ui/           # egui settings/history/stats window
models/              # ONNX model (gitignored)
config/              # default config.toml + dictionary
```

## Configuration

No web env vars. Settings and secrets live in OS-standard locations:

| Item | Location |
|---|---|
| `config.toml` (hotkey, default voice, BYOK provider/model, dictionary, capture toggle, retention cap) | OS config dir (`~/Library/Application Support/Hark/`, `%APPDATA%\Hark\`) |
| `hark.db` (history + stats) | OS data dir |
| BYOK API key | OS keychain — never written to `config.toml` |

## Development phases

- **Phase 1 — Foundation:** core loop, Verbatim only. Native hotkey, ring buffer, warm Parakeet decode, clipboard injection. Prove latency + hotkey reliability on both OSes. Spike the sherpa-onnx crate ↔ Parakeet ↔ hotwords alignment first.
- **Phase 2 — Dictionary:** phonetic post-correction (primary) + experimental hotword biasing (gated on sherpa-onnx issue #3267).
- **Phase 3 — Voice layer + BYOK:** OpenAI-compatible adapter, keychain key storage, voice presets, tray selector (Clean default).
- **Phase 4 — Settings/history UI + storage:** SQLite, retention pruning, lifetime stats, egui window.
- **Phase 5 — Ship:** processing indicator, packaging + notarization/signing, first-run permissions, launch-at-login, single-instance guard.

## Privacy

- Transcription and history are **local-only** and never transmitted.
- Any non-Verbatim voice sends your dictated text to **your chosen** LLM provider — surfaced honestly in the UI, with the selected model always visible.
- The SQLite file is plaintext on disk (normal for a local single-user tool); delete-one, clear-all, disable-capture, and a retention cap are provided. Lifetime stats survive history clears and have a separate reset control.

## License

MIT

---
description: Rust + native-desktop conventions and verified gotchas for Hark
paths:
  - "crates/**"
  - "**/*.rs"
  - "build.rs"
---

# Rust / Desktop Rules (Hark)

Loads whenever Rust source is touched. Full context: root `CLAUDE.md`, [`tasks/plan-repo.md`](../../tasks/plan-repo.md), and `.claude/agent-memory/explorer/hark_cloud_stt_providers.md` + `hark_cloud_stt_rust_stack.md`.

## Threading (non-negotiable)

- **All UI on the main thread** (macOS requirement): main thread owns the tray + egui event loop; the dictation pipeline runs on worker threads only. Communicate via channels; never block the UI thread on pipeline work or vice versa.
- **Latency is the product.** One long-lived `reqwest::blocking::Client` per process (keep-alive + TLS resumption); never build a client per press. One POST per utterance; at most one retry, on timeout only. History/stats DB writes happen after injection, off the hot path.

## Verified stack gotchas (as of 2026-07-15, re-verify before relying on them)

- **STT is BYOK cloud, not local.** `SttProvider` trait, two Phase 1 adapters: OpenAI-compatible multipart `/audio/transcriptions` (OpenAI + Groq share the exact contract) and Deepgram `/v1/listen` (nova-3, `Token` auth, raw `audio/wav` body, `keyterm` biasing). No sherpa-onnx, no ONNX runtime, no model files.
- **Transport:** `reqwest` 0.13 `blocking` + `multipart` + `rustls-tls-webpki-roots` on the pipeline worker thread; no global tokio runtime. `ureq` multipart is unstable (as of 3.3.0); the `deepgram` crate is pre-1.0 and drags in full tokio.
- **Provider quirks:** Groq bills a 10 s minimum per request. Deepgram `keyterm` needs nova-3+ (weighted `keywords` is nova-2 legacy; mutually exclusive). OpenAI/Groq biasing goes through `prompt` (224-token cap on Whisper-family models).
- **Never log API keys or raw audio**; error messages must not echo the Authorization header or request bodies.
- **Push-to-talk:** use native low-level hooks (**CGEventTap** on macOS, **`WH_KEYBOARD_LL`** on Windows) for reliable key-down/up edges on a held key. The `global-hotkey` crate is not reliable for held-key press/release semantics.

## LL-G Rust gotchas (HIGH severity)

- **Console-window flashing** (`kb/rust/gui-subsystem-console-child-window.md`): a `windows_subsystem="windows"` tray binary has no console, so every console child process (signing tools, `taskkill`, launch-at-login setup) pops a visible console window that steals focus. Set `CREATE_NO_WINDOW` (0x0800_0000) via `CommandExt::creation_flags` on Windows.
- **Blocking IO on an async executor** (`kb/rust/blocking-io-on-tokio.md`): moot while all HTTP is `reqwest::blocking` on worker threads (no executor exists to starve). Becomes live the moment a streaming adapter introduces tokio; keep any runtime scoped to that adapter and blocking IO off it.

## General

- `Result` + `?` everywhere; never swallow errors. Validate at boundaries: mic samples, STT provider responses, cleanup responses, TOML/DB I/O.
- Secrets (provider API keys) live only in the OS keychain via `keyring`; never in `config.toml`, never logged.
- `cargo fmt` + `cargo clippy --all-targets -- -D warnings` clean before done. Files > 500 lines get split.

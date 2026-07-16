---
description: Rust + native-desktop conventions and verified gotchas for Hark
paths:
  - "crates/**"
  - "**/*.rs"
  - "build.rs"
---

# Rust / Desktop Rules (Hark)

Loads whenever Rust source is touched. Full context: root `CLAUDE.md`, [`tasks/plan-repo.md`](../../tasks/plan-repo.md), and `.claude/agent-memory/explorer/hark_stt_stack_risk.md`.

## Threading (non-negotiable)

- **All UI on the main thread** (macOS requirement): main thread owns the tray + egui event loop; the dictation pipeline runs on worker threads only. Communicate via channels; never block the UI thread on pipeline work or vice versa.
- **Latency is the product.** Model loaded once + warmup inference at launch; never load per-press. Verbatim and skip-eligible short utterances never touch the network. History/stats DB writes happen after injection, off the hot path.

## Verified stack gotchas (as of 2026-07-15 — re-verify before relying on them)

- **STT binding:** use the official **`sherpa-onnx` crate (v1.13.4+)**, NOT the deprecated/archived `sherpa-rs`. Model is **Parakeet TDT 0.6B v2 (English)** ONNX (`model-type=nemo_transducer`, encoder/decoder/joiner + tokens.txt).
- **Hotwords are risky:** contextual biasing needs `modified_beam_search`, which has an **open ~20% hallucination/empty-output bug on Parakeet TDT (sherpa-onnx #3267)**. Default to `greedy_search` + the phonetic post-correction pass; gate decode-time biasing behind an experimental flag. There is **no working Rust hotwords example** upstream — treat it as a spike, not a given.
- **GPU features:** confirm CoreML (macOS) / DirectML (Windows) cargo feature-flag names by reading the `sherpa-onnx` crate's `Cargo.toml`/build script directly — the docs don't spell them out.
- **Push-to-talk:** use native low-level hooks (**CGEventTap** on macOS, **`WH_KEYBOARD_LL`** on Windows) for reliable key-down/up edges on a held key. The `global-hotkey` crate is not reliable for held-key press/release semantics.

## LL-G Rust gotchas (HIGH severity)

- **Console-window flashing** (`kb/rust/gui-subsystem-console-child-window.md`): a `windows_subsystem="windows"` tray binary has no console, so every console child process (signing tools, `taskkill`, launch-at-login setup) pops a visible console window that steals focus. Set `CREATE_NO_WINDOW` (0x0800_0000) via `CommandExt::creation_flags` on Windows.
- **Blocking IO on an async executor** (`kb/rust/blocking-io-on-tokio.md`): `std::fs` and other sync/blocking calls on a Tokio runtime starve the executor. For the BYOK HTTP call, either use `tokio::fs`/`spawn_blocking`, or run a blocking HTTP client on a dedicated worker thread.

## General

- `Result` + `?` everywhere; never swallow errors. Validate at boundaries: mic samples, model output, BYOK responses, TOML/DB I/O.
- Secrets (BYOK key) live only in the OS keychain via `keyring` — never in `config.toml`, never logged.
- `cargo fmt` + `cargo clippy --all-targets -- -D warnings` clean before done. Files > 500 lines get split.

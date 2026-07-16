---
name: hark-stt-stack-risk
description: Research findings (2026-07-15) on whether sherpa-onnx Rust bindings + Parakeet TDT 0.6B ONNX + hotwords/contextual-biasing align for the Hark dictation app
metadata:
  type: project
---

Researched 2026-07-15 whether the sherpa-onnx Rust bindings, Parakeet TDT 0.6B English ONNX export, and hotword/contextual-biasing support all line up today for Hark (offline push-to-talk dictation app).

**Finding: PARTIAL alignment, one open reliability bug is the critical risk.**

- `sherpa-rs` (thewh1teagle) is deprecated/archived (as of 2026-06-06) in favor of the **official Rust API now shipped inside k2-fsa/sherpa-onnx itself** (crate `sherpa-onnx` on crates.io, v1.13.4 as of 2026-07-08). Plans should target the official crate, not sherpa-rs.
- Parakeet TDT 0.6B (v2 English, v3 multilingual) ONNX exports are officially distributed by k2-fsa/sherpa-onnx and exposed via `OfflineRecognizer` (encoder/decoder/joiner + tokens.txt).
- Hotwords/contextual biasing for NeMo TDT (Parakeet) transducers only landed via PR #3077, merged 2026-02-05 (new `OfflineTransducerModifiedBeamSearchNeMoDecoder`). Before that, TDT models only supported `greedy_search` (no hotwords path).
- **Open bug (sherpa-onnx issue #3267, filed 2026-03-07, still open as of 2026-07-15):** `modified_beam_search` with Parakeet TDT hallucinates or returns empty text ~20% of the time; `greedy_search` is reliable but does not support hotwords. This is the actual blocker for Hark's dictionary decode-biasing plan — not availability, but reliability of the required decoding mode.
- The official Rust API's `rust-api-examples/` directory (as of research date) has NeMo/Parakeet TDT streaming-mic examples but no dedicated hotwords example — untested/unconfirmed for Rust specifically even though the underlying C++ core supports it.
- Comparable prior-art project (Open-Less/openless, Tauri+Rust hold-key dictation for macOS/Windows) deliberately avoided sherpa-onnx/Parakeet entirely, using Qwen3-ASR / cloud ASR / Apple Speech instead — a signal that this exact stack combo isn't yet proven in a shipped app.
- For push-to-talk key edges: recommend platform-native hooks (CGEventTap on macOS, WH_KEYBOARD_LL on Windows) over the `global-hotkey` (tauri-apps) crate, which is undocumented for reliable down/up semantics outside X11 and had known release-ordering bugs on Linux. Open-Less made the same choice.

**Why:** This determines whether Hark's Phase 1 plan can commit to real-time hotword biasing on Parakeet TDT, or must plan a fallback (e.g. ship with greedy_search + a post-hoc dictionary correction pass instead of decode-time biasing) until sherpa-onnx issue #3267 is resolved.

**How to apply:** Before writing the Phase 1 plan or starting pipeline code, re-check the status of https://github.com/k2-fsa/sherpa-onnx/issues/3267 — if still open, the plan should default to `greedy_search` and treat `modified_beam_search` hotword biasing as an experimental/opt-in path, matching the workaround the issue reporter already adopted. Revisit this memory once #3267 is closed or a new sherpa-onnx release changelog claims a fix.

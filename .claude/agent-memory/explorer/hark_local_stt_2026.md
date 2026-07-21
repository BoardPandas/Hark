---
name: hark-local-stt-2026
description: Model + Rust inference stack research (2026-07-21) for adding an optional on-device/local STT fallback-or-primary engine to Hark, on top of the existing BYOK cloud pivot
metadata:
  type: project
---

Researched 2026-07-21 for the local-STT feature (settings toggle, "Download model" button with progress, local as fallback-or-primary alongside existing BYOK cloud STT). Builds on and does NOT replace [[hark-stt-stack-risk]] / [[sherpa_onnx_rust_api]] (2026-07-15 sherpa-onnx + Parakeet TDT research from before the BYOK pivot) — those findings are still current; this memory adds the model-selection comparison, a second Rust crate option (`transcribe-rs`), download-URL/size verification, and a latency reality check against the cloud numbers in [[patterns]].

## Recommendation (condensed — see below for full comparison)

**PRIMARY: `sherpa-onnx` crate v1.13.4 + NVIDIA Parakeet TDT 0.6B v2 (English), int8 ONNX, `greedy_search` decoding.**
Reuses the working Rust API sketch already captured in [[sherpa_onnx_rust_api]]. Best CPU latency of any candidate (RTF ~0.3 on a 12th-gen-i7-class CPU → a 5 s utterance transcribes in roughly 1.5–2 s with no GPU), WER competitive with Whisper large-v3 (1.69% LibriSpeech test-clean vs Whisper large-v3's ~7.4% on a broader multi-dataset average — not apples-to-apples, but Parakeet wins on every English benchmark checked), CC-BY-4.0 license (redistribution-friendly), ~630 MB on disk (three int8 ONNX files), no CMake/C++ toolchain needed to *build* (the crate's `static` feature auto-downloads a prebuilt native lib archive at `cargo build` time). English-only, which matches Hark's stated language priority.

**Important simplification vs the 2026-07-15 research:** the open sherpa-onnx reliability bug (issue #3267, `modified_beam_search` hallucinating ~20% of the time) blocked *decode-time hotwords*. That blocker is now moot for the local-STT feature — Hark's dictionary correction already runs as a provider-agnostic **phonetic post-correction pass** (`rphonetic` + `strsim`, per the cloud-STT dictionary design) that applies equally to local-model output. So the local engine only ever needs to run `greedy_search`, which is the reliable path; there's no reason to touch `modified_beam_search`/hotwords for local STT at all. This removes the single biggest open risk from the prior sherpa-onnx research.

**RUNNER-UP: `transcribe-rs` crate (cjpais) v0.3.11, `onnx` feature, same Parakeet TDT weights.**
This is the extracted, reusable Rust STT library behind **Handy** (github.com/cjpais/handy), a shipped open-source Tauri+Rust push-to-talk dictation app that already does exactly what Hark is building (local model download, model picker in settings, Parakeet as the CPU-optimized default, Whisper via whisper.cpp as an optional GPU-accelerated alternative). Its `onnx` feature wraps `ort` (ONNX Runtime bindings) with `download-binaries`, so — like sherpa-onnx — no CMake is needed for the Parakeet path; a separate `whisper-cpp` feature (needed only if Whisper is ever added) does pull in a full C++ toolchain requirement via whisper.cpp. Prefer sherpa-onnx as primary because: (a) Hark already has a verified, working Rust code sketch and confirmed struct/field names for it (see [[sherpa_onnx_rust_api]]) — zero re-derivation cost; (b) it's the more mature, widely-deployed project (k2-fsa, official multi-language bindings) vs. transcribe-rs, which is a newer (single-maintainer) extraction from one app. Keep transcribe-rs in mind if Hark later wants Whisper/Moonshine/multi-engine flexibility behind one trait — its `SpeechModel` abstraction is a cleaner multi-engine story than hand-rolling per-engine adapters.

**Explicitly NOT recommended as primary:** Whisper (any size) as the default local engine. Even `large-v3-turbo`/`medium.en` on CPU-only laptops land at roughly real-time-to-2x-slower-than-realtime (see latency section), which for a 5 s utterance is a 5–15 s wait — directly against Hark's "release-to-inject latency is the product" rule. Whisper remains viable as an *optional* alternate local model for users who want maximum language coverage (99 languages) or already have a discrete GPU (Metal/CUDA cuts this dramatically — M2/M3 Metal does a 10 s clip in 0.4–0.8 s for whisper-small), but should not be the on-by-default local model.

---

## 1. Model comparison (as of 2026-07-21)

| Model | WER (English, LibriSpeech-class) | CPU RTF (no GPU) | Disk size (recommended quant) | License | Languages |
|---|---|---|---|---|---|
| **Parakeet TDT 0.6B v2** (English) | test-clean 1.69%, test-other 3.19% | ~0.3 (≈3x faster than realtime on i7-12700K-class CPU, int8 ONNX) | ~630 MB (int8: encoder 622M + decoder 6.9M + joiner 1.7M) | **CC-BY-4.0** | English only |
| **Parakeet Unified EN 0.6B** (newer, Apr 2026) | test-clean 1.63%, test-other 3.11% (best-in-class of the models checked) | Not yet published for CPU-only ONNX; NeMo-framework native, **no confirmed sherpa-onnx/ONNX export as of this research** | 600M params, not yet packaged as quantized ONNX | NVIDIA Open Model License (permissive, but framework-only) | English only |
| **Parakeet TDT v3** (multilingual) | 6.34% (25-language avg) | Similar RTF profile to v2, ~933 MB for one repackaged int8 export seen (some third-party re-exports are larger than the original v2 due to non-quantized joint decoder) | ~630–933 MB depending on export | CC-BY-4.0 | 25 languages |
| **Whisper large-v3** | ~7.4% (99-language multi-dataset avg; not English-only comparable) | Slow on CPU — full non-turbo large model, minutes for longer clips | ~3 GB fp16 / ~1.5 GB q5 GGUF | MIT | 99 languages |
| **Whisper large-v3-turbo** | Close to large-v3, ~6x faster (distilled decoder) | Still CPU-heavy without GPU; "under 20s" reported for unspecified-length clips via faster-whisper CPU | ~1.6 GB fp16 GGML / ~526 MB–820 MB quantized GGUF (varies by quant: Q4_1 ~526 MB) | MIT | 99 languages |
| **Whisper medium.en** | Worse than Parakeet/large but reasonable | ~1.0x realtime on a fast desktop CPU (borderline for a 5s dictation) | ~1.5 GB fp16 | MIT | English only |
| **Whisper small(.en)** | Noticeably worse WER | ~1.5–2x realtime on a modern laptop CPU (10s clip → 5-7s) | <1 GB | MIT | English only / multilingual |
| **Moonshine** (Useful Sensors) | Comparable to Whisper tiny.en on short utterances, "no increase in WER" vs tiny.en at 5x lower compute | Fast — built for edge devices (Raspberry Pi class); tiny model is 27 MB | 27 MB (tiny) up to small sizes | Permissive (MIT-class, Useful Sensors OSS release) | Primarily English; streaming-capable v2 |
| **Distil-Whisper** | Slightly worse than full Whisper, much faster (encoder-shared distillation) | Faster than base Whisper, still autoregressive decoder-bound on CPU | Similar order to base sizes | MIT (same licensing lineage as Whisper) | English-focused variants most common |
| **NVIDIA Canary** | Competitive with Parakeet on translation-heavy tasks | Not primarily optimized for pure ASR CPU latency; heavier multi-task model | Larger than Parakeet TDT (1B-class) | NVIDIA Open Model License | Multilingual + translation |
| **Kyutai (Moshi/STT)** | No fresh 2026 CPU-latency or WER benchmark surfaced in this research pass | — | — | — | — |

**Bottom line on models:** Parakeet TDT 0.6B v2 (English) is the clear pick for a CPU-only fallback/primary given Hark's constraints — best WER of anything with a working CPU-friendly ONNX export, smallest practical download for its accuracy tier, and a genuinely permissive redistribution license (CC-BY-4.0 just requires attribution, which is easy to satisfy in an in-app credits/about screen). Parakeet Unified EN (Apr 2026) is nominally slightly more accurate but has **no confirmed quantized ONNX/sherpa-onnx export yet** — worth re-checking in a future research pass, not ready today. Moonshine is worth keeping on the radar as a *tiny* secondary option (27 MB) if Hark ever wants an even-lower-footprint always-resident model, but its accuracy/language coverage is a step below Parakeet for general dictation.

## 2. Rust inference stack comparison

| Crate | Version / date | Wraps | Build complexity | GPU accel | Notes |
|---|---|---|---|---|---|
| **`sherpa-onnx`** (k2-fsa official) | 1.13.4, confirmed current as of 2026-07-15 research, re-confirmed still the crates.io name as of 2026-07-21 | Bundled onnxruntime + k2-fsa's own C++ core, via FFI | `static` feature (default) auto-downloads a prebuilt native lib archive at build time — **no CMake needed to consume the crate**. No confirmed CUDA/CoreML Cargo *feature* strings; GPU selection is via a runtime `provider` config string ("cuda"/"coreml"), unverified for Rust bindings specifically. | CPU confirmed; CUDA/CoreML plausible via `provider` string, unverified | Already has a working Rust code sketch for Hark in [[sherpa_onnx_rust_api]]. Most mature/widely-used option of the three. |
| **`transcribe-rs`** (cjpais) | 0.3.11, released 2026-04-07; ~28k downloads/month | `onnx` feature → `ort` (pykeio/ort) for Parakeet/Canary/Moonshine/SenseVoice/GigaAM; `whisper-cpp` feature → whisper.cpp (GGML/GGUF) for Whisper/Whisperfile; also has an `openai` remote-API feature | `onnx` feature: no CMake (ort's `download-binaries` fetches prebuilt onnxruntime). `whisper-cpp` feature: full C++ toolchain required (same class of build pain as `whisper-rs`). | Metal (macOS), CUDA (NVIDIA, needs Toolkit 12.0+), DirectML (Windows), ROCm (AMD) via `set_ort_accelerator()` for ONNX engines; whisper.cpp side has its own Metal/CUDA/Vulkan flags | This is the exact crate powering **Handy**'s local STT — real shipped-app precedent for Hark's use case (see §4). High-level `SpeechModel` trait abstracts engines uniformly. Younger/single-maintainer project vs sherpa-onnx. |
| **`whisper-rs`** | 0.16.0, released 2026-03-12 | whisper.cpp bindings | Requires a C++ build toolchain always; Linux "just works," Windows and Apple Silicon Macs need extra steps per BUILDING.md. This is the "heavy C++/CMake" case the Hark constraints ask to avoid if possible. | CUDA, Metal, Vulkan, CoreML, ROCm/hipBLAS, OpenBLAS, Intel SYCL — all via Cargo feature flags | License: Unlicense (public domain). Only relevant if Whisper becomes a supported local model; skip for the Parakeet-only MVP. |
| **`candle` / `candle-transformers::models::whisper`** | Actively maintained by Hugging Face; whisper implementation present in the transformers crate | Pure-Rust ML framework, no C++ FFI at all | No CMake, pure Rust — best build story of any option, but only implements Whisper (no Parakeet TDT support found) | Metal, CUDA via candle's own backends | Not evaluated further because it only covers Whisper, which this research recommends against as the *primary* local model; worth a look only if Hark later wants a build-simplicity-first Whisper option with zero C/C++ deps. |
| **`ort` (pykeio/ort)** | Underlies transcribe-rs's `onnx` feature and is usable standalone | Direct ONNX Runtime bindings | `download-binaries` feature avoids CMake entirely (default "Download" build strategy); a "Compile" strategy exists but requires the full CMake+C++ build env — only opt into that if a custom onnxruntime build is needed | CUDA, ROCm, DirectML, CoreML, WebGPU | Could be used directly instead of transcribe-rs for a hand-rolled Parakeet ONNX adapter, trading transcribe-rs's convenience for more control — not needed given sherpa-onnx already covers this. |

**Build-complexity bottom line:** both the primary (sherpa-onnx) and runner-up (transcribe-rs `onnx` feature) avoid a CMake/C++ toolchain requirement for the Parakeet path specifically. Any future decision to add Whisper as an *additional* local option should be scoped as an explicit, separate build-complexity tradeoff (whisper-rs or transcribe-rs's `whisper-cpp` feature), not bundled into the MVP.

## 3. Model distribution — concrete URLs

Parakeet TDT 0.6B v2 (English), int8 ONNX, from the k2-fsa official release (same as captured in [[sherpa_onnx_rust_api]], re-verified 2026-07-21):
```
https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8.tar.bz2
```
Extracted contents: `encoder.int8.onnx` (~622 MB), `decoder.int8.onnx` (~6.9 MB), `joiner.int8.onnx` (~1.7 MB), `tokens.txt`, `test_wavs/`. Total ~630 MB int8 (~1.3 GB extracted incl. test wavs — the wavs are not needed for shipping, only the 3 onnx files + tokens.txt, so the actual "Download model" payload for Hark should be ~630 MB, not the full extracted tarball).

Mirror on Hugging Face (license CC-BY-4.0 confirmed on the model card):
```
https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8
```

Parakeet TDT v3 (multilingual, if Hark ever wants non-English), third-party int8 ONNX re-export:
```
https://huggingface.co/nasedkinpv/parakeet-tdt-0.6b-v3-onnx-int8
```
Files: `encoder-int8.onnx` (1.44 MB) + `encoder-int8.onnx.data` (878 MB, the actual weight blob — ONNX external-data format) + `decoder_joint-int8.onnx` (53.6 MB) + `vocab.txt` (93.9 kB). Total ~933 MB — larger than v2 because the joint decoder here isn't as aggressively quantized; the official k2-fsa v2 packaging (above) is the tighter option and is English-only anyway, which matches Hark's priority.

Whisper large-v3-turbo GGUF/GGML quantized (only relevant if Whisper is added later):
```
https://huggingface.co/ggerganov/whisper.cpp/tree/main   (canonical GGML source, referenced directly by transcribe-rs's README)
```
Quantized turbo variants seen: Q4_1 ~526 MB, another quantized build ~822.75 MB — exact filenames vary by uploader; ggerganov/whisper.cpp is the canonical source whisper-rs and transcribe-rs both point at.

**HF download mechanics for a `reqwest`-blocking implementation:** direct `resolve/main/<file>` URLs on Hugging Face are plain HTTPS GETs served off LFS/CDN infrastructure and support standard `Range:` request headers and return `Content-Length` — sufficient for both a progress bar (`Content-Length` on the initial response) and resumable partial downloads (`Range: bytes=<n>-` on retry). Note: HF's *own* client tooling (`huggingface_hub`/`hf` CLI) has moved to a Xet-backed chunked-transfer backend for its own smart-resume logic, but that's an HF-client-side optimization — a plain `reqwest` GET against the `resolve/main/...` URL still works as a normal byte-range-capable HTTP download and does not require adopting the Xet protocol. No auth/token required for any of the public model repos above (all public, non-gated).

## 4. Real-world precedent — Handy

**Handy** (github.com/cjpais/handy) is the closest existing shipped app to what Hark is building: Tauri (Rust backend + web UI) push-to-talk dictation, fully local, cross-platform (Windows/macOS/Linux). Directly relevant findings:
- Uses **Parakeet V3 as the CPU-optimized default** and Whisper (Small/Medium/Turbo/Large via GGML/GGUF) as GPU-accelerated alternatives — validates "Parakeet as default, Whisper as opt-in" as a proven product decision, not just a benchmark-driven guess.
- Model storage convention: per-OS app-data directory + `models/` subfolder (macOS: `~/Library/Application Support/<bundle-id>/models/`; Windows: `%AppData%\<bundle-id>\models\`) — a reasonable pattern for Hark's own model storage location, adjusted to Hark's own bundle identifier and existing config directory conventions.
- Reports "~5x real-time speed on mid-range hardware (tested on i5)" for Parakeet V3 — consistent with the RTF ~0.3 figure found independently above (0.3 RTF ≈ 3.3x realtime; Handy's informal i5 number of "5x" is in the same ballpark, plausibly a faster/more-recent i5 or a shorter/cleaner test clip).
- No detailed public writeup of the download-progress UI implementation was found in the documentation surfaced by this research pass — worth a deeper follow-up read of Handy's actual source (not just docs) if Hark wants to mirror its exact progress-bar wiring; flagged as unconfirmed rather than assumed.

Other adjacent apps surfaced (Whisperi, open-whisper, VoiceInk-class tools) largely wrap whisper.cpp/faster-whisper rather than Parakeet, reinforcing that Parakeet+ONNX-via-Rust is the newer, less-trodden but demonstrably-shipping path (Handy is the one clear precedent), while Whisper-based local dictation is the well-trodden but slower-on-CPU path.

## 5. Latency reality check (5 s utterance, CPU-only laptop, no discrete GPU)

| Option | Estimated release-to-transcript latency | Source basis |
|---|---|---|
| **Parakeet TDT 0.6B v2, int8 ONNX (sherpa-onnx or transcribe-rs)** | **~1.5–2 s** | RTF ~0.3 measured on i7-12700K-class CPU (sherpa-onnx docs benchmark: 3.845 s clip → 1.249 s elapsed, RTF 0.325); Handy's informal "~5x realtime on i5" is consistent or better |
| Whisper small(.en), whisper.cpp CPU | ~2.5–3.5 s | "modern laptop CPU ~1.5–2x realtime" (10 s clip → 5–7 s) scaled to 5 s |
| Whisper medium.en, whisper.cpp CPU | ~5 s (borderline realtime) | "medium runs ~1.0x realtime on a fast desktop CPU" |
| Whisper large-v3-turbo, CPU only (no GPU) | Likely 8–15 s+ for a 5 s clip (uncertain — sources reported total times for unspecified/longer clips, not a clean per-5s figure) | faster-whisper CPU report of "under 20 s" for an unstated-length clip; treat as a rough upper bound, **flag for direct measurement during implementation**, don't trust as precise |
| Whisper large-v3-turbo, Apple Silicon Metal | ~0.4–0.8 s (comparable to or better than cloud) | M2/M3 Metal whisper.cpp reported 10 s clip in 0.4–0.8 s for whisper-small; turbo is in the same tier with Metal acceleration |

**Implication for Hark:** only Parakeet TDT (or Whisper *with* GPU acceleration, which can't be assumed present) gets close to the cloud provider latencies already measured in [[patterns]] (Deepgram p50 150 ms / p95 630 ms; OpenAI/Groq ~800-1500 ms). Even Parakeet's best case (~1.5-2 s) is roughly 2-10x slower than the cloud p50s — expected and acceptable for a **fallback** engine (only invoked when cloud has already failed/timed out, so there's no faster alternative to compare against in that moment), but this makes Parakeet-as-**primary** a real UX tradeoff Hark's product decision needs to weigh explicitly, not a free upgrade. Any CPU-only Whisper variant above medium.en is likely too slow to be usable even as a fallback for a product whose entire value proposition is low release-to-inject latency.

## Sources
- https://openwhispr.com/blog/parakeet-vs-whisper-vs-nemotron (2026-07-18)
- https://lib.rs/crates/transcribe-rs
- https://github.com/cjpais/transcribe-rs
- https://raw.githubusercontent.com/cjpais/transcribe-rs/main/README.md
- https://github.com/cjpais/handy
- https://lib.rs/crates/whisper-rs
- https://huggingface.co/nvidia/parakeet-tdt-0.6b-v2
- https://huggingface.co/nvidia/parakeet-unified-en-0.6b
- https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8
- https://huggingface.co/nasedkinpv/parakeet-tdt-0.6b-v3-onnx-int8/tree/main
- https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx
- https://huggingface.co/docs/hub/en/models-downloading
- https://github.com/pykeio/ort ; https://ort.pyke.io/setup/cargo-features
- k2-fsa/sherpa-onnx release page: https://github.com/k2-fsa/sherpa-onnx/releases
- Whisper/Moonshine CPU benchmark roundups: https://www.promptquorum.com/power-local-llm/local-whisper-stt-comparison-2026 ; https://whispernotes.app/blog/introducing-whisper-large-v3-turbo ; arXiv Moonshine papers (2410.15608, 2509.02523)

**Why:** Hark's product plan wants an on-device fallback/primary STT option; the 2026-07-15 research answered "does sherpa-onnx+Parakeet work at all" but pre-dated the BYOK cloud pivot and didn't compare against other model/crate families or check today's (2026-07-21) versions, nor did it do a latency reality-check against the now-measured cloud baseline in [[patterns]].

**How to apply:** Use the Parakeet TDT 0.6B v2 + sherpa-onnx combo as the default local-STT implementation plan; reuse the working code sketch in [[sherpa_onnx_rust_api]] directly. Treat `greedy_search`-only (no hotwords) as final, not a workaround, since the phonetic post-correction pass now carries dictionary biasing for both cloud and local paths. Re-verify Parakeet Unified EN's ONNX export availability in a future pass before considering it as an upgrade path. Measure real large-v3-turbo CPU-only latency directly rather than trusting the rough upper-bound estimate above if Whisper is ever added as a secondary local option.

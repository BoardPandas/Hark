---
name: hark-cloud-stt-rust-stack
description: Verified 2026-07-15 Rust dependency research for Hark's pivot to BYOK cloud STT (HTTP/WebSocket client, audio encoding, local fallback model)
metadata:
  type: project
---

# Hark cloud STT Rust stack — verified 2026-07-15

Researched because Hark is pivoting from on-device sherpa-onnx + Parakeet TDT (rejected: ~1.1 GB footprint, see [[hark-stt-stack-risk]] and [[sherpa-onnx-rust-api]]) to BYOK cloud STT: multipart audio upload -> text, on the hot path key-release -> POST -> parse JSON -> inject.

## 1. HTTP client: reqwest vs ureq

- **reqwest** 0.13.4 (crates.io, last publish 2026-05-25). Async-first (hyper/tokio under the hood) but ships a `blocking` feature that runs its own internal single-thread tokio runtime per client — usable from a plain worker thread without the caller touching async at all. Multipart via `reqwest::blocking::multipart` (`Form::file()` helper for streaming a file part). TLS: `rustls-tls` (plus `-native-roots`/`-webpki-roots` variants) or `native-tls` feature flags — rustls avoids a system OpenSSL dependency, better for a single self-contained Windows/macOS binary. Source: https://docs.rs/reqwest/latest/reqwest/blocking/multipart/index.html
- **ureq** 3.3.0 (crates.io, last publish 2026-03-21). Sync-only by design, no tokio anywhere in the dependency tree — smaller compile graph and binary. As of 3.2.0 it ships **built-in multipart support** behind an explicit `multipart` Cargo feature (migrated off the old unmaintained third-party `multipart` crate); the maintainers flag this feature as **unstable, API may change in a minor version**. Older `ureq_multipart` third-party crate still exists but is now redundant. TLS: rustls by default in 3.x. Source: https://crates.io/crates/ureq (via `/api/v1/crates/ureq`), https://github.com/pyo3/maturin/pull/2997 (real-world migration to built-in multipart), https://docs.rs/ureq/latest/ureq/
- **Recommendation:** for Hark's actual hot path (one POST of 100-500 KB per utterance, occasional retry, generous timeout, no concurrent request fan-out), **reqwest with the `blocking` + `multipart` + `rustls-tls-webpki-roots` features, called from a dedicated worker thread**, is the pragmatic choice over ureq. Reasoning: reqwest's multipart API is stable and battle-tested (used by the official Deepgram SDK itself, see below), whereas ureq's multipart support is explicitly marked unstable as of today. The "don't block a tokio executor" gotcha in the project's stack notes doesn't apply here because there is no shared tokio runtime to block — `reqwest::blocking` owns its own isolated runtime instance, and the call happens on a worker thread with nothing else scheduled on it. Only reach for ureq if final binary size becomes a measured problem later; it is the leaner option (no tokio) if that tradeoff ever matters more than reusing the same HTTP stack as a WebSocket-streaming adapter (see below, which needs tokio anyway for `tokio-tungstenite`).

## 2. WebSocket streaming (Deepgram-style live upload)

- **tokio-tungstenite** 3.30.0 [reported version string was 0.30.0 per crates.io API — the crate has not crossed 1.0] , last publish 2026-07-11 (actively maintained, 219M+ total downloads). Standard choice; requires tokio.
- **Official Deepgram Rust SDK**: crate name **`deepgram`** on crates.io, version **0.10.0**, last published 2026-05-12, repo https://github.com/deepgram/deepgram-rust-sdk. Its own `Cargo.toml` (fetched today) pins: `tokio = "1.45.1"` (features `["full"]`), `reqwest = "0.13"` (features `["json","rustls","stream","query"]`), `tokio-tungstenite = "0.28"` (feature `rustls-tls-webpki-roots`, only pulled in when the crate's own `listen` feature is enabled) plus `tungstenite`, `tokio-stream`, `tokio-util`. Crate description still self-identifies as "Community Rust SDK" — actively published in 2026 but not yet a Deepgram-owned 1.0 GA.
- **Implication for Hark:** if a Deepgram streaming adapter is added, it drags in a full tokio runtime regardless of what was chosen for the plain-upload HTTP path in §1 — so if streaming is on the roadmap at all, standardizing on **reqwest (async, not just `blocking`) + tokio + tokio-tungstenite** from the start avoids running two different HTTP/async stacks side by side. If streaming is a later, optional/rare adapter, it's reasonable to keep the default provider on `reqwest::blocking` (simple, no runtime to manage) and isolate the tokio runtime construction entirely inside the Deepgram adapter module, spun up only when that adapter is selected.

## 3. Audio encoding for upload (16 kHz mono, from cpal f32 ring buffer)

Size math for a 10 s clip (16 kHz mono):
- Raw f32 PCM: 16,000 * 4 bytes * 10 s = 640,000 bytes (~625 KB) — not sent as-is.
- **WAV (16-bit PCM) via `hound`**: 16,000 * 2 bytes * 10 s = 320,000 bytes (~312 KB) + ~44-byte header. `hound` is version 3.5.1, **last published 2023-09-25** — stale but the WAV/PCM format itself hasn't changed and the crate is simple/complete; staleness is not a real risk here (source: `/api/v1/crates/hound`).
- **FLAC via `flacenc`**: version 0.5.1, last published 2025-12-18, described as a **pure-Rust** FLAC encoder (no native C dependency). Lossless, typically compresses speech-like 16-bit PCM to roughly 45-65% of WAV size for voice content, i.e. ballpark ~150-200 KB for the same 10 s clip (rule-of-thumb estimate, not a specific FLAC-on-this-exact-file measurement).
- **Opus via `opus` crate**: version 0.3.1, last published 2026-01-03 (actively maintained; description "Safe Rust bindings for libopus"). Note: the older **`audiopus`** crate is stale — newest version is a `0.3.0-rc.0` prerelease last published 2021-04-22, effectively unmaintained; prefer the `opus` crate if Opus is chosen. Opus at a speech-appropriate 16-24 kbps mono bitrate: 10 s clip ≈ 20-30 KB, a 10-15x reduction vs WAV. Cost: `opus` binds to **native libopus**, i.e. a C build dependency on both Windows and macOS — exactly the kind of "giant native build" the project wants to avoid per the local-fallback criteria in this same research task.
- **Does the size difference matter?** On a typical broadband/Wi-Fi uplink (multi-Mbps upload), the difference between 312 KB (WAV) and 25 KB (Opus) is on the order of tens to a couple hundred milliseconds of transfer time — noticeable but not the dominant term in a release-to-inject latency budget that also includes cloud STT processing time and BYOK round-trip. On a poor/metered connection (e.g. tethered mobile, hotel Wi-Fi), the gap becomes the difference between "instant" and "clearly waiting," so it is not purely academic.
- **Recommendation:** ship **WAV via `hound`** for Phase 1 (simplest, zero native deps, every cloud STT provider accepts WAV natively, matches what the STT-provider docs use as their example payload). Treat **FLAC via `flacenc`** as the natural upgrade path if upload latency on slow connections proves to be a real complaint — it's pure Rust (no native build headache, consistent with the project's Windows tray / no-console-flash and "no giant native build" constraints) and cuts payload size roughly in half. Avoid Opus unless a specific STT provider's API meaningfully favors/requires it, because it's the only option that reintroduces a native C library dependency into an otherwise pure-Rust pipeline.

## 4. Local fallback model (future phase, opt-in download, target <200 MB)

- **whisper-rs** 0.16.0 (crates.io, last published 2026-03-12), Rust bindings to whisper.cpp; supports CUDA/Metal/HIP/Vulkan/OpenBLAS/OpenMP acceleration. **whisper.cpp `tiny.en` ggml model is ~75 MB** (39M params) — well under the 200 MB budget, runs on essentially any device, and whisper.cpp itself has first-class Windows and macOS support (Metal on Apple Silicon, CUDA optionally on Windows, plain CPU everywhere). This is a real native-build dependency (whisper.cpp is C/C++, so cargo needs a C toolchain / cmake at build time), but it's a much smaller, better-trodden build than sherpa-onnx + full ONNX Runtime.
- **Moonshine (Useful Sensors)** ONNX models, exposed today via **sherpa-onnx's own Moonshine support** (https://k2-fsa.github.io/sherpa/onnx/moonshine/index.html) — meaning it would reuse the same sherpa-onnx Rust crate already spiked in [[sherpa-onnx-rust-api]] rather than requiring a new binding. Moonshine model weights run roughly 108-246 MB depending on variant, with a "tiny" variant reported around 27M params; Moonshine v2 is architected specifically for low-latency streaming (v2 Tiny ~50ms, v2 Small ~148ms, per the paper arXiv:2602.12241, published 2026). No separate/newer Rust ONNX runtime crate for Moonshine was found beyond the existing sherpa-onnx integration path.
- **Leading candidate for the local-fallback note:** **whisper-rs + whisper.cpp `tiny.en` (~75 MB)** — smallest, most mature, most widely deployed combination, simplest build story of the native-dependency options, and doesn't reopen the sherpa-onnx reliability question from [[hark-stt-stack-risk]] (issue #3267 only affects Parakeet TDT's modified_beam_search hotword path, not whisper.cpp at all). Moonshine via sherpa-onnx is a credible secondary option worth revisiting specifically because it reuses spike work already done, but whisper-rs's ecosystem maturity (bindings actively published 2026-03-12, huge existing user base) makes it the safer first pick for a "just needs to work offline as a fallback" feature. This is a future-phase note only, not a commitment.

## 5. LL-G index findings (kb/rust/llms.txt, fetched today)

Only **2 HIGH-severity Rust entries exist** in LL-G as of 2026-07-15, both already known and already captured in root `CLAUDE.md`:
1. Blocking `std::fs` (or any blocking IO) on the Tokio executor starves other tasks — reinforces the §1/§2 design split above (keep the plain-upload path off any shared tokio runtime, or isolate the WebSocket adapter's runtime).
2. GUI-subsystem binary spawning a visible console for child processes unless `CREATE_NO_WINDOW` is set (Windows tray gotcha, unrelated to network IO directly but already in CLAUDE.md).

The LL-G rust index does **not** currently have dedicated entries for HTTP clients, TLS, or multipart upload specifically — those topics aren't covered yet, so this research doc is the closest thing to that guidance for now. No new HIGH entries surfaced beyond what CLAUDE.md already states.

## Synthesis / recommended dependency set

```toml
# Phase 1 cloud STT upload path (blocking, single POST per utterance)
reqwest = { version = "0.13", default-features = false, features = ["blocking", "multipart", "rustls-tls-webpki-roots", "json"] }
hound = "3.5"

# If/when a streaming (Deepgram-style) adapter is added
tokio = { version = "1", features = ["rt-multi-thread", "macros", "net", "time"] }
tokio-tungstenite = { version = "0.30", features = ["rustls-tls-webpki-roots"] }
# or simply depend on the official `deepgram = "0.10"` crate directly for that adapter,
# which already vendors the tokio + reqwest + tokio-tungstenite combination above.

# Later, opt-in: pure-Rust FLAC upgrade if upload latency on slow links matters
flacenc = "0.5"

# Future-phase, opt-in local fallback (not committed)
whisper-rs = "0.16"   # whisper.cpp tiny.en, ~75 MB model download
```

**Threading/IO design recommendation:** keep the default cloud-STT provider on `reqwest::blocking` executed inside the pipeline's existing dedicated worker thread (the same thread that already owns hotkey -> audio -> STT -> injection, per the project's one-hard-rule threading model). This needs no tokio runtime at all for the common case, sidesteps the "blocking IO on tokio executor" LL-G HIGH entry entirely (there's no shared executor to block), and keeps the binary simpler. Only introduce a tokio runtime when a WebSocket-streaming provider (e.g. Deepgram's live endpoint) is actually wired up, and scope that runtime's lifetime to that adapter alone rather than promoting it to a global app-wide runtime — this avoids forcing every other provider integration (simple REST upload) to pay the complexity/binary-size cost of async for no benefit.

**Audio format recommendation:** ship WAV (`hound`) in Phase 1 for simplicity and zero native dependencies; treat pure-Rust FLAC (`flacenc`) as the first upgrade if slow-connection upload latency becomes a measured problem. Avoid Opus/`opus` unless a specific provider strongly prefers it, since it's the only path that reintroduces a native C library build.

**Local fallback candidate (future phase only):** whisper-rs + whisper.cpp `tiny.en` (~75 MB download), maintained (last publish 2026-03-12), mature Windows/macOS support. Moonshine via sherpa-onnx is a secondary option worth a follow-up look since it reuses the existing sherpa-onnx spike.

## Sources
- https://docs.rs/reqwest/latest/reqwest/blocking/multipart/index.html
- https://crates.io/crates/reqwest and https://crates.io/api/v1/crates/reqwest
- https://crates.io/crates/ureq and https://crates.io/api/v1/crates/ureq
- https://github.com/pyo3/maturin/pull/2997
- https://docs.rs/ureq/latest/ureq/
- https://crates.io/crates/deepgram and https://crates.io/api/v1/crates/deepgram
- https://github.com/deepgram/deepgram-rust-sdk/blob/main/Cargo.toml
- https://crates.io/api/v1/crates/tokio-tungstenite
- https://crates.io/api/v1/crates/hound
- https://crates.io/api/v1/crates/flacenc
- https://crates.io/api/v1/crates/opus
- https://crates.io/api/v1/crates/audiopus
- https://crates.io/api/v1/crates/whisper-rs
- https://k2-fsa.github.io/sherpa/onnx/moonshine/index.html
- https://arxiv.org/pdf/2602.12241 (Moonshine v2 paper)
- https://raw.githubusercontent.com/BoardPandas/LL-G/main/llms.txt and its kb/rust/llms.txt index

**Why:** Hark's plan is pivoting from local sherpa-onnx/Parakeet STT (rejected for ~1.1 GB footprint) to BYOK cloud STT, and the rewritten plan needs a concrete, dated, verified dependency set rather than assumptions carried over from the local-model research already captured in [[hark-stt-stack-risk]] and [[sherpa-onnx-rust-api]].

**How to apply:** Paste the "Synthesis / recommended dependency set" section directly into the rewritten plan's stack table and Cargo.toml scaffolding. Re-verify ureq's multipart feature stability and the Deepgram SDK's move toward 1.0 GA before final implementation, since both were explicitly flagged as still-evolving as of this research date.

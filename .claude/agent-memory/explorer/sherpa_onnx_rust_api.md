---
name: sherpa-onnx-rust-api
description: Concrete Rust API/build specifics for sherpa-onnx crate v1.13.4 + Parakeet TDT 0.6B v2, for the Hark Phase-1 spike implementation spec
metadata:
  type: project
---

Concrete implementation facts gathered 2026-07-15 for the Hark Phase-1 Rust spike (load Parakeet TDT via `sherpa-onnx` crate, batch-decode a wav, test hotwords reachability). Complements [[hark-stt-stack-risk]], which covers the *risk* (issue #3267); this memory covers the *how*.

## 1. Crate + Cargo.toml

- Crate name on crates.io: **`sherpa-onnx`** (module path `sherpa_onnx`), version **1.13.4** as of 2026-07-15 (matches version pinned in the project's own `rust-api-examples/Cargo.toml`, itself versioned 1.13.4 in lockstep with the main repo release).
- Minimal dependency line (from `rust-api-examples/Cargo.toml`, https://raw.githubusercontent.com/k2-fsa/sherpa-onnx/master/rust-api-examples/Cargo.toml):
  ```toml
  sherpa-onnx = { version = "1.13.4", default-features = false }
  ```
- Feature flags (same Cargo.toml):
  ```toml
  [features]
  default = ["static"]
  mic     = ["cpal"]
  static  = ["sherpa-onnx/static"]
  shared  = ["sherpa-onnx/shared"]
  ```
  - `static` (default): statically links the native sherpa-onnx/onnxruntime libs, no manual `LD_LIBRARY_PATH` needed at runtime.
  - `shared`: dynamic linking; build script adds rpath entries and copies required shared runtime libs automatically on Unix (per crate docs on docs.rs).
  - **Could NOT confirm** explicit `cuda`, `directml`, or `coreml` feature strings in the crate's own feature list — the crate docs only clearly document `static`/`shared`. CUDA/CoreML likely selected via the `provider` config field (e.g. `"cuda"`, `"coreml"`) rather than a Cargo feature, but this needs verification during the spike if GPU accel is ever wanted (out of scope for Phase 1 CPU spike).
- Build behavior (build.rs, from docs.rs crate-level docs): if `SHERPA_ONNX_LIB_DIR` env var is **not** set, the build script **auto-downloads a matching prebuilt `-lib` archive** from GitHub releases (e.g. `sherpa-onnx-v1.13.4-linux-x64-static-lib.tar.bz2` for Fedora x86_64) and links it — no manual ONNX Runtime install needed. Set `SHERPA_ONNX_LIB_DIR` to point at a pre-existing lib dir to skip the download.
- Linux-specific gotchas: none documented beyond static/shared distinction (no glibc version pin, no explicit cmake/cc requirement mentioned in the crate docs excerpt fetched). **Verify during spike**: confirm `cargo build` succeeds on Fedora 44 x86_64 without extra system packages (cc/cmake may still be pulled transitively — not confirmed either way).

## 2. Model files + download

- Parakeet TDT 0.6B v2 (English) archives, from k2-fsa release page (https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/...) and doc page https://k2-fsa.github.io/sherpa/onnx/pretrained_models/offline-transducer/nemo-transducer-models.html:
  - int8: `sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8.tar.bz2`
  - fp16: `sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-fp16.tar.bz2`
  - full precision (non-quantized): `sherpa-onnx-nemo-parakeet-tdt-0.6b-v2.tar.bz2`
- Archive contents (int8 variant): `encoder.int8.onnx` (~622M), `decoder.int8.onnx` (~6.9M), `joiner.int8.onnx` (~1.7M), `tokens.txt`, `test_wavs/` (includes `0.wav`). Total extracted ~1.3GB. **Not fully confirmed**: the full-precision/fp16 archive file names inside (likely `encoder.onnx`/`encoder.fp16.onnx` by analogy — verify during spike) and their exact sizes.
- Fetch + extract (int8, matches the project's own example script `run-nemo-parakeet-en.sh`):
  ```bash
  wget https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8.tar.bz2
  tar xvf sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8.tar.bz2
  rm sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8.tar.bz2
  ```
- **Could NOT confirm** whether a `bpe.vocab` file (needed for hotwords per PR #3077, see below) ships inside the standard release archive or must be generated separately via the repo's `generate_bpe_vocab.py` script (referenced in PR #3077 discussion, https://github.com/k2-fsa/sherpa-onnx/pull/3077). **Verify during spike** — if missing, look at `scripts/nemo/parakeet-tdt-0.6b-v2/` in the k2-fsa/sherpa-onnx repo for the generator script.

## 3. Rust API surface (offline transducer decode)

Source: `rust-api-examples/examples/nemo_parakeet.rs` in k2-fsa/sherpa-onnx (https://raw.githubusercontent.com/k2-fsa/sherpa-onnx/master/rust-api-examples/examples/nemo_parakeet.rs), matching the shell driver `run-nemo-parakeet-en.sh`.

Construction path:
```rust
let mut recognizer_config = OfflineRecognizerConfig::default();
recognizer_config.model_config.transducer = OfflineTransducerModelConfig {
    encoder: Some(args.encoder.clone()),
    decoder: Some(args.decoder.clone()),
    joiner: Some(args.joiner.clone()),
};
recognizer_config.model_config.tokens = Some(args.tokens.clone());
recognizer_config.model_config.provider = Some(args.provider.clone());
// model_config.model_type should be set to "nemo_transducer" for Parakeet TDT
// (confirmed value from k2-fsa docs page for nemo-transducer-models; the example
// itself doesn't set it explicitly, so auto-detection may work — verify during spike)

let recognizer = OfflineRecognizer::create(&recognizer_config)
    .expect("Failed to create OfflineRecognizer");

let stream = recognizer.create_stream();
stream.accept_waveform(wave.sample_rate(), wave.samples());
recognizer.decode(&stream);

if let Some(result) = stream.get_result() {
    println!("Decoded text: {}", result.text);
}
```

Reference example run command (`run-nemo-parakeet-en.sh`):
```bash
cargo run --example nemo_parakeet -- \
    --wav "./sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8/test_wavs/0.wav" \
    --encoder "./sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8/encoder.int8.onnx" \
    --decoder "./sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8/decoder.int8.onnx" \
    --joiner "./sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8/joiner.int8.onnx" \
    --tokens "./sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8/tokens.txt" \
    --provider cpu \
    --num-threads 2 \
    --debug
```

Types confirmed via docs.rs (`https://docs.rs/sherpa-onnx/latest/sherpa_onnx/`):
- `OfflineRecognizerConfig` — top-level config, `Default` impl exists (start from `::default()` and fill fields).
- `OfflineModelConfig` (nested at `.model_config`) — has ~18 model-family sub-configs (`transducer`, `paraformer`, `nemo_ctc`, `whisper`, `sense_voice`, `qwen3_asr`, etc.) plus shared fields: `tokens: Option<String>`, `num_threads: i32`, `debug: bool`, `provider: Option<String>`, `model_type: Option<String>`, `modeling_unit: Option<String>`, `bpe_vocab: Option<String>`, `telespeech_ctc: Option<String>`.
- `OfflineTransducerModelConfig` — only 3 fields: `encoder: Option<String>`, `decoder: Option<String>`, `joiner: Option<String>` (no model_type/modeling_unit here — those live one level up on `OfflineModelConfig`).
- Sample format: `stream.accept_waveform(sample_rate: u32, samples: &[f32])` — samples are normalized f32 PCM (typical range -1.0..1.0), matching the crate's own `Wave` loader output. Confirms 16kHz mono f32 is the expected input; no explicit range assertion found in docs excerpt, but standard sherpa-onnx convention is normalized float32.

## 4. Hotwords config fields

Confirmed on **`OfflineRecognizerConfig`** itself (top-level, not nested under model_config) via docs.rs:
- `decoding_method: Option<String>` — values `"greedy_search"` or `"modified_beam_search"` (from k2-fsa docs + issue #3267 repro command).
- `max_active_paths: i32`
- `hotwords_file: Option<String>`
- `hotwords_score: f32`
- `blank_penalty: f32`
- `rule_fsts` / `rule_fars: Option<String>`
- `hr: HomophoneReplacerConfig`

Plus, on `OfflineModelConfig`: `modeling_unit: Option<String>` (values seen: `"bpe"`, `"cjkchar"`, `"cjkchar+bpe"`) and `bpe_vocab: Option<String>` — **required alongside hotwords when modeling_unit is bpe/cjkchar+bpe**, per k2-fsa hotwords docs (https://k2-fsa.github.io/sherpa/onnx/hotwords/index.html) and PR #3077.

Hotwords file format (same page): one phrase per line, plain text (not pre-tokenized), tokenization uses the modeling unit rules internally. Optional per-line boost score via trailing `:score`, e.g.:
```
SPEECH RECOGNITION :3.5
```
The score must be the last token on the line (can't be split by hotword text after it).

**Rust binding reachability**: docs.rs confirms all these fields (`decoding_method`, `hotwords_file`, `hotwords_score`, `modeling_unit`, `bpe_vocab`) DO exist on the Rust struct definitions — they are generic top-level config fields shared across all model backends via the underlying C API, not something PR #3077 added only to C++/Python bindings. PR #3077 (https://github.com/k2-fsa/sherpa-onnx/pull/3077) added the *decoder logic* (`OfflineTransducerModifiedBeamSearchNeMoDecoder`) in C++ and exposed it via the existing generic C API / config surface, which the Rust crate already wraps generically. So the fields should be reachable from Rust — **this is inference from struct field presence, not a working Rust code sample with hotwords enabled** (no dedicated Rust hotwords example exists in `rust-api-examples/`, confirmed by directory listing). Treat as "should work, verify empirically during the spike" per [[hark-stt-stack-risk]]'s note about issue #3267 reliability.

## 5. Reading a wav in Rust

**No need for `hound`** — the crate ships its own `Wave` loader:
```rust
// sherpa_onnx::Wave (module path per docs.rs)
let wave = Wave::read("path/to/file.wav")?;   // -> Result<Wave, Error>
wave.sample_rate()  // -> u32
wave.samples()      // -> &[f32], normalized PCM
```
This is exactly what `nemo_parakeet.rs` uses (`wave.sample_rate()`, `wave.samples()` fed into `stream.accept_waveform(...)`). Use this instead of pulling in `hound` as a separate dependency — one less crate, and it's already guaranteed to produce the sample format the recognizer expects.

## Sources
- https://crates.io/crates/sherpa-onnx
- https://docs.rs/sherpa-onnx/latest/sherpa_onnx/ (and sub-pages for `OfflineRecognizerConfig`, `OfflineModelConfig`, `OfflineTransducerModelConfig`)
- https://github.com/k2-fsa/sherpa-onnx/tree/master/rust-api-examples
- https://raw.githubusercontent.com/k2-fsa/sherpa-onnx/master/rust-api-examples/Cargo.toml
- https://raw.githubusercontent.com/k2-fsa/sherpa-onnx/master/rust-api-examples/run-nemo-parakeet-en.sh
- https://raw.githubusercontent.com/k2-fsa/sherpa-onnx/master/rust-api-examples/examples/nemo_parakeet.rs
- https://raw.githubusercontent.com/k2-fsa/sherpa-onnx/master/rust-api-examples/for-advanced-users.md
- https://k2-fsa.github.io/sherpa/onnx/pretrained_models/offline-transducer/nemo-transducer-models.html
- https://k2-fsa.github.io/sherpa/onnx/hotwords/index.html
- https://github.com/k2-fsa/sherpa-onnx/pull/3077
- https://github.com/k2-fsa/sherpa-onnx/issues/3267

**Why:** These are the exact Cargo.toml lines, struct/field names, download URLs, and code sketch needed so the Phase-1 spike implementation spec doesn't require re-deriving the API surface from scratch.

**How to apply:** Paste the Cargo.toml snippet, download commands, and code sketch directly into the spike's implementation spec. Flag the 3 unconfirmed items (CUDA/CoreML feature strings, bpe.vocab inclusion in release archive, and whether Rust hotwords actually work end-to-end) as explicit "verify during spike" checklist items, and update this memory with the real answers once found — especially the bpe.vocab question, since it blocks step 4 (hotwords) entirely if the vocab file must be generated from the original NeMo checkpoint rather than being shipped in the ONNX release archive.

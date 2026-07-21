# On-Device Speech-to-Text

Optional, opt-in transcription that runs entirely on the user's machine. Off by
default; nothing is downloaded, loaded, or linked into the hot path until the
user asks for it.

Related: [Transcription](TRANSCRIPTION.md) (the BYOK cloud path this falls back
from), [Dictionary](DICTIONARY.md) (the phonetic pass that corrects local
output the same way it corrects cloud output).

## Why it exists

Cloud transcription has two failure modes a dictation app cannot hide: the
network goes away, and the provider has a bad day. Either one costs the user the
sentence they just spoke. On-device STT makes that recoverable, and for users who
would rather not send audio to a provider at all, it removes the cloud entirely.

## The three modes

Set under Settings → On-device model, stored as `[local_stt] mode`.

| Mode | Behavior | API key needed |
|---|---|---|
| `off` (default) | Never used. No download, no RAM, no behavior change. | Yes |
| `fallback` | Cloud first. Any cloud failure — timeout, connection, 5xx, **or auth** — falls back to the local model. | Yes |
| `primary` | Local only. No provider is ever contacted. | **No** |

Auth failures fall back deliberately: a user whose key has expired would rather
have their words than a diagnostic. The dictation record still names which engine
produced the text, so history never misattributes the result.

`primary` is the only mode that lets the pipeline start without a resolvable API
key — see `PipelineController::start`, which skips the keychain failure path when
`mode.uses_cloud()` is false.

## The model

**Parakeet TDT 0.6B v3, int8 ONNX** — NVIDIA, CC-BY-4.0, 25 languages.

| | |
|---|---|
| Download | ~670 MB across 4 files |
| RAM once loaded | ~1.2 GB resident, ~2 GB while decoding |
| Latency, ~5 s clip, CPU-only | ~1.5–2 s |
| Decoding | `greedy_search`, always |

Weights are **not** bundled. They download on demand into
`<data_dir>/models/<model-id>/` from a public Hugging Face repo (no auth token),
with `Range`-based resume, live progress, cancel, and a pinned-sha256 integrity
check per file.

`greedy_search` is not a tuning choice. `modified_beam_search` with hotwords is
the open [sherpa-onnx #3267](https://github.com/k2-fsa/sherpa-onnx/issues/3267)
hallucination bug; Hark never needs it because the dictionary's phonetic
post-correction already handles term biasing, provider-agnostically.

v3 is the latest revision and costs ~0.27 pp of English accuracy against the
English-only v2 in exchange for 24 more languages. Switching back is a one-line
config change once `parakeet-tdt-0.6b-v2-int8` is added to the catalog in
`hark-local-stt/src/model.rs`.

## The fallback deadline

This is the detail that makes fallback worth having.

`hark_stt::TOTAL_TIMEOUT_MS` is 15 s. Waiting that out and *then* spending ~2 s
decoding locally produces a 17-second dictation — worse than no fallback. So when
`mode = "fallback"` **and the weights are actually on disk**, the pipeline builds
its STT client with `local_stt.fallback_after_ms` (default 6000) instead. With no
model downloaded, nothing changes and the normal 15 s applies.

## Residency

The engine loads on first use and stays resident. Reloading ~670 MB per dictation
would dominate release-to-inject latency, which is the product.

The consequence is that Hark idles at over a gigabyte of RAM once local STT has
been used in a session. That is the trade, and it is why Settings offers a Delete
button. The engine is dropped (and its RAM released) whenever the pipeline
restarts — every settings Save does that.

First load takes seconds, so the pipeline emits `PipelineEvent::LoadingLocalModel`
before blocking and the status bar names it. Without that the app simply looks
frozen on the first local dictation.

## Build-time cost and the `local-engine` feature

Statically linking sherpa-onnx + ONNX Runtime costs **~28 MB of binary**
(measured 2026-07-21, linux-x64: a 0.3 MB probe became 28.0 MB). Every user pays
that whether or not they enable local STT.

So it sits behind a Cargo feature, on by default:

```bash
cargo build --release                        # includes the engine
cargo build --release --no-default-features  # slim, cloud-only
```

`hark-app/local-engine` → `hark-local-stt/engine` + `hark-pipeline/engine`.

Without the feature, the model-management half still compiles and tests
normally, `LocalEngine::load` returns `EngineUnavailable`, and the Settings
section says so plainly instead of offering a toggle that cannot work.

## Crate layout

```
hark-local-stt/
  model.rs     ModelSpec, pinned file sizes + sha256, on-disk status
  download.rs  streaming resumable download, progress, cancel, verify
  engine.rs    LocalEngine (behind `engine`): sherpa-onnx offline transducer
hark-pipeline/
  local.rs     LocalPlan (lazy resident engine) + the cloud/local policy
hark-app/
  model_download.rs        download lifecycle for the UI (thread + mpsc + poll)
  ui/settings/local.rs     the Settings section
```

The policy in `hark-pipeline/src/local.rs` is expressed over a `Transcriber`
trait so it is unit-tested without a network or a 670 MB model on disk.

## Gotchas worth keeping

- **Never `.bytes()` a model file.** `hark-update` buffers its small installer
  that way; doing the same to a 652 MB encoder is a 652 MB RAM spike. The
  downloader streams to disk.
- **The shared HTTP client's 15 s timeout must be overridden per download
  request**, or every model download dies at 15 seconds.
- **`Accept-Encoding: identity` on download requests.** A compressed transfer
  makes `Content-Length` disagree with the bytes written, which silently breaks
  resume arithmetic.
- **A `200` response to a `Range` request means the server ignored it.** Append
  in that case and you corrupt the file; the downloader restarts from zero.
- **`.part` suffixes must be additive, not `with_extension`.** `with_extension`
  would turn `encoder.int8.onnx` into `encoder.int8.part` and collide across
  files.
- **A model id becomes a directory name**, so `hark-config` validates it as a
  plain path segment — no separators, no `..`.

## Verifying on real hardware

The engine cannot be meaningfully tested without the weights, so there is an
opt-in integration test that reports load and decode wall times:

```bash
HARK_LOCAL_MODEL_DIR=<model dir> \
  cargo test -p hark-local-stt --features engine -- --ignored --nocapture
```

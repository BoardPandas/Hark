<!-- PAGE_ID: hark_07b_on_device_stt -->
# On-Device Speech-to-Text

Optional, opt-in transcription that runs entirely on the user's machine. Off by
default; nothing is downloaded, loaded, or linked into the hot path until the
user asks for it.

Related: [Transcription](TRANSCRIPTION.md) (the BYOK cloud path this falls back
from), [Spellbook](SPELLBOOK.md) (the phonetic pass that corrects local
output the same way it corrects cloud output).

## Why it exists

Cloud transcription has two failure modes a dictation app cannot hide: the
network goes away, and the provider has a bad day. Either one costs the user the
sentence they just spoke. On-device STT makes that recoverable, and for users who
would rather not send audio to a provider at all, it removes the cloud entirely.

## The three modes

Set in Settings → Dictation → Speech to text, below the cloud provider: the **Use on-device transcription** toggle with **Primary** and **Backup**, under a chip naming the engine in the on-device hue History also uses ([ui/settings/local.rs:17-89](../../crates/hark-app/src/ui/settings/local.rs#L17-L89)), inside the card that also hides the cloud controls in Primary and suggests on-device when no cloud key is stored ([ui/settings/stt_card.rs:17-95](../../crates/hark-app/src/ui/settings/stt_card.rs#L17-L95)). Stored as `[local_stt] mode` ([hark-config/src/local.rs:14-30](../../crates/hark-config/src/local.rs#L14-L30)). Turning the toggle on picks Backup when a cloud key is stored and Primary when none is; the download still starts only from the model card's own button.

| Mode | Behavior | API key needed |
|---|---|---|
| `off` (default) | Never used. No download, no RAM, no behavior change. | Yes |
| `fallback` | Cloud first. Any cloud failure — timeout, connection, 5xx, **or auth** — falls back to the local model. | Yes |
| `primary` | Local only. No provider is ever contacted. | **No** |

Auth failures fall back deliberately: a user whose key has expired would rather
have their words than a diagnostic. The dictation record still names which engine
produced the text, so history never misattributes the result.

One case reverses the order. With a streaming provider (Gemini Live, or OpenAI
with Live transcribe), a stream that fails has already spent the dictation's
first cloud attempt. In `fallback` mode with the model ready, the on-device
engine then goes first and the cloud replay becomes the rescue: a stalled live
turn means the provider is struggling right now, and replaying to it made a
rescued dictation take 8 to 11 seconds
([hark-pipeline/src/local.rs:241-286](../../crates/hark-pipeline/src/local.rs#L241-L286)).

`primary` is the only mode that lets the pipeline start without a resolvable API
key — see `PipelineController::start_worker`, which skips the keychain failure
path when `mode.uses_cloud()` is false
([hark-app/src/pipeline.rs:183-198](../../crates/hark-app/src/pipeline.rs#L183-L198),
[hark-config/src/local.rs:42](../../crates/hark-config/src/local.rs#L42)).

## The model

**Parakeet TDT 0.6B v3, int8 ONNX** — NVIDIA, CC-BY-4.0, 25 languages
([hark-local-stt/src/model.rs:119-150](../../crates/hark-local-stt/src/model.rs#L119-L150)).

| | |
|---|---|
| Download | ~670 MB across 4 files |
| RAM once loaded | ~1.2 GB resident, ~2 GB while decoding |
| Latency, ~5 s clip, CPU-only | ~1.5–2 s |
| Decoding | `greedy_search`, always |

Weights are **not** bundled. They download on demand into
`<data_dir>/models/<model-id>/` from a public Hugging Face repo (no auth token),
with `Range`-based resume, live progress, cancel, and a pinned-sha256 integrity
check per file ([download.rs:97-160](../../crates/hark-local-stt/src/download.rs#L97-L160),
[model.rs:19](../../crates/hark-local-stt/src/model.rs#L19)).

`greedy_search` is not a tuning choice ([engine.rs:83](../../crates/hark-local-stt/src/engine.rs#L83)). `modified_beam_search` with hotwords is
the open [sherpa-onnx #3267](https://github.com/k2-fsa/sherpa-onnx/issues/3267)
hallucination bug; Hark never needs it because the spellbook's phonetic
post-correction already handles term biasing, provider-agnostically.

v3 is the pinned multilingual build, not the newest Parakeet: it costs ~0.27 pp
of English accuracy against the English-only v2 in exchange for 24 more
languages, and the English-only `parakeet-unified-en-0.6b` (2026-04) also exists.
Keeping v3 was decided on 2026-10-29 without an A/B test. Switching back is a one-line
config change once `parakeet-tdt-0.6b-v2-int8` is added to the catalog in
[`hark-local-stt/src/model.rs:151`](../../crates/hark-local-stt/src/model.rs#L151).

## The fallback deadline

This is the detail that makes fallback worth having.

Without a fallback, a dictation's cloud request is allowed 6 s plus half the
clip's length, capped at `hark_stt::TOTAL_TIMEOUT_MS`, which is 15 s
([hark-stt/src/lib.rs:31-51](../../crates/hark-stt/src/lib.rs#L31-L51)). Waiting
out a long clip's bound and *then* spending ~2 s decoding locally produces a
dictation slower than no fallback at all. So when `mode = "fallback"` **and the
weights are actually on disk**, the pipeline holds every cloud request to
`local_stt.fallback_after_ms` (default 6000, floor 500) instead, however long
the clip is
([hark-config/src/local.rs:69](../../crates/hark-config/src/local.rs#L69),
[:106-108](../../crates/hark-config/src/local.rs#L106-L108),
[hark-pipeline/src/lib.rs:248-273](../../crates/hark-pipeline/src/lib.rs#L248-L273)).

The leash is applied in two places that must agree. The HTTP client is built
with `client_with_timeout(leash)`, and the adapter with
`build_with_total_timeout(…, leash)`. The second is the one that decides: each
batch adapter sets a timeout on the request itself, which replaces the
client's, so a short client alone would be stretched back out to the
clip-length bound
([hark-stt/src/lib.rs:165-176](../../crates/hark-stt/src/lib.rs#L165-L176),
[:251-262](../../crates/hark-stt/src/lib.rs#L251-L262)). With no model
downloaded, nothing changes and the clip-length bound applies.

## Residency

The engine stays resident once it is loaded. Reloading ~670 MB per dictation
would dominate release-to-inject latency, which is the product.

**It is loaded when the pipeline starts, not on first use.** In Backup
(`fallback`) and Primary mode with the model on disk, the worker asks a
short-lived helper thread, `hark-local-preload`, to read the model in as soon as
it starts, and goes on to pre-warm the network connection without waiting for
it. The first dictation in Primary, or the first rescue in Backup, then finds
the engine already loaded and skips the cold load, which the code puts at about
1.2 s. Off never loads anything, and a model that is not downloaded has nothing
to load
([local.rs:17-23](../../crates/hark-pipeline/src/local.rs#L17-L23),
[local.rs:150-166](../../crates/hark-pipeline/src/local.rs#L150-L166),
[worker.rs:126-132](../../crates/hark-pipeline/src/worker.rs#L126-L132)).

The slot that holds the engine has three states: empty, loading, and loaded. A
dictation that needs the engine while the preload is still running waits for it
rather than starting a second load, which would hold two copies of the weights.
A preload that fails, or whose thread could not be started, is only a warning:
the slot goes back to empty and the engine loads on first use, exactly as it did
before preloading existed. The helper thread is never joined. A pipeline retired
before the thread gets going skips the load, and one retired mid-load lets the
thread finish and drop the engine
([local.rs:25-124](../../crates/hark-pipeline/src/local.rs#L25-L124),
[local.rs:157-165](../../crates/hark-pipeline/src/local.rs#L157-L165)).
`LocalEngine` is `Send`, which is what allows it to be built on the helper
thread and handed to the worker; a test pins that
([engine.rs:25-30](../../crates/hark-local-stt/src/engine.rs#L25-L30),
[local.rs:589-593](../../crates/hark-pipeline/src/local.rs#L589-L593)).

The consequence is that in Backup or Primary, Hark holds the model in memory
from the moment the pipeline starts, whether or not a dictation ever needs it.
The Backup line in Settings says so, quoting the measured resident figure from
the table above ("keeps the model loaded in memory (about 1.2 GB)"); the weights
themselves are about 670 MB on disk
([ui/settings/stt_card.rs:175-194](../../crates/hark-app/src/ui/settings/stt_card.rs#L175-L194)).
That is the trade, and it is why Settings offers a Delete button. The engine is
dropped (and its RAM released) whenever the pipeline restarts — every settings
Save does that — and the new pipeline preloads again.

`PipelineEvent::LoadingLocalModel` is sent only when a dictation actually has to
wait: nothing is loaded yet, and either the preload is still running or a lazy
load is next (`LocalPlan::needs_load_wait`). A preload that has already finished
costs the dictation nothing, so the status bar never says "loading model" for
it. When the wait is real, the event goes out before the worker blocks, so the
app does not look frozen
([events.rs:90-94](../../crates/hark-pipeline/src/events.rs#L90-L94),
[worker/engines.rs:75-82](../../crates/hark-pipeline/src/worker/engines.rs#L75-L82),
[local.rs:177-184](../../crates/hark-pipeline/src/local.rs#L177-L184)).

## Build-time cost and the `local-engine` feature

Statically linking sherpa-onnx + ONNX Runtime costs **~28 MB of binary**
(measured 2026-07-21, linux-x64: a 0.3 MB probe became 28.0 MB). Every user pays
that whether or not they enable local STT.

So it sits behind a Cargo feature, on by default:

```bash
cargo build --release                        # includes the engine
cargo build --release --no-default-features  # slim, cloud-only
```

`--no-default-features` also drops `live-stt`, the WebSocket client behind
Gemini and OpenAI's Live transcribe; add `--features live-stt` to keep them
([hark-app/Cargo.toml](../../crates/hark-app/Cargo.toml)).

`hark-app/local-engine` → `hark-local-stt/engine` + `hark-pipeline/engine`
([hark-app/Cargo.toml:199-200](../../crates/hark-app/Cargo.toml#L199-L200),
[hark-local-stt/Cargo.toml:34-36](../../crates/hark-local-stt/Cargo.toml#L34-L36)).

Without the feature, the model-management half still compiles and tests
normally, `LocalEngine::load` returns `EngineUnavailable`
([engine.rs:100-107](../../crates/hark-local-stt/src/engine.rs#L100-L107)), and the Settings
section says so plainly instead of offering a toggle that cannot work
([ui/settings/local.rs:91-105](../../crates/hark-app/src/ui/settings/local.rs#L91-L105)).

## Crate layout

```
hark-local-stt/
  model.rs     ModelSpec, pinned file sizes + sha256, on-disk status
  download.rs  streaming resumable download, progress, cancel, verify
  engine.rs    LocalEngine (behind `engine`): sherpa-onnx offline transducer
hark-pipeline/
  local.rs           LocalPlan (resident engine, preloaded at start) + the cloud/local policy
  worker/engines.rs  binds the worker's cloud adapter and engine to that policy
  plan.rs            build_local: the plan from settings, or None when unusable
hark-app/
  model_download.rs        download lifecycle for the UI (thread + mpsc + poll)
  ui/settings/local.rs     the on-device half of the Speech to text card
  ui/settings/stt_card.rs  the card, the no-key suggestion, the mode decisions
```

The policy in `hark-pipeline/src/local.rs` is expressed over a `Transcriber`
trait ([local.rs:222-230](../../crates/hark-pipeline/src/local.rs#L222-L230)) so it is unit-tested
without a network or a 670 MB model on disk. The engine slot is generic over
the engine type for the same reason: the wait-for-preload behaviour is tested
with a stand-in instead of the real model
([local.rs:25-36](../../crates/hark-pipeline/src/local.rs#L25-L36)).

## Gotchas worth keeping

- **Never `.bytes()` a model file.** `hark-update` buffers its small installer
  that way; doing the same to a 652 MB encoder is a 652 MB RAM spike. The
  downloader streams to disk.
- **The shared HTTP client's 15 s timeout must be overridden per download
  request**, or every model download dies at 15 seconds.
- **`Accept-Encoding: identity` on download requests** ([download.rs:139](../../crates/hark-local-stt/src/download.rs#L139)). A compressed transfer
  makes `Content-Length` disagree with the bytes written, which silently breaks
  resume arithmetic.
- **A `200` response to a `Range` request means the server ignored it** ([download.rs:155-161](../../crates/hark-local-stt/src/download.rs#L155-L161)). Append
  in that case and you corrupt the file; the downloader restarts from zero.
- **`.part` suffixes must be additive, not `with_extension`.** `with_extension`
  would turn `encoder.int8.onnx` into `encoder.int8.part` and collide across
  files.
- **A model id becomes a directory name**, so `hark-config` validates it as a
  plain path segment — no separators, no `..` ([hark-config/src/local.rs:87-96](../../crates/hark-config/src/local.rs#L87-L96)).

## Verifying on real hardware

The engine cannot be meaningfully tested without the weights, so there is an
opt-in integration test that reports load and decode wall times
([tests/engine_real_model.rs:49](../../crates/hark-local-stt/tests/engine_real_model.rs#L49)):

```bash
HARK_LOCAL_MODEL_DIR=<model dir> \
  cargo test -p hark-local-stt --features engine -- --ignored --nocapture
```

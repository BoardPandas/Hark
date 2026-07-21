# Plan — Optional On-Device STT (local model, fallback or primary)

**Created:** 2026-07-21
**Status:** IMPLEMENTED 2026-07-21 in v0.18.0. CP0 came back **+27.7 MB**, which
is why the engine ships behind the default-on `local-engine` Cargo feature
rather than unconditionally. See `Docs/features/ON_DEVICE_STT.md`.
**Supersedes:** the "Local fallback (later phase, opt-in)" line in `tasks/plan-repo.md` §5 (which named `whisper-rs` + `tiny.en`)

---

## 1. Goal

Let a user opt into an on-device STT model that runs with no network:

- **Off** (default) — nothing downloaded, no RAM, no behavior change.
- **Fallback** — cloud first; local rescues the dictation when cloud fails or times out.
- **Primary** — local only; the cloud provider is never called and **no API key is required**.

The model is **downloaded on demand** from the Settings UI behind an explicit toggle
+ Download button, with live byte progress, resume, and cancel.

## 2. Decisions (locked 2026-07-21)

| Decision | Choice | Why |
|---|---|---|
| Engine | `sherpa-onnx` **1.13.4** (2026-07-08, Apache-2.0) | Verified current on crates.io. No CMake/C++ toolchain — the crate pulls a prebuilt native lib. Existing verified API notes in `.claude/agent-memory/explorer/sherpa_onnx_rust_api.md`. |
| Model | **Parakeet TDT 0.6B v3**, int8 ONNX | Latest revision (v2 is the English-only predecessor). Best accuracy per byte of any candidate; ~1.5–2 s for a 5 s clip on CPU with no GPU. |
| Decoding | `greedy_search` | `modified_beam_search` + hotwords is the open sherpa-onnx #3267 hallucination bug. We never need it: the existing `hark-dictionary` phonetic pass corrects local output exactly as it does cloud output. |
| Distribution | HF individual files, **not** the tarball | 670 MB vs 487 MB, but no bzip2/tar dependency, per-file resume, and progress that reflects reality. |
| Residency | Load on first use, stay resident | Keeps the hot path free of reload cost. Model lives on the pipeline worker thread, so it need not be `Send`. |

### 2.1 Model version: v3 over v2

v3 is the latest revision and costs **0.27 pp of English accuracy** (6.32 % vs 6.05 % WER,
Open ASR Leaderboard) in exchange for 25-language support at identical size and speed.
Keep the model id in config so dropping to English-only v2 is a one-line change.

### 2.2 Verified artifacts

Repo: `csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8` (HF, public, **no auth token**).
Base: `https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/main/`

| File | Bytes |
|---|---|
| `encoder.int8.onnx` | 652,184,281 |
| `decoder.int8.onnx` | 11,845,275 |
| `joiner.int8.onnx` | 6,355,277 |
| `tokens.txt` | 93,939 |
| **Total** | **670,478,772** (~670 MB / 639 MiB) |

Verified 2026-07-21: HTTP 200 anonymously, `Content-Length` present, `accept-ranges: bytes`
(so progress bars and resume both work). Model licence is **CC-BY-4.0** — re-read the HF
model card before shipping and put the attribution in the UI + docs.

### 2.3 Cost profile

| | |
|---|---|
| Disk | ~670 MB (opt-in download) |
| RAM resident | **~1.2 GB**, ~2 GB during inference (sherpa-onnx#2626) |
| Latency, 5 s clip, CPU-only | ~1.5–2 s |
| First-use model load | seconds — must be a visible state, not a freeze |

---

## 3. RISK — the binary-size gate (CP0 blocks everything)

`sherpa-onnx` statically links a native lib bundling ONNX Runtime. **Every user pays that
binary growth, including users who never enable local STT.** The 2026-07-15 pivot rejected
on-device STT specifically over footprint, so this is the decision that can kill the feature.

**CP0 measures the delta before any other work starts.** Rough expectation is tens of MB;
if it roughly doubles the shipped binary, escalate before continuing. Mitigations in
preference order:

1. Cargo feature `local-stt`, **off** in the default release build; ship a separate
   "Hark + Local" artifact. Keeps the mainline binary untouched.
2. Accept the growth if it is small enough to be uninteresting.
3. `dlopen` the native lib and download it with the model — genuinely opt-in, materially
   more complex, and a per-platform build matrix. Last resort.

Do not start Phase 2 until CP0 has a number and the user has seen it.

---

## 4. Architecture

### 4.1 New crate `hark-local-stt`

```
crates/hark-local-stt/
  src/lib.rs        # LocalEngine: load(dir, opts) + transcribe(&[f32]) -> Transcript
  src/model.rs      # ModelSpec (files, urls, sizes, sha256), status(), paths
  src/download.rs   # streaming resumable downloader + progress callback
  src/error.rs      # LocalSttError
```

**Samples, not WAV.** The cloud `SttProvider` trait takes encoded WAV bytes. A local engine
wants raw `f32` — which the worker already holds as `clip.samples_16k` *before* the WAV
encode at `worker.rs:145`. `LocalEngine` therefore takes `&[f32]` directly and stays off the
`SttProvider` trait entirely; no decode round-trip, and the cloud trait keeps its shape.

### 4.2 Config (`hark-config`)

```toml
[local_stt]
mode = "off"                              # off | fallback | primary
model = "parakeet-tdt-0.6b-v3-int8"
threads = 2                               # ONNX intra-op threads
fallback_after_ms = 6000                  # see §4.3
```

Additive, so existing config files load unchanged (`#[serde(default)]`, as every other
section does). `validate()` gains: `mode = "primary"` with no model present is a warning
surfaced in the UI, not a hard config error — the user may toggle before downloading.

### 4.3 Cloud timeout when fallback is armed — the load-bearing detail

`TOTAL_TIMEOUT_MS` is **15 s**. A naive fallback would be 15 s of cloud + 2 s of local = a
17-second dictation, which is worse than useless. **When `mode = "fallback"` and the model is
ready, the cloud request uses `fallback_after_ms` (default 6 s) instead.** Falling back has to
be fast or the feature is decorative.

### 4.4 Pipeline integration (`hark-pipeline`)

`transcribe_with_retry` (`worker.rs:294`) becomes the policy point:

- **fallback** — cloud (+ the existing single retry on timeout) → on *any* cloud error, try
  local. Auth failures fall back too: the user would rather have their words than a
  diagnostic. The dictation record still names which engine produced the text.
- **primary** — local only; cloud is never constructed.

`Worker` gains `local: Option<LocalEngine>` built lazily on first use. `dictate` takes
`&mut Worker` (it is single-threaded; no interior mutability needed).

**Pipeline must start without an STT key when `mode = "primary"`.** Today `provider_config`
(`lib.rs:81`) hard-requires a key and the pipeline refuses to start without one. That gate
becomes conditional, and the Get Started onboarding card
(`ui/settings/get_started.rs`) needs a local-primary path that does not nag for a key.

`DictationRecord.stt_provider` reports `"local"` / `"local (fallback)"` so history never
misattributes which engine produced a line.

### 4.5 Downloader

Mirrors the existing `Updater` (`hark-app/src/update.rs`): detached thread, `mpsc` to the UI,
drained in `poll()`, `ctx.request_repaint()` to wake the UI.

- **Stream to disk.** `hark-update` uses `.bytes()`, which buffers the whole body — at 652 MB
  that is a 652 MB RAM spike. Local model files must `copy` the response reader into the file.
- Per file: `Range:` resume onto `<file>.part`, rename into place when complete and verified.
- SHA-256 per file; a mismatch deletes the file and reports a real error.
- Throttle progress events to ~10/s. Do not repaint per chunk.
- Cancel via an `AtomicBool` checked in the copy loop; partial `.part` files survive for resume.
- Storage: `hark_config::default_data_dir()/models/<model-id>/`.

### 4.6 Settings UI (`ui/settings/local.rs`, new)

Mode radio (Off / Fallback / Primary) · model card with size + status · Download button →
progress bar (%, MB of MB, cancel) · Ready/Not-downloaded/Failed state · Delete-model button
to reclaim 670 MB · CC-BY-4.0 attribution line.

Per `hark-app/CLAUDE.md`: under ~300 lines, all tokens from `theme.rs`, status never conveyed
by color alone (icon + label).

---

## 5. Phases

### Foundation — CP0 (GATE, do first, standalone branch)
Add the dep behind a feature, download the model by hand, and **measure**: binary size delta,
cold model-load time, and real release-to-transcript latency on the existing fixture WAV for
a ~5 s clip, on both Windows and macOS. Report the numbers. **Stop and confirm before Phase 2.**

### Core
1. `hark-local-stt`: `ModelSpec` + status + streaming resumable downloader + sha256 (pure
   logic unit-tested; network paths behind an integration test).
2. `LocalEngine::load` / `transcribe(&[f32])`.
3. `hark-config`: `[local_stt]` section, defaults, validation, round-trip tests.
4. Pipeline: fallback policy, lazy engine, `fallback_after_ms`, key-optional start for
   primary mode, `stt_provider` labelling.

### Polish
5. Settings UI + progress/cancel/delete.
6. Onboarding path for local-primary (no key).
7. Model-load and fallback-in-progress states surfaced via `PipelineEvent`.

### Ship
8. `Docs/` page, CHANGELOG, version bump (**Minor** — new feature), CC-BY-4.0 attribution.
9. Validate on real Windows + macOS hardware: download, cancel, resume, offline dictation,
   airplane-mode fallback, RAM at idle.

---

## 6. Testing

Pure/unit: fallback policy per error class · resume offset math · progress math · model
status detection · config round-trip · sha256 mismatch handling.
Mocked: `MockLocalEngine` mirroring the existing `MockProvider` for worker-level fallback tests.
Integration (ignored by default, env-gated): real model load + transcribe a fixture WAV.

---

## 7. Lessons Learned / Gotchas

Route confirmed discoveries to LL-G via `/add-lesson` after implementation.

**Carried in from prior work (respect these):**

- **`.bytes()` buffers the entire body.** Fine for a small installer, a 652 MB RAM spike here.
  Stream large downloads to disk. (`hark-update` is the tempting but wrong template.)
- **`JoinHandle` joined in `Drop` deadlocks** (LL-G Rust HIGH): declare senders *before*
  handles so `recv()` loops actually end. Directly applicable to the downloader thread.
- **`CREATE_NO_WINDOW`** (LL-G Rust HIGH, standing): release is `windows_subsystem="windows"`.
  We shell out to nothing here, but any future extraction helper must set it.
- **sherpa-onnx build downloads a large native lib** — `tasks/plan-repo.md:225` flagged this
  when the dep was removed. It will surprise CI. Pin the version and budget the build time.
- **sherpa-onnx #3267:** `modified_beam_search` + hotwords hallucinates ~20 %. Use
  `greedy_search`; the dictionary phonetic pass covers biasing.

**Found during implementation (2026-07-21) — route these to LL-G:**

- **CP0 measured +27.7 MB** of binary for a static sherpa-onnx + ONNX Runtime
  link (0.3 MB probe → 28.0 MB, linux-x64, release, stripped). Hence the
  `local-engine` feature. `libonnxruntime.a` is 109 MB before dead-code
  elimination.
- **`OfflineRecognizer::create` returns `Option`, not `Result`** — there is no
  error detail to surface, so the wrapper supplies its own message naming the
  model directory.
- **A 200 response to a `Range` request means the server ignored the range.**
  Appending in that case silently corrupts the file; detect via status 206 and
  restart from zero.
- **`Accept-Encoding: identity` is required on resumable downloads**, or a
  compressed transfer makes `Content-Length` disagree with bytes written.
- **`Path::with_extension` is wrong for `.part` suffixes** —
  `encoder.int8.onnx` becomes `encoder.int8.part`, colliding across files. The
  suffix must be appended to the full `OsString`.
- **The shared 15 s STT client timeout kills any model download**; override per
  request (`hark-update` had already hit this for its installer).
- **Hugging Face exposes the LFS sha256 as `x-linked-etag`**, which is how the
  pinned hashes were obtained without downloading 670 MB. Files stored in git
  rather than LFS (e.g. `tokens.txt`) return a git SHA-1 instead, so they get
  size-only verification.
- **`hark-app` cannot link on Linux** (`-lxdo`, enigo's X11 dep), so no
  end-to-end binary size for the real app was obtainable on the dev machine.
  Pre-existing, unrelated to this feature, but it means CP0's number is the
  marginal delta from an isolated probe, not a before/after of the shipped exe.

**Still to verify on real hardware:**

- Confirm the ~1.2 GB resident figure on both platforms; if it is worse on Windows, the
  "stay resident" decision may need revisiting.
- Confirm CC-BY-4.0 on the v3 model card and that attribution text is correct.
- Watch for HF redirecting `resolve/main/*` to a CDN host — resume logic must follow
  cross-host redirects (the `WebFetch` tool refuses them; `reqwest` follows by default).
- Measure whether `threads = 2` is the right ONNX default; more threads may not help a
  ~5 s clip and will compete with the UI thread.

# Hark, Project Plan (plan-repo) v2: BYOK Cloud STT

**Planned:** 2026-07-15 (v2, replaces the v1 local-STT plan written earlier the same day)
**Pivot:** v1 committed to on-device STT (sherpa-onnx + Parakeet TDT 0.6B, ~1.1 GB of model assets). The user rejected that footprint on 2026-07-15 and chose **BYOK cloud transcription with multi-provider adapters**. Dictation history, stats, settings, and the dictionary remain strictly local. A small opt-in local fallback model is a later-phase option, not a Phase 1-5 commitment.
**Skill note (unchanged from v1):** the `plan-repo` skill's locked web infrastructure (Northflank/Cloudflare/Better Auth/Postgres/Redis) does not apply; Hark is a native Rust desktop app for Windows + macOS.

---

## 1. Project Requirements

| Question | Answer |
|---|---|
| What it does | System-wide push-to-talk voice dictation. Hold a key, speak, release; polished English text is injected at the cursor in any app. |
| Users | A single end user (the operator of the machine). Not multi-tenant, no accounts. |
| Scale | Personal / single-user tool. One process per machine. |
| UI | Native desktop: always-on tray daemon + an on-demand settings/history window. No web UI, no webview. |
| Data beyond "user accounts" | Local dictation history + lifetime stats (SQLite). Dictionary + voice presets (TOML config). BYOK API keys (OS keychain): one for STT (required), optionally one for cleanup. |
| Real-time features | None in the web sense. Hot path is audio capture (local) then one HTTPS transcription call then inject. The only latency budget is release-to-inject. |
| Payments | None. Users pay their own STT/LLM providers directly (BYOK). |
| External services | The user's chosen BYOK STT endpoint (required for dictation) and an optional BYOK LLM cleanup endpoint. Nothing else; no telemetry. |
| Hard constraints | Audio leaves the machine only to the user's chosen STT provider; everything else stays local (history, stats, dictionary, settings). English only. No JS toolchain, no webview. Single Rust process. Network required to dictate (opt-in small local fallback is a later-phase option). |
| Greenfield / integrate | Greenfield. Repo holds the starter template plus a `crates/hark-stt` skeleton from the abandoned v1 spike (to be repurposed; see §5). |

---

## 2. Infrastructure

**Still not applicable; this is a desktop app with no backend of ours.** Runtime dependencies:

- **Required:** the user's chosen STT provider HTTPS endpoint (OpenAI, Groq, or Deepgram in Phase 1).
- **Optional:** the user's BYOK LLM cleanup endpoint (OpenAI-compatible chat completions).

Distribution via signed/notarized native installers (macOS `.dmg`/`.app`, Windows `.msi`/`.exe`). No model files are bundled or downloaded; the installer shrinks from ~1.5 GB (v1) to tens of MB.

---

## 3. Stack (v2, corrected by 2026-07-15 research)

| Layer | Choice | Status |
|---|---|---|
| Language | **Rust** (single process; UI on main thread, pipeline on worker threads) | Locked |
| Audio capture | **`cpal`**, continuous 16 kHz mono ring buffer | Unchanged from v1 |
| Push-to-talk hotkey | **Native low-level hooks: CGEventTap (macOS) + `WH_KEYBOARD_LL` (Windows)** | Unchanged from v1 |
| STT | **BYOK cloud via an `SttProvider` trait.** Phase 1 adapters: (a) **`openai-compatible`**: multipart `POST /v1/audio/transcriptions`, covers OpenAI (`gpt-4o-transcribe`, `gpt-4o-mini-transcribe`, `whisper-1`) and Groq (`whisper-large-v3-turbo`) with one code path; (b) **`deepgram`**: `POST /v1/listen` REST, model nova-3, `keyterm` biasing | **NEW, replaces sherpa-onnx/Parakeet** |
| STT transport | **`reqwest` 0.13, `blocking` + `multipart` + `rustls-tls-webpki-roots` + `json`**, running on the dedicated pipeline worker thread. One long-lived `Client` (connection keep-alive + TLS resumption). No global tokio runtime | NEW |
| Upload audio format | **WAV via `hound` 3.5** (16 kHz mono, ~312 KB per 10 s clip). FLAC via pure-Rust `flacenc` is the later optimization if slow uplinks prove material | NEW |
| Dictionary, primary path | **Phonetic post-correction** (Metaphone/Soundex against the dictionary), provider-agnostic, runs locally after every transcription | Kept from v1, still primary |
| Dictionary, provider biasing | Map the dictionary per adapter: OpenAI/Groq `prompt` field (224-token cap on Whisper-family models); Deepgram `keyterm` (nova-3+, up to 500 tokens, unweighted) | NEW |
| Cleanup / voices | BYOK OpenAI-compatible chat endpoint; single low-temp call; Verbatim/Clean/Professional/Casual/Custom; shares the same reqwest blocking stack | Unchanged from v1 |
| Injection | Clipboard paste (stash, set, paste, restore); **`enigo`** keystroke fallback | Unchanged |
| Tray / window | **`tray-icon`** + **`eframe`/`egui`** | Unchanged |
| Storage | **`rusqlite`** (history + stats); TOML (settings + dictionary) | Unchanged |
| Key storage | **`keyring`** crate; separate keychain entries per provider (STT and cleanup may be the same provider or different) | Unchanged, now holds 1-2 keys |
| Local fallback (later phase, opt-in) | **`whisper-rs` 0.16 + whisper.cpp `tiny.en` (~75 MB download)**; leading candidate only, not a commitment | Deferred |

### Runner-ups / rejected
- **sherpa-onnx + Parakeet TDT 0.6B (the entire v1 STT stack)**: rejected 2026-07-15 for its ~1.1 GB footprint. Side benefit: the open hotword-biasing bug (sherpa-onnx #3267), the CoreML/DirectML feature-flag uncertainty, and model warmup discipline all drop out of the plan.
- **`ureq`** for transport: leaner (no tokio anywhere), but its built-in multipart (since 3.2.0) is explicitly marked unstable as of 3.3.0. Re-evaluate if reqwest weight ever hurts.
- **Opus upload encoding** (`opus` crate): only option that reintroduces a native C dependency; savings don't justify it. (`audiopus` is abandoned; never use it.)
- **Streaming/realtime STT APIs** (Deepgram live, ElevenLabs Scribe v2 Realtime, OpenAI Realtime): deferred. Push-to-talk captures a bounded clip before any request can fire, so batch-per-utterance is the natural fit; streaming-while-holding is a possible future latency optimization behind the same trait.
- **ElevenLabs Scribe, Mistral Voxtral, AssemblyAI adapters**: viable, deferred; add opportunistically once the trait is proven with the Phase 1 pair.

---

## 4. Research Findings (verified 2026-07-15)

Full citations: `.claude/agent-memory/explorer/hark_cloud_stt_providers.md` and `.claude/agent-memory/explorer/hark_cloud_stt_rust_stack.md`.

1. **OpenAI and Groq share the identical multipart `/v1/audio/transcriptions` contract** (Bearer auth; `file`/`model`/`prompt`/`response_format`/`language` fields). One adapter covers both, giving a cheap+fast tier (Groq ~$0.000667/min) and a quality tier (OpenAI gpt-4o-transcribe, $0.006/min) for free.
2. **Deepgram `keyterm` is the only first-class dictionary-biasing mechanism** among Phase 1 candidates (nova-3+, up to 500 tokens, no weighting). The legacy weighted `keywords` syntax is nova-2 only; the two are mutually exclusive by model generation.
3. **`reqwest::blocking` on the pipeline worker thread needs no tokio runtime**, so the LL-G HIGH rule about blocking IO on an executor is moot for the default path. Only a future streaming adapter would introduce tokio, and it must stay scoped to that adapter.
4. **The `deepgram` crate (0.10.0, 2026-05-12) is pre-1.0 and "Community"-branded** despite docs.rs saying "official"; it pins full tokio + reqwest + tokio-tungstenite. Verify provenance before pinning; for REST-only Phase 1 use, calling `/v1/listen` directly with reqwest is simpler than adopting the SDK.
5. **Groq bills a 10-second minimum per request**: every 2-5 s push-to-talk utterance is billed as 10 s. Still roughly 100x cheaper per utterance than OpenAI, but surface it honestly in any cost display.
6. **A 10 s 16 kHz mono WAV is ~312 KB**; `flacenc` (pure Rust) roughly halves that. On typical connections upload time is minor; treat FLAC as a measured optimization, not a default.
7. **No provider's marketing latency number is a release-to-inject measurement.** Groq's "216x realtime", Deepgram's "sub-300 ms", ElevenLabs' "150 ms" are not comparable; the Phase 1 spike measures real p50/p95 per provider from Rust.
8. **Several pricing figures came from aggregators**, not primary pricing pages. Cross-check before building any cost model into the UI.

---

## 5. Planned File Structure

Same Cargo workspace as v1; `hark-stt` is repurposed from "ONNX engine wrapper" to "provider adapter layer". No `models/` directory anywhere.

```
Hark/
├── Cargo.toml                  # workspace
├── crates/
│   ├── hark-app/               # binary: main-thread event loop (tray + window), worker orchestration, single-instance guard
│   ├── hark-hotkey/            # native PTT: CGEventTap (macOS), WH_KEYBOARD_LL (Windows); key-down/up edges
│   ├── hark-audio/             # cpal ring buffer, pre-roll + tail, silence trim, WAV encode for upload
│   ├── hark-stt/               # SttProvider trait + openai_compatible + deepgram adapters (reqwest blocking)
│   ├── hark-dictionary/        # phonetic post-correction + per-provider bias mapping (prompt / keyterm)
│   ├── hark-voice/             # voice presets (prompt templates) + BYOK cleanup adapter (OpenAI-compatible)
│   ├── hark-inject/            # clipboard paste path + enigo fallback
│   ├── hark-store/             # rusqlite (history + stats), retention pruning, TOML settings load/save
│   ├── hark-keychain/          # keyring wrapper for provider API keys
│   └── hark-ui/                # egui settings/history/stats window incl. provider config
├── config/                     # default config.toml, default dictionary
├── tasks/                      # plans (this file)
├── CLAUDE.md
└── README.md
```

**Repurpose note:** `crates/hark-stt` currently carries the v1 skeleton with a `sherpa-onnx = "1.13.4"` dependency whose build script auto-downloads a large native lib. The first checkpoint of the Phase 1 spike removes that dependency and rewrites the crate around the `SttProvider` trait. Do not run `cargo build` on the workspace before that checkpoint lands.

---

## 6. Planned CLAUDE.md Hierarchy

- **Root `CLAUDE.md`**: updated for the pivot (done alongside this plan): BYOK cloud STT identity, reqwest-blocking transport rule, provider gotchas.
- **`crates/hark-stt/CLAUDE.md`** (create when the crate is rewritten): the `SttProvider` contract, per-provider request shapes, key-handling rules (never log keys or audio URLs with query tokens), retry policy.
- **`.claude/rules/*.md`**: `rust.md` updated for the pivot (done); hotkey/inject path rules unchanged.

---

## 7. Design Guardrails Summary

Full file: `.claude/references/design-guardrails.md` (updated for the pivot).

- **UI:** egui immediate-mode; window on the macOS main thread; pipeline strictly off it.
- **Latency SLA:** release-to-inject = WAV encode (keep under ~10 ms) + one HTTPS POST + provider inference + phonetic post-correct + inject. One long-lived HTTP client for keep-alive and TLS resumption; at most one retry on timeout; history writes after injection.
- **Every dictation now shows the lightweight processing indicator** (it always hits the network); still never a modal.
- **Offline / error UX:** clearly distinguishable tray states for "no network", "provider rejected key", "provider error/timeout". Dictation fails fast and visibly, never silently.
- **Privacy surface:** honest in-UI disclosure that **audio is sent to the user's chosen STT provider** for every dictation, and text to the cleanup provider for non-Verbatim voices. History/stats/dictionary stay local. Keys only in the OS keychain.
- **Accessibility:** unchanged (keyboard-navigable, OS theme, no color-only status).

---

## 8. Phase-Based Development Plan

### Phase 1, Foundation: core loop, Verbatim only
- **Spike first (blocking):** prove the cloud path from Rust; spec: `tasks/2026-07-15-phase1-stt-spike.md` (rewritten for v2). Strip sherpa-onnx, build the `openai-compatible` adapter, POST a fixture WAV to Groq and OpenAI, measure release-to-inject-equivalent latency (p50/p95, N=20, warm client vs cold), then the Deepgram adapter + a `keyterm` smoke test.
- Native PTT hooks (CGEventTap / `WH_KEYBOARD_LL`) with separate key-down/up edges.
- `cpal` ring buffer with 200-300 ms pre-roll + ~150 ms tail; silence trim; WAV encode.
- Clipboard-paste injection (stash/set/paste/restore).
- Tray icon only; API key via keychain CLI or env var (settings UI is Phase 4). **Goal:** hold key, speak, release; raw provider text appears. Prove latency + hotkey reliability on real Windows and macOS.

### Phase 2, Dictionary
- Phonetic post-correction as the primary, provider-agnostic path (unchanged from v1's corrected design).
- Map the dictionary onto provider biasing: `prompt` for OpenAI/Groq (mind the 224-token Whisper-family cap), `keyterm` for Deepgram nova-3. Measure whether biasing adds lift over post-correction alone; keep it if it does.
- Dictionary lives in `config.toml` for now (UI editor is Phase 4).

### Phase 3, Voice layer + cleanup BYOK
- OpenAI-compatible chat adapter reusing the same blocking HTTP stack; `keyring` storage; Verbatim/Clean/Professional/Casual/Custom; tray voice selector; Clean default.
- Provider profiles: STT and cleanup may share one provider+key (e.g. both OpenAI) or differ (Groq STT + OpenAI cleanup); config models this explicitly.
- One low-temp call; dictionary terms passed as "leave these untouched"; length-gate lets send-ready short utterances skip cleanup (cleanup only; STT always runs).

### Phase 4, Settings/history UI + storage
- `rusqlite` schema (`entries`, `stats`); retention pruning (default: last 1,000 entries or 90 days); lifetime counters survive history clears; separate reset-stats control.
- egui window: settings form incl. **provider config** (STT provider picker, model picker, base-URL override for compatible endpoints, key entry straight to keychain, test-connection button), dictionary editor, history list, stats panel, capture toggle.
- **First-run onboarding:** the app cannot dictate without an STT key; a clean guided first-run (pick provider, paste key, test, dictate) is a product requirement, not polish.

### Phase 5, Polish / Ship
- Offline/error UX hardening (tray states, retry-once policy, actionable error toasts).
- Packaging + notarization (macOS) and signing (Windows); first-run permission flow (macOS Accessibility + microphone); launch-at-login; single-instance guard; `CREATE_NO_WINDOW` on any Windows console child.
- **Stretch / later:** opt-in local fallback (`whisper-rs` + `tiny.en`, ~75 MB download); optional Deepgram streaming adapter behind the same trait if measured latency justifies it.

---

## 9. Configuration & Secrets

| Item | Location |
|---|---|
| `config.toml` (hotkey binding, default voice, **STT provider + model + optional base URL**, cleanup provider + model, dictionary, capture toggle, retention cap) | OS config dir (`~/Library/Application Support/Hark/` on macOS, `%APPDATA%\Hark\` on Windows) |
| `hark.db` (history + stats) | OS data dir (not next to the binary) |
| **STT provider API key** (required) and cleanup key (optional; may be the same entry) | OS keychain (`keyring` crate), one entry per provider; never in `config.toml`, never logged |
| Model files | **None.** (Later-phase local fallback would download to the OS data dir, opt-in.) |

---

## 10. Tools Required

Core: `rustup`, `cargo`, `cargo clippy`, `cargo fmt`, `cargo nextest` (or `cargo test`). Cross-compile targets for both OSes. Packaging/signing: `cargo-dist` or `cargo-bundle`, macOS `codesign`/`notarytool`, Windows signing + WiX/MSI. **Dropped from v1:** ONNX Runtime, model fetch scripts, model assets. **New:** provider API keys (Groq/OpenAI/Deepgram) for spike and integration testing; keep them in env vars or the keychain, never in the repo.

---

## 11. Lessons Learned / Gotchas

**Pre-seeded from LL-G (as of 2026-07-15):**
- [ ] **Rust HIGH, GUI-subsystem console child** (`kb/rust/gui-subsystem-console-child-window.md`): any console child of the tray binary needs `CREATE_NO_WINDOW` (0x0800_0000).
- [ ] **Rust HIGH, blocking IO on tokio** (`kb/rust/blocking-io-on-tokio.md`): moot for the default reqwest-blocking-on-worker-thread design; becomes live the moment a streaming adapter introduces tokio. Keep any runtime scoped to that adapter.
- [ ] **SQLite MEDIUM, upsert-by-name collisions** (`kb/sqlite/upsert-by-name-collision.md`): key `stats` on stable identifiers.

**Pre-seeded from 2026-07-15 cloud-STT research (full cites in agent-memory):**
- [ ] **Groq bills a 10 s minimum per transcription request**; every short utterance costs as 10 s.
- [ ] **Deepgram `keyterm` (nova-3+, unweighted) vs legacy `keywords` (nova-2, weighted) are mutually exclusive by model generation**; don't assume weighting on nova-3.
- [ ] **`deepgram` crate 0.10 is pre-1.0, "Community"-branded, and drags in full tokio**; for REST-only use, call `/v1/listen` directly with reqwest.
- [ ] **`ureq` multipart is explicitly unstable as of 3.3.0**; reqwest blocking is the safe multipart choice.
- [ ] **`hound` (3.5.1) is stale since 2023 but WAV is frozen**; acceptable. `flacenc` is the pure-Rust upgrade path.
- [ ] **Marketing latency numbers are not release-to-inject numbers**; only trust the spike's measurements.
- [ ] **Some pricing figures came from aggregators**; cross-check on provider pricing pages before displaying costs.
- [ ] **The app is dead without an STT key**: first-run onboarding is a Phase 4 product requirement.
- [ ] **`crates/hark-stt` still declares `sherpa-onnx`** (auto-downloads a large native lib at build time); remove it in spike checkpoint 0 before any workspace build.

**Discovered during implementation (route new items to LL-G via `/add-lesson`):**
- [ ] _(add as found)_

# Hark — Project Plan (plan-repo)

**Planned:** 2026-07-15
**Skill note:** The `plan-repo` skill assumes a containerized full-stack web app on locked Northflank/Cloudflare/Better Auth/Postgres/Redis/Resend infrastructure. **Hark is none of that** — it is an offline, single-user, local-first **Rust desktop application** for Windows + macOS. The locked web infrastructure and the six-wave stack-research machinery **do not apply** and were deliberately skipped (confirmed with the user). This plan captures the already-decided stack, corrects it against current-as-of-2026-07-15 research, and lays out the phased build.

---

## 1. Project Requirements

| Question | Answer |
|---|---|
| What it does | System-wide push-to-talk voice dictation. Hold a key, speak, release; polished English text is injected at the cursor in any app. |
| Users | A single end user (the operator of the machine). Not multi-tenant, no accounts. |
| Scale | Personal / single-user tool. One process per machine. |
| UI | Native desktop: always-on tray daemon + an on-demand settings/history window. No web UI, no webview. |
| Data beyond "user accounts" | Local dictation history + lifetime stats (SQLite). Dictionary + voice presets (TOML config). Optional BYOK LLM API key (OS keychain). |
| Real-time features | None in the web sense. Hot path is a local audio→STT→inject pipeline; the only latency budget is release-to-inject. |
| Payments | None. |
| External services | Exactly one, optional: the user's own BYOK LLM endpoint (OpenAI-compatible chat completions). Used only for non-Verbatim voices. |
| Hard constraints | Local-first and offline for transcription (non-negotiable). English only. No JS toolchain, no webview. Single Rust process. |
| Greenfield / integrate | Greenfield. Repo currently holds only the Claude Code starter template. |

---

## 2. Infrastructure

**Not applicable — this is a desktop app.** The template's locked stack (Northflank containers, Cloudflare CDN/R2/WAF, Better Auth, Postgres, Redis, Resend, Northflank Cron) is **not used**. There is no server, no database service, no auth service, no CDN, no object storage, no cron.

The only runtime dependencies are:
- **On-device:** ONNX Runtime + the Parakeet TDT 0.6B model files (bundled or downloaded on first run).
- **Off-device (optional):** the user's chosen BYOK LLM HTTPS endpoint.

Distribution is via signed/notarized native installers (macOS `.dmg`/`.app`, Windows `.msi`/`.exe`), not a hosting platform.

---

## 3. Stack (Decided in the design doc, corrected by research)

| Layer | Choice | Status |
|---|---|---|
| Language | **Rust** (single process; UI on main thread, pipeline on worker threads) | Locked |
| Audio capture | **`cpal`** — continuous 16 kHz mono ring buffer | Confirmed current |
| Push-to-talk hotkey | **Native low-level hooks: CGEventTap (macOS) + `WH_KEYBOARD_LL` (Windows)** for reliable key-down/key-up edges | **CORRECTED** — see §4 |
| STT runtime | **sherpa-onnx** via the **official `sherpa-onnx` crate (v1.13.4+)** | **CORRECTED** — `sherpa-rs` is deprecated |
| STT model | **Parakeet TDT 0.6B v2 (English)** ONNX, `model-type=nemo_transducer` (encoder/decoder/joiner + tokens.txt) | Confirmed available |
| Dictionary — decode biasing | sherpa-onnx hotwords via `modified_beam_search` — **experimental opt-in only** | **CORRECTED** — see §4 (open bug #3267) |
| Dictionary — post-correction | Phonetic (Metaphone/Soundex) match against dictionary long tail — **promoted to the reliable primary path** until #3267 is fixed | **CORRECTED** |
| Cleanup / voices | BYOK OpenAI-compatible chat endpoint; single low-temp call; Verbatim/Clean/Professional/Casual/Custom | Locked |
| Injection | Clipboard paste (stash → set → paste → restore); **`enigo`** keystroke fallback | Confirmed current |
| Tray | **`tray-icon`** | Confirmed current |
| Window UI | **`eframe`/`egui`** (immediate-mode, compiled in, no webview) | Confirmed current |
| Storage | **`rusqlite`** (history + stats); **TOML** (settings + dictionary) | Confirmed current |
| Key storage | **`keyring`** crate → macOS Keychain / Windows Credential Manager | Confirmed current |
| HTTP client | `reqwest` (or `ureq` for a smaller, blocking client) against the BYOK endpoint | To decide in Phase 3 |

### Runner-ups / rejected
- **`global-hotkey` crate** for PTT — rejected as the *primary* mechanism (see §4). May remain as an optional fallback for non-held combo bindings.
- **`sherpa-rs` crate** — rejected; deprecated and archived 2026-06-06.
- **Slint** instead of egui — noted in the doc as the prettier alternative; deferred, not chosen. egui wins on minimalism and zero-toolchain.

---

## 4. Research Findings (as of 2026-07-15)

Full citations live in `.claude/agent-memory/explorer/hark_stt_stack_risk.md`. The three findings that changed the plan:

1. **`sherpa-rs` is deprecated (archived 2026-06-06).** Use the official `sherpa-onnx` crate on crates.io (**v1.13.4**, released 2026-07-08), which exposes `OfflineRecognizer` / `OfflineRecognizerConfig` / `OfflineTransducerModelConfig` and plumbs hotwords through `hotwords_file` + `hotwords_score`. **Caveat:** there is *no dedicated Rust hotwords example* in the official repo — hotwords are plumbed but undemonstrated in Rust. Budget a Phase-1 spike to prove them end-to-end.

2. **Hotword biasing on Parakeet TDT is a live risk (sherpa-onnx issue #3267, open since 2026-03-07).** Contextual biasing for NeMo TDT transducers only exists via `modified_beam_search` (added in PR #3077, 2026-02-05). That decoder **hallucinates or returns empty text ~20% of the time, non-deterministically**, on Parakeet TDT. `greedy_search` is reliable but ignores hotwords. **Consequence:** the design doc's "decode-time biasing is primary, phonetic is fallback" is inverted for now. Ship with `greedy_search` + the **phonetic post-correction pass as the dependable dictionary path**, and gate `modified_beam_search` hotwords behind an "experimental" toggle. Re-check #3267 before finalizing Phase 2.

3. **Use native low-level key hooks for push-to-talk, not `global-hotkey`.** The `global-hotkey` crate (tauri-apps, v0.8.0) does not affirmatively support reliable held-key press/release edges and has release-ordering bugs. A comparable shipped hold-to-talk dictation app (Open-Less) independently chose **CGEventTap (macOS)** and **`WH_KEYBOARD_LL` (Windows)** directly. Plan Phase 1 around native hooks. (Open-Less also avoided sherpa-onnx/Parakeet, a soft signal this exact ASR stack is not yet proven in a shipped consumer app — reinforcing finding #2.)

Additional (verify, don't assume):
- **CoreML (macOS) / DirectML (Windows) cargo feature flags** for the `sherpa-onnx` crate are under-documented. Read the crate's `Cargo.toml`/build script directly before wiring GPU-accelerated inference; do not assume feature names from docs.

---

## 5. Planned File Structure

A Cargo workspace keeps the hot path, UI, and pipeline as separable crates (the doc's "data, not code" principle and the 500-line file rule both favor this). A single binary is still produced.

```
Hark/
├── Cargo.toml                  # workspace
├── crates/
│   ├── hark-app/               # binary: main-thread event loop (tray + window), worker orchestration, single-instance guard
│   │   └── src/main.rs
│   ├── hark-hotkey/            # native PTT: CGEventTap (macOS), WH_KEYBOARD_LL (Windows); key-down/up edges
│   ├── hark-audio/             # cpal ring buffer, pre-roll + tail, silence trim
│   ├── hark-stt/               # sherpa-onnx wrapper, warm model, batch decode, hotwords (experimental)
│   ├── hark-dictionary/        # hotword list builder + phonetic post-correction
│   ├── hark-voice/             # voice presets (prompt templates) + BYOK adapter (OpenAI-compatible)
│   ├── hark-inject/            # clipboard paste path + enigo fallback
│   ├── hark-store/             # rusqlite (history + stats), retention pruning, TOML settings load/save
│   ├── hark-keychain/          # keyring wrapper for BYOK key
│   └── hark-ui/                # egui settings/history/stats window
├── models/                     # Parakeet TDT 0.6B ONNX (gitignored; downloaded or bundled at package time)
├── config/                     # default config.toml, default dictionary
├── tasks/                      # plans (this file)
├── CLAUDE.md
└── README.md
```

Adjust crate granularity down if it proves over-split; three or four crates (app / pipeline / ui) is an acceptable minimum.

---

## 6. Planned CLAUDE.md Hierarchy

Create these only once the folders exist:

- **Root `CLAUDE.md`** — Rust workspace conventions, the hot-path/UI-thread rule, the latency budget, links to design-guardrails. Strip the web-infra rules that don't apply.
- **`crates/hark-ui/CLAUDE.md`** — egui/immediate-mode conventions, main-thread-only rule, accessibility expectations for the settings window.
- **`crates/hark-stt/CLAUDE.md`** — sherpa-onnx binding rules, model warmup discipline, the greedy vs modified_beam_search decision and the #3267 caveat.
- **`.claude/rules/*.md`** with `paths:` frontmatter — e.g. a `paths: crates/hark-hotkey/**` rule documenting the platform-hook gotchas, and a `paths: crates/hark-inject/**` rule for injection edge cases.

The template's `commit-changelog.md` rule stays (CHANGELOG + `package.json`/`Cargo.toml` version bump discipline still applies).

---

## 7. Design Guardrails Summary

Full file: `.claude/references/design-guardrails.md` (desktop/egui-adapted, not a web component library).

- **UI:** egui immediate-mode. Window on the macOS main thread; pipeline strictly off it. Keep tray-menu logic trivial. Virtualize the history list with `ScrollArea`.
- **Latency SLA:** release-to-inject is the product. Model warm + warmup inference at launch. Verbatim and skip-eligible short utterances never touch the network. History writes happen after injection.
- **Accessibility (desktop):** keyboard-navigable settings window, respects OS light/dark + reduced-motion, legible default type scale, visible focus, no color-only status.
- **Privacy surface:** honest in-UI disclosure that non-Verbatim voices send text to the BYOK provider; the selected model always visible; key only ever in the keychain.

---

## 8. Phase-Based Development Plan

Adapted from the doc's five phases, with the research corrections folded in.

### Phase 1 — Foundation: core loop, Verbatim only
- **Spike first (blocking):** prove the `sherpa-onnx` crate v1.13.4+ loads Parakeet TDT 0.6B v2 and batch-decodes a wav end-to-end; separately prove hotwords are reachable from Rust (expect friction — no official Rust example).
- Native PTT hooks (CGEventTap / `WH_KEYBOARD_LL`) with separate key-down/up edges.
- `cpal` ring buffer with 200–300 ms pre-roll + ~150 ms tail; optional silence trim.
- Warm model + launch warmup inference.
- Clipboard-paste injection (stash/set/paste/restore).
- Tray icon only. **Goal:** hold key → speak → release → raw text appears. Prove latency + hotkey reliability on **real macOS and Windows**.

### Phase 2 — Dictionary
- Build the hotword bias list; wire the **phonetic post-correction pass as the primary, reliable path**.
- Gate `modified_beam_search` decode-biasing behind an experimental toggle; **re-check issue #3267** before relying on it. If still open, ship phonetic-only and document why.
- Dictionary lives in `config.toml` for now (UI editor is Phase 4).

### Phase 3 — Voice layer + BYOK
- OpenAI-compatible adapter; `keyring` key storage; Verbatim/Clean/Professional/Casual/Custom templates; tray voice selector; **Clean default**.
- One low-temp call; pass dictionary terms into the cleanup prompt with "leave these untouched"; length-gate to let send-ready short utterances skip cleanup.
- No async-executor blocking during the HTTP call (LL-G Rust HIGH).

### Phase 4 — Settings/history UI + storage
- `rusqlite` schema (`entries`, `stats`); retention pruning (default: last 1,000 entries or 90 days); lifetime counters that **survive history clears**; separate reset-stats control.
- egui window: settings form (incl. dictionary editor), history list (copy/delete/clear-all with confirm), stats panel, capture toggle.

### Phase 5 — Polish / Ship
- Processing indicator; packaging + **notarization** (macOS) and signing (Windows); first-run permission flow (macOS Accessibility + microphone); launch-at-login; single-instance guard; `CREATE_NO_WINDOW` on any Windows console child (LL-G Rust HIGH).

---

## 9. Configuration & Secrets (no web env vars)

Desktop app: settings in a TOML file under the OS config dir; DB under the OS data dir; secret in the keychain. No `.env`, no Postgres/Redis/R2/Resend variables.

| Item | Location |
|---|---|
| `config.toml` (hotkey binding, default voice, BYOK provider + model, dictionary, capture toggle, retention cap) | OS config dir (e.g. `~/Library/Application Support/Hark/` on macOS, `%APPDATA%\Hark\` on Windows) |
| `hark.db` (history + stats) | OS **data** dir (not next to the binary) |
| BYOK API key | OS keychain (`keyring` crate) — never in `config.toml` |
| Parakeet TDT model files | `models/` — bundled at package time or downloaded on first run |

---

## 10. Tools Required

Full entries in `.claude/references/tools.md`. Core: `rustup`, `cargo`, `cargo clippy`, `cargo fmt`, `cargo nextest` (or `cargo test`). Cross-compile targets for both OSes. Packaging/signing: `cargo-dist` or `cargo-bundle`, macOS `codesign`/`notarytool`, Windows signing + WiX/MSI. ONNX Runtime + model assets.

---

## 11. Lessons Learned / Gotchas

**Pre-seeded from LL-G (as of 2026-07-15):**
- [ ] **Rust HIGH — GUI-subsystem console child spawns visible console windows** (`kb/rust/gui-subsystem-console-child-window.md`). Hark's Windows tray binary has no console; any console child (signing tools, `taskkill`, launch-at-login setup) must set `CREATE_NO_WINDOW` (0x0800_0000) or a console flashes and steals focus.
- [ ] **Rust HIGH — Blocking `std::fs`/sync calls on a Tokio runtime starve the executor** (`kb/rust/blocking-io-on-tokio.md`). If the BYOK HTTP call uses async, keep blocking IO off the executor (`tokio::fs` / `spawn_blocking`), or use a blocking HTTP client on a worker thread.
- [ ] **SQLite MEDIUM — Upsert by display name silently merges records** (`kb/sqlite/upsert-by-name-collision.md`). Key the `stats` table on stable identifiers, not human-readable labels.

**Pre-seeded from this session's research (full cites in agent-memory):**
- [ ] **`sherpa-rs` is dead (archived 2026-06-06)** — build on the official `sherpa-onnx` crate v1.13.4+.
- [ ] **Parakeet TDT hotword biasing needs `modified_beam_search`, which has an open ~20% hallucination/empty-output bug (sherpa-onnx #3267, open since 2026-03-07).** Default to `greedy_search` + phonetic post-correction; treat decode-time biasing as experimental. Re-check #3267 before Phase 2 finalization.
- [ ] **No working Rust example of sherpa-onnx hotwords end-to-end** — spike it in Phase 1, don't assume C++/Python parity.
- [ ] **Verify CoreML / DirectML cargo feature flags in the `sherpa-onnx` crate source** before writing GPU-accel build config.
- [ ] **Use native key hooks (CGEventTap / WH_KEYBOARD_LL) for PTT**, not `global-hotkey` — held key-down/up edges are unreliable through that crate.

**Discovered during implementation (route new items to LL-G via `/add-lesson`):**
- [ ] _(add as found)_
```

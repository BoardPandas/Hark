# Handoff: Phase 4 planning session

**Written:** 2026-07-16, end of the Phase 3 execution session. **Load this as the sole starting context for the Phase 4 planning session.** Plan in that session; execute in a later one (project rule).

## 1. Where the repo stands

- `main` @ `febcb5c` (pushed to origin), v0.9.1, **236 workspace tests green**, fmt + clippy `-D warnings` clean.
- Phase 1 (BYOK cloud STT), Phase 2 (dictionary correction), and Phase 3 CP0-CP4 (voice cleanup layer) are code complete. Phase 2 was user-validated on real Windows hardware; **nothing has ever been validated on macOS** (no Mac hardware yet).
- Phase 3 commits: CP0 `0ce463c`, spec update `c8de534`, CP1 `c8de8c4`, CP2 `3cf697b`, CP3 `b42a305`, CP4 `5e94d47` (v0.9.0), lessons `febcb5c`.
- Crates: hark-audio, hark-cli, hark-config, hark-dictionary, hark-hotkey, hark-inject, hark-keychain, hark-pipeline, hark-stt, hark-voice.

## 2. What Phase 3 delivered (one paragraph)

After STT and dictionary pass 1, an optional low-temp chat-completions call rewrites the transcript in a configured voice (verbatim/clean/professional/casual/custom) before dictionary pass 2 and injection. Word-count gate (default: skip below 5 words), fail-open at every layer (a dictation is never lost to the optional feature), no retry, 10 s per-request timeout, provider inheritance (openai/groq STT reuses kind + base URL + already-resolved key), `hark-cli --voice` override. Full spec + lessons: `tasks/2026-07-16-phase3-voices.md`.

## 3. What Phase 3 explicitly deferred INTO Phase 4

**User decision (2026-07-16): BYOK key entry happens only in the Phase 4 UI paste field.** No terminal/env key handling. Consequences the Phase 4 plan must absorb:

1. **Key paste field writes the OS keychain**: service `"hark"`, account = provider label (`deepgram` / `openai` / `groq` / `openai-compatible`), plus the `voice.provider.key_account` override slot. **Gap: `hark-keychain` is currently read-only** (`resolve_key`, `resolve_key_for`); Phase 4 must add a store/delete API (`keyring::Entry::set_password`).
2. **The CP0 cleanup model spike still needs a live run** to pin chat model defaults empirically (currently provisional research pins: `gpt-5-nano` at `reasoning_effort="minimal"`, `llama-3.1-8b-instant` at temp 0.2). Harness is ready: `cargo run --example cleanup_spike -p hark-voice` (reads `OPENAI_API_KEY`/`GROQ_API_KEY` env). Decide in planning how it runs post-UI-field (adapt spike to keychain, or fold its verifications into the UI test-connection button + CP5).
3. **Phase 3 CP5 (interactive voice gate on real Windows hardware)** is queued behind a resolvable cleanup key. Checklist lives in the Phase 3 spec §3 CP5. Note: it can run without the UI field if the user's STT provider is openai/groq (inherit path reuses the STT key).

## 4. Phase 4 scope per the master plan (`tasks/plan-repo.md` §8)

> Settings/history UI + storage
- `rusqlite` schema (`entries`, `stats`); retention pruning (default last 1,000 entries or 90 days); lifetime counters survive history clears; separate reset-stats control.
- egui window: settings form incl. provider config (STT provider picker, model picker, base-URL override, **key entry straight to keychain**, test-connection button), dictionary editor, history list, stats panel, capture toggle.
- First-run onboarding (pick provider, paste key, test, dictate) is a **product requirement**, not polish: the app is dead without an STT key.
- Tray icon work also lands here (Phase 3 explicitly moved all tray/egui to Phase 4). Voice picker UI (the `[voice]` config + CLI flag exist; the UI surfaces them). History/stats of cleanup results were also deferred to Phase 4 storage.

## 5. Load-bearing constraints for the plan

- **THE threading rule:** all UI on the main thread (macOS hard requirement). `hark_pipeline::run(&Settings, api_key) -> PipelineHandle` was designed to be called from the UI event loop; capture/hook/worker live on their own threads. Settings changes mean drop the handle and `run` again; plan the restart UX around that.
- **Stack (locked):** `tray-icon` + `eframe`/`egui` native, no webview; `rusqlite` for history/stats in the OS data dir; TOML config additive-only so far (the first breaking config change triggers BP `versioned-config-migration-backup`).
- **Latency is the product:** history/stats writes happen after injection, off the hot path.
- **Hygiene:** never log keys, transcripts, prompts, or terms (counts/millis/labels only); no `Debug` on key-bearing structs; files under ~500 lines; every commit updates CHANGELOG.md + bumps package.json (`.claude/rules/commit-changelog.md`).
- **Windows tray binary has no console:** any console child needs `CREATE_NO_WINDOW` (LL-G HIGH `kb/rust/gui-subsystem-console-child-window.md`).
- **Parallel-enums precedent:** config-side serde enums map to crate-side enums in the pipeline (ProviderKind, VoiceName/Voice). Keep it for any new UI-facing enums.

## 6. Read before planning

1. `tasks/plan-repo.md` §8 Phase 4, §9 config/secrets table, §11 gotchas.
2. `tasks/2026-07-16-phase3-voices.md` (esp. §2.4 provisional defaults, CP5 checklist, Lessons Learned).
3. `.claude/references/design-guardrails.md` (UI/latency/accessibility SLA; it was written for exactly this phase).
4. `.claude/agent-memory/explorer/hark_llm_cleanup_chat_models.md`, `hark_cloud_stt_providers.md` (citations behind the pins).
5. RULE 1 / RULE 3: fetch LL-G and BP indexes; egui/eframe/tray-icon/rusqlite entries if any exist. **Verify current crate versions with WebSearch as of the planning date** (egui releases move fast; do not trust cached knowledge or Phase 1 versions).

## 7. Open questions the planning session should put to the user

1. Window-first or tray-first? (Master plan implies both; sequencing is open.)
2. First-run onboarding scope for a single-user app: full wizard vs a settings window that opens on missing key.
3. History retention defaults (1,000 entries / 90 days) confirm or adjust; where cleanup results appear in history (raw vs cleaned vs both).
4. How the deferred spike runs once the key field exists (keychain-reading spike vs test-connection button + CP5 subsumption).
5. Whether Phase 4 includes the `hark-app` binary rename (master plan: hark-cli's `main` is replaced by the tray/egui binary calling the same `hark_pipeline::run`).

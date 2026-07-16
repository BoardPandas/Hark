# Handoff: Phase 4 execution, CP0 + CP1 done, CP2 next

**Date:** 2026-07-16. Load this (plus the spec) as the starting context for the next session.
**Spec:** `tasks/2026-07-16-phase4-ui-storage.md` (committed; §6 now carries CP0/CP1 implementation lessons).

## Repo state

- `main` @ v0.9.5, 264 tests green, fmt + clippy `-D warnings` clean.
- Commits this session: `0784303` (CP0, 0.9.3), `7035a3d` (CP1, 0.9.4), plus this wrap-up commit (0.9.5).
- **Toolchain: stable 1.97.1** (updated from 1.94.0 this session; workspace `rust-version = "1.97"`). Reason: libsqlite3-sys 0.38.1 build script uses `cfg_select!`. See spec §6.

## What landed

- **CP0 `hark-store`:** WAL SQLite, embedded immutable migrations (`PRAGMA user_version`), entries + fixed-id stats tables, `record` (capture flag: content optional, counters always tick), paged `entries` with literal-LIKE search, `delete_entry`, `clear_entries` / `reset_stats` (independent), `prune` (age strict-boundary + newest-N), 15 tests. `Store::open` creates parent dirs; `open_in_memory` for tests.
- **CP1 `hark-keychain`:** `store_key` (trims; rejects empty pre-backend), `delete_key` (NoEntry = success), `key_status` -> `Stored | Missing | Backend(detail)`; pure outcome-mapping functions keep tests off the real keyring.
- **CP1 `hark-config`:** `version = 1` stamp (`CONFIG_VERSION`), `[history]` (capture / max_entries / max_age_days, validated >= 1), full-model `Serialize` (Options omitted when unset), `Settings::save` (validate, temp + rename; replace-on-Windows verified), `to_toml`, `default_data_dir()`. Legacy `bias_terms` upgrades to `terms` on next save.

## Next: CP2 (hark-app shell + design system, Minor bump)

Spec §3.1, §3.2, §3.10, §3.11, checkpoint list §4. First execution step per spec: compile a minimal eframe 0.35 `App` impl (trait: `ui(&mut self, ui, frame)` required; `logic()` defaulted) before building on it.

**Font assets: decision made (user, 2026-07-16, end of the CP0/CP1 session).** The user approved option 1: download the four static TTFs (Inter Regular/Medium/SemiBold from the rsms/inter official release; JetBrains Mono Regular from the JetBrains/JetBrainsMono official release; both SIL OFL 1.1) into `crates/hark-app/assets/` at the start of the CP2 session and commit them as embedded assets. Verify checksums/sizes look sane and the OFL license files ride along in the assets directory.

**Also re-verify at CP2 start (spec §3.9):** eframe/egui 0.35 still current; egui-phosphor 0.35-compat status (was pinned to egui 0.34 as of 2026-07-16; fallback = curated ~12-glyph set or a thin dep-patch).

**PipelineEvent plumbing** (part of CP2, not started): `hark_pipeline::run(settings, api_key)` gains an `events: Sender<PipelineEvent>` parameter; worker sends are non-blocking and ignore full/disconnected channels. hark-cli (alive until CP7) passes a channel and may drop the receiver. `DictationRecord` (raw, final, labels, timings incl. `audio_ms`) maps to `hark_store::NewDictation` in hark-app at CP4; hark-store stays dependency-free of hark-pipeline.

## Deferred / open

- LL-G contribution for the libsqlite3-sys MSRV lesson (`/add-lesson`), plus the spec's pre-seeded candidates once confirmed at CP2 (eframe 0.34/0.35 trait split; companion-crate lag). Batch them when CP2 confirms the egui ones.
- KB-check note: `versioned-config-migration-backup` and `no-test-touches-real-keyring` are cited in the spec as KB entries but exist in neither LL-G nor BP index (verified 2026-07-16); they are local conventions until promoted.
- Tasks #3-#8 in the session task list mirror CP2-CP7.

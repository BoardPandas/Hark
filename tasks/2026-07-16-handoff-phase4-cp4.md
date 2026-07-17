# Handoff: Phase 4 execution, CP3 done, CP4 next

**Date:** 2026-07-16. Load this (plus the spec) as the starting context for the next session.
**Spec:** `tasks/2026-07-16-phase4-ui-storage.md` (§6 now carries the CP3 lessons).

## Repo state

- `main` @ `fa41063` (v0.11.0, pushed to origin), 296 tests green, fmt + clippy `-D warnings` clean.
- **CP3 delivered:** full settings form (progressive disclosure holds: provider row, key section, Test connection, hotkey, voice visible; Model & endpoint / Cleanup provider / Behavior / History & privacy collapsed), keychain key section with confirm-guarded Remove, background-thread test connection (fixture WAV + tiny chat call, inline persistent results), save = validate -> atomic persist -> pipeline restart, Get Started onboarding card, dictionary editor, Light/Dark/System theme radio (persists via egui memory).
- Module map: `ui/settings/{mod,form,cleanup,keys,test,get_started}.rs`, `ui/dictionary.rs`, `ui/widgets.rs` (the reusable `Confirm` modal). `crates/hark-app/CLAUDE.md` still carries the crate conventions; read it first.
- New cross-crate surface: `hark_pipeline::provider_config` is pub; `hark_stt::fixture::SPIKE_WAV` embeds the test clip.

## Load-bearing CP3 discoveries (full list in spec §6)

- A stored key restarts the pipeline immediately (keys resolve only at `run`); `KeySection::show` returns a keychain-changed signal.
- `theme::apply` re-applies the current `ThemePreference` instead of forcing System, or every launch clobbers the persisted radio choice (now LL-G `kb/rust/eframe-set-theme-clobbers-persisted-preference.md`, with the CollapsingHeader one-frame `open(Some(true))` latch as a second new entry).
- The test fixture is ~10 s (reused verified `spike_clip.wav`), not the planned ~1 s; for Groq that is exactly the disclosed billing minimum.
- Get Started's `active` flag latches at startup and clears only on dismiss or first injection; a passing onboarding test auto-runs save-restart so the success copy is true.
- `PipelineController` now counts injections (`injected_count`), which is also the CP4 hook point for forwarding `Injected` records.

## Next: CP4 (history + stats, Minor bump to 0.12.0)

Spec §3.3, §3.7, §3.11, checkpoint list §4. The big pieces:

- **Storage thread** owned by hark-app: consumes `Injected(DictationRecord)` events, writes history + stats via hark-store, runs retention pruning after each insert, calls `ctx.request_repaint()` after a write. Read `crates/hark-store/src/lib.rs` first for the CP0 API (insert/query/clear/reset, WAL, two-connection writer/reader pattern, `default_data_dir()` for the DB path). Respect `history.capture = false` (no entries rows; stats still tick). Verify a DB write never precedes injection (it holds by construction: the event fires post-inject).
- The current pump (`spawn_repaint_pump`) forwards events to the UI only; CP4 needs the record to reach the storage thread too (tee in the pump, or a second channel) without the UI channel ever blocking dictation.
- **History panel:** `ScrollArea::show_rows` over paged reader queries, newest first under day headers (Today / Yesterday / date); toolbar (search-as-you-type over raw + final, entry count, Clear all as quiet danger button); rows = two-line final text, caption (relative time · voice · model), always-visible copy/delete; copy affirms inline ("Copied", fades via `animate_value_with_time`); expand = raw transcript (mono) + stt/cleanup/total ms + full timestamp; Esc or re-click collapses; Ctrl+F focuses search; both empty states (§3.11). Clear-all behind the existing `Confirm` widget.
- **Stats panel:** unlock gate under 10 dictations ("3 of 10 dictations to unlock stats" progress placeholder, never zeros), then 2x2 stat cards (dictations, words, speaking time, avg release-to-inject derived from sums), "time saved vs typing at 40 WPM" line, since-date caption, Reset stats behind its own confirm (copy notes history untouched).
- Independence rule: Clear history touches `entries` only; Reset stats zeroes the stats row only.

## Deferred / open

- Real-hardware items parked at CP6: mouse-unplugged keyboard pass, rapid save-save-save restart race, confirmation that eframe restores egui memory (theme preference) before app construction, wgpu-vs-glow measurement (this machine's Application Control policy still blocks release builds, os error 4551).
- `key_related` detection in `hark-app/src/pipeline.rs` is still the substring heuristic ("key" in detail); fine so far.
- Nav pill hover feedback (explicit `Button::fill` suppresses egui hover styling); tune when convenient.
- Stats page placeholder copy becomes live at CP4; History empty state already matches §3.11.
- Dictionary edits save + restart per change; if that ever feels janky in live use, batch on page-leave instead (decide at CP6 with real hardware).

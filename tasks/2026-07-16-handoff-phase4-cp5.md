# Handoff: Phase 4 execution, CP4 done, CP5 next

**Date:** 2026-07-16. Load this (plus the spec) as the starting context for the next session.
**Spec:** `tasks/2026-07-16-phase4-ui-storage.md` (§6 now carries the CP4 lessons).

## Repo state

- `main` @ `3c7a647` (v0.12.0, pushed to origin; the wrap commit adding this handoff is 0.12.1), 316 tests green, fmt + clippy `-D warnings` clean.
- **CP4 delivered:** storage worker thread (`hark-app/src/storage.rs`: `StorageCmd` Record/Prune/DeleteEntry/ClearEntries/ResetStats; writer `Store` on the worker; `StorageHandle` carries the reader `Store`, an `Arc<AtomicU64>` write-generation counter, and joins the worker on Drop), pump tee (per-run `RecordPolicy`, `Prune` sent on every pipeline start), history panel (`ui/history/{mod,row}.rs`: search, day headers, windowed paging, expandable rows, copy/delete, clear-all confirm), stats panel (`ui/stats.rs`: 10-dictation gate, 2x2 cards, reset confirm), format helpers (`ui/format.rs`, jiff 0.2), hark-store migration 002 (stats `total_ms` sum), and a `pages::Views` bundle grouping per-page UI state.

## Load-bearing CP4 discoveries (full list in spec §6)

- `ScrollArea::show_rows` needs uniform row heights; history uses windowed LIMIT queries + "Show more", caches keyed on (generation, search, windows loaded) so idle frames never query.
- Avg release-to-inject derives from the migration-002 `total_ms` sum; a pre-002 DB shows "n/a", never a fake 0 ms.
- The record policy (capture + retention) snapshots into the pump at pipeline start; the storage thread is stateless. Every `start` also sends `Prune`.
- `HarkApp` declares `pipeline` before `storage` on purpose: drop order ends the pump (which holds a storage sender) before `StorageHandle::drop` joins the worker to flush the final write. Do not reorder those fields.
- jiff civil dates drive day grouping (a 2-hour-old entry can be "Yesterday"); tests pin `TimeZone::fixed`.

## Next: CP5 (tray, Minor bump to 0.13.0)

Spec §3.8, checkpoint list §4. Re-verify tray-icon/muda versions at execution (pinned 0.24.1 / 0.19.3 on 2026-07-16). The big pieces:

- **Tray icon created lazily on the first `App` callback** (`logic` runs first), main thread, after the event loop is running; never before `run_native` (macOS hard requirement, invisible on a Windows-only dev loop; get it right by construction).
- **Event delivery while hidden is the open design question:** `MenuEvent::receiver()` / `TrayIconEvent::receiver()` are global static channels, but a hidden, idle window paints no frames, so nothing drains them. Recommended: reuse the CP2 pump pattern — one thread per receiver that `recv()`s, forwards to a UI-side channel, and calls `ctx.request_repaint()` per event (zero idle cost, same lifecycle discipline as the pipeline pump).
- **Menu:** voice radio group, Open Settings, Quit. Voice select = update config + persist + pipeline restart (`settings::save_to_disk` + `PipelineController::start` already do this; restart is the accepted baseline per spec §5).
- **Tray states** map 1:1 from `PipelineStatus` (Idle/Recording/Processing/Errored/Stopped): distinct icons + tooltips, error variants included. Simple generated RGBA icons are fine this phase (draw programmatically; no asset files yet).
- **Close = hide** (`ViewportCommand::CancelClose` + `Visible(false)`), Quit lives in the tray menu; launch hidden when a key resolves at startup, visible when onboarding is needed. Get Started's first-injection retirement and the storage error states must stay reachable once the window can hide.

## Deferred / open

- Real-hardware items parked at CP6, now also including: the history/stats panels have never rendered on real hardware (keyboard-only pass incl. Ctrl+F/Esc, stat-card contrast, day-header readability), and the history Copy button's clipboard write should be sanity-checked against the injection clipboard stash/restore (not concurrent by construction, but verify on hardware).
- Application Control policy still blocks release builds here (os error 4551); wgpu-vs-glow measurement stays at CP6.
- `key_related` detection in `hark-app/src/pipeline.rs` is still the substring heuristic; fine so far.
- Nav pill hover feedback; dictionary save-per-change batching: both decide at CP6 with real hardware.
- Storage failures surface on the History/Stats panels only; the footer stays pipeline-only truth (deliberate, revisit only if it confuses in live use).
- Prune-on-start fires even when key resolution fails (sent before resolve); harmless and intentional (retention applies regardless of dictation availability).

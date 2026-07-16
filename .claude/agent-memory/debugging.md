# Debugging Log

Failed approaches and dead ends. Agents read this before attempting fixes to avoid repeating known failures.

<!-- Add entries in reverse chronological order using this format:
## YYYY-MM-DD: Issue Title
**Symptom:** What was observed.
**Attempted:** What was tried and why it failed.
**Resolution:** What actually worked (or "unresolved" if still open).
**Lesson:** What to do differently next time.
-->

## Known Gotchas (from LL-G + stack research, as of 2026-07-15)

HIGH-severity gotchas for Hark's stack. Full LL-G detail: https://github.com/BoardPandas/LL-G. Full STT-stack research with citations: `.claude/agent-memory/explorer/hark_stt_stack_risk.md`.

- **Rust — GUI-subsystem console child spawns visible windows (HIGH, `kb/rust/gui-subsystem-console-child-window.md`):** a `windows_subsystem="windows"` tray binary has no console; every console child (signing tools, `taskkill`, launch-at-login setup) pops a console window that steals focus. Set `CREATE_NO_WINDOW` (0x0800_0000).
- **Rust — Blocking IO starves the Tokio executor (HIGH, `kb/rust/blocking-io-on-tokio.md`):** `std::fs`/sync calls on an async runtime block the executor. For the BYOK HTTP call use `tokio::fs`/`spawn_blocking` or a blocking client on a worker thread.
- **SQLite — Upsert by display name silently merges records (MEDIUM, `kb/sqlite/upsert-by-name-collision.md`):** key the `stats` table on stable identifiers, not human-readable labels.
- **sherpa-onnx #3267 (open since 2026-03-07):** `modified_beam_search` hotword biasing hallucinates/returns empty ~20% of the time on Parakeet TDT. Default to `greedy_search` + phonetic post-correction; re-check the issue before Phase 2.
- **`sherpa-rs` is deprecated/archived (2026-06-06):** build on the official `sherpa-onnx` crate v1.13.4+.
- **Push-to-talk:** use CGEventTap (macOS) / `WH_KEYBOARD_LL` (Windows), not the `global-hotkey` crate, for reliable held-key edges.

# Hark refined UI implementation

Approved reference: the interactive Hark mock-ups from this task, including the requested visual depth. Implement in Rust/egui; no webview or replacement frontend.

## Foundation

- Centralize charcoal/light palettes, accessible semantic colors, typography, spacing, recessed fields, raised surfaces, and floating-surface shadows in the native theme.
- Preserve Inter and the vendored Phosphor icon family, system appearance, native menu styling, and keyboard navigation.

## Core

- Refresh the shell, top tabs, status footer, update banner, and destructive dialogs.
- Refresh History, Spellbook (including aliases and selection handoff), Invocations/editor, and lifetime Stats with the same tokens.
- Group Settings into Dictation, Audio & shortcut, On-device, Behavior, Privacy, and Updates. Preserve draft/save/discard and immediate key/download actions; keep background tasks polling across navigation.
- Guide first run through cloud/on-device choice, actual configuration, permissions guidance, and the first dictation. Do not claim permission or download success without real evidence.
- Add recording, processing, inserted, and recoverable-error feedback to the existing persistent overlay without creating windows per dictation or depending on parent paint delivery.

## Polish

- Check both themes, narrow window layout, truthful privacy/provider copy, visible focus, empty/error states, and status/icon consistency.
- Regression-test theme contrast, settings routing/state, and overlay transitions. Keep recording visibility tied to the worker's shared state and preserve zero idle repaint work.

## Ship

- Run formatting, workspace tests, clippy with all targets, and the repository wiring check. Compile Windows-only UI paths if the installed cross toolchain supports them.
- Document the new Settings navigation and first-run flow, and add an Unreleased changelog entry. Native microphone/hotkey/window behavior still needs a real desktop smoke test.

## Completion and validation

- Implemented all four phases across the native shell, five pages, six Settings sections, first run, tray, dialogs, and persistent feedback overlay.
- `cargo test --workspace --locked --quiet`: 696 passed, 1 existing ignored test.
- `cargo clippy --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, and `npm run check:claude`: passed.
- `cargo check -p hark-app --all-targets --target x86_64-pc-windows-gnu --locked`: passed with default features, including the local engine.
- Headless egui checks cover both-theme navigation at 720 × 480 and card widths; state tests cover setup readiness, recording release, feedback expiry, and stopped-pipeline races. Contrast checks cover semantic/secondary text, primary actions, and tray marks.
- Native composition, OS permission panels, global shortcut capture, and microphone-to-insertion smoke tests remain hardware validation. No desktop session is available here.

## Lessons Learned / Gotchas

- The lockfile resolves egui/eframe 0.36.2; use its local source for APIs rather than the older 0.35 notes.
- Every icon must use the dedicated Phosphor family, including status rows and mixed icon/text labels.
- Give each Settings section a stable scroll ID. Keep draft values and background task completion independent of the visible section.
- On Windows, the overlay must remain a persistent shaped, nonactivating window; its own pass must know when to hide, even when the parent is delayed.

- egui 0.36 requires headless test harnesses to explicitly clear unused texture deltas; dropping them silently panics in debug builds.

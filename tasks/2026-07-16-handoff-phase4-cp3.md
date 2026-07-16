# Handoff: Phase 4 execution, CP2 done, CP3 next

**Date:** 2026-07-16. Load this (plus the spec) as the starting context for the next session.
**Spec:** `tasks/2026-07-16-phase4-ui-storage.md` (§6 now carries the CP2 lessons).

## Repo state

- `main` @ v0.10.1, 279 tests green, fmt + clippy `-D warnings` clean.
- Commits this session: `eba33d9` (CP2, 0.10.0) plus this wrap-up (0.10.1).
- **hark-app exists and builds:** eframe 0.35 window, `theme.rs` design system (embedded Inter/JetBrains Mono/vendored Phosphor, light+dark Visuals, WCAG tests), sidebar shell + status footer, placeholder pages, key resolution + pipeline start at launch, close = quit. `crates/hark-app/CLAUDE.md` carries the crate conventions (read it first).
- **hark-pipeline emits events:** `run(settings, api_key, events)`; `PipelineEvent::{Recording, Processing, Injected(DictationRecord), Failed{stage, detail}}`. hark-cli passes and drops the channel.

## Load-bearing CP2 discoveries (full list in spec §6)

- eframe 0.35: `App::ui(&mut self, ui: &mut egui::Ui, ..)` gets a root Ui; `logic(ctx, ..)` drains events and also runs while hidden on `request_repaint`. Panels are unified `egui::Panel::left/bottom(..)` + `.exact_size()`; `show_inside` is deprecated.
- egui-phosphor still pins egui 0.34 → Phosphor TTF + codepoints are vendored (`theme::icons`, `assets/README.md`).
- **This machine's Windows Application Control policy blocks `cargo build --release` (os error 4551).** Debug/test/clippy fine. The wgpu size/startup measurement moved to CP6 real-hardware validation; do not fight the policy.
- LL-G contributions pushed (libsqlite3-sys MSRV, eframe trait split, egui companion lag); the deferred batch from the CP2 handoff is done.

## Next: CP3 (settings + onboarding, Minor bump to 0.11.0)

Spec §3.6, §3.11, checkpoint list §4. The big pieces:

- Full settings form: provider kind picker + model + base-URL override, **key section** (masked paste → `hark_keychain::store_key`, status line from `key_status`, Remove via `delete_key`; a stored key is never displayed back), hotkey field bound to `PttChord::parse` with inline validation, voice picker + custom prompt + `skip_below_words`, cleanup provider section ("Inherited from STT (openai)" display + explicit override), history/privacy section (capture toggle, retention caps, disclosure block).
- Progressive disclosure: essentials visible, the rest in `CollapsingHeader`s (§3.11); auto-expand Model & endpoint for openai-compatible.
- **Test connection** on a background thread (bundled ~1 s fixture WAV through the configured provider; hark-stt has a fixture in its tests — check `crates/hark-stt/tests/`), inline persistent results, honest Groq 10 s billing line.
- **Save = validate → `Settings::save` → pipeline restart** (drop old `PipelineHandle` before `run`; rapid save-save-save needs care, spec §5 restart-races note). `key_status` cache in `HarkApp` must refresh after store/remove.
- Get Started card on missing key (§3.11), dictionary editor, both confirms via `egui::Modal`.
- Acceptance: essentials-only default view; full keyboard-only pass (real-hardware part moves to CP6 with the rest of live validation).

## Deferred / open

- `key_related` detection in `hark-app/src/pipeline.rs` is a substring heuristic ("key" in detail); fine for CP2, revisit if it misfires.
- Nav pill hover feedback (explicit `Button::fill` suppresses egui hover styling) — tune during CP3's keyboard/focus pass.
- Stats page copy says "unlock after 10 dictations" with no live count until CP4 wires the DB.
- KB-check note: `versioned-config-migration-backup` / `no-test-touches-real-keyring` still exist in neither LL-G nor BP (re-verified 2026-07-16); local conventions until promoted.

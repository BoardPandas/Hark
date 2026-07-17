# Handoff: Phase 4 CP5+CP6 done, signed releases live, CP7 next

**Date:** 2026-07-17. Load this (plus the spec) as the starting context for the next session.
**Spec:** `tasks/2026-07-16-phase4-ui-storage.md` (§4 now marks CP6 done and CP7 next; §6 carries the CP5/CP6 lessons).

## Repo state

- `main` @ `0.13.5` (the commit adding this handoff). CP0-CP6 delivered; only **CP7** of Phase 4 remains.
- fmt + clippy `-D warnings` clean; full workspace tests green (run `cargo test --workspace`). This machine builds/tests/lints only; the app runs on real Windows/macOS hardware.
- **Signed Windows releases are live.** `.github/workflows/release.yml`: push a `vX.Y.Z` tag (matching `package.json`) and it builds `-p hark-app`, signs with Azure Trusted Signing, verifies the signature, and publishes a GitHub release with `Hark-<ver>-windows-x64.exe`. `v0.13.4` is the first published signed release. Setup + secret names in `.github/RELEASING.md`.

## What landed since the CP5 handoff

- **CP5 (tray, 0.13.0):** `hark-app/src/tray/{mod,icon}.rs` — `TrayState` mapped 1:1 from `PipelineStatus`, programmatic RGBA state icons + tooltips, menu (voice radio group shared with the settings form via `form::VOICES`, Open Settings, Quit), one pump thread per global receiver (`MenuEvent`/`TrayIconEvent`) forwarding to a UI channel with `request_repaint`. Tray created lazily on the first `logic` call; close = hide (`CancelClose` + `Visible(false)`) gated on the tray existing; launch hidden when the pipeline is running.
- **CP6 (live validation, 0.13.x):** interactive checklist run through hark-app on real Windows hardware, confirmed working end to end. A **microphone picker** was added during this pass (`hark-audio` dep, `form::mic_section`, `SettingsPage::mic_devices`): device enumeration off the UI thread (WASAPI COM), an unplugged-but-configured device stays selectable, Rescan re-scans.
- **Signed release pipeline (0.13.1 -> 0.13.4):** four iterations to green. The failures are recorded as lessons (below + spec §6, routed to LL-G).

## Load-bearing discoveries (full list in spec §6)

- **The tray pumps drain global static receivers that never disconnect** — spawn them exactly once (`Tray::create`, guarded by a `tray_failed` no-retry flag) or events double-deliver. `MenuEvent::set_event_handler` and `::receiver()` are mutually exclusive.
- **Native `CheckMenuItem`s self-toggle on click**, so the voice radio group reconciles every checkmark unconditionally after a selection.
- **Azure Trusted Signing action (v2) takes SP creds via `with:` inputs, not step `env:`** (env is clobbered by the action's own credential-input promotion; auth fails "not fully configured"). An empty signing secret surfaces as an opaque `SignerSign() failed` deep in signtool, so the workflow preflights every secret (lengths only). The service is rebranded "Artifact Signing"; `Azure/trusted-signing-action` redirects to `Azure/artifact-signing-action@v2`.
- **Mic enumeration must stay off the egui/main thread** (WASAPI COM); gathered at page construction + on explicit Rescan, never per frame.

## Next: CP7 (retire hark-cli, Patch)

Spec §2.5 + §4. hark-app has dictated end to end on hardware (CP6), which is the gate the plan set for deleting the CLI. Steps:

- Delete `crates/hark-cli` and remove it from the workspace `Cargo.toml` `members`.
- Sweep references: README, any docs, scripts, `.claude/` rules/agent-memory mentions. The release workflow already builds `-p hark-app` only, so it needs no change. The per-crate `examples/` dev harnesses (`transcribe_spike`, `cleanup_spike`) live in their own crates and stay.
- Confirm `cargo build --workspace`, `cargo test --workspace`, `cargo clippy --all-targets -- -D warnings`, and `cargo fmt --check` all pass without hark-cli.
- CHANGELOG + `package.json` bump (Patch) per `.claude/rules/commit-changelog.md`; one commit.

After CP7, Phase 4 is complete. Next body of work is **Phase 5 (Polish/Ship):** macOS packaging/notarization, launch-at-login, single-instance guard, the first-run macOS permission flow (Accessibility + microphone), and the deferred UI polish (floating recording pill, tray asset art, nav hover feedback). A Windows MSI (WiX) installer around the signed exe is an option here too.

## Deferred / open (carried forward)

- **macOS validation still waits for hardware** — everything is built macOS-correct by construction (main-thread UI, tray after loop start) but validated only on Windows so far.
- **CP6 per-metric figures were not transcribed** (release-to-inject p50/p95, per-voice results); add to the spec/Phase 3 spec if captured.
- **Application Control policy blocks `cargo build --release` on the dev box** (os error 4551); the wgpu-vs-glow size/startup measurement is still unrun (it does run fine on the CI Windows runner, which is how releases build).
- `key_related` detection in `hark-app/src/pipeline.rs` is still a substring heuristic; fine so far.
- Storage failures surface on History/Stats only; the footer stays pipeline-only truth (deliberate).
- The release workflow uses `actions/checkout@v4` (a run annotation notes Node 20 deprecation on some actions); harmless, bump to `@v5`/newer at leisure.

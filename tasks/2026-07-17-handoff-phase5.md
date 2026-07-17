# Handoff: Phase 4 complete, Phase 5 (Ship) Windows-side landed, macOS + single-instance remain

**Date:** 2026-07-17. Load this as the starting context for the next session.
**Canonical scope:** `tasks/plan-repo.md` §"Phase 5, Polish / Ship" (line ~153) is the one-line source of truth. There is **no dedicated Phase 5 spec doc yet** — the remaining macOS work needs a planning session before execution (plan in one session, execute in another). The two feature plans already executed this phase are `tasks/2026-07-17-windows-installer-autostart.md` and `tasks/majestic-percolating-puffin.md` (in-app updater); both carry Lessons Learned sections routed to LL-G.

## Repo state

- `main` @ **`0.14.0`** (HEAD `6a33396`), tree clean, single linear history.
- fmt + clippy `-D warnings` and workspace tests were green per each landing commit's own verification. **This machine builds/tests/lints only** (WDAC/Application Control constrains release builds — see gotchas); the app runs on real Windows/macOS hardware.
- **Signed Windows releases + installer are live.** `.github/workflows/release.yml`: push a `vX.Y.Z` tag matching `package.json` (and the Cargo workspace version — the run verifies all three agree) → builds `-p hark-app`, signs the exe (Azure Trusted Signing / "Artifact Signing"), builds a signed per-user Inno Setup `setup.exe`, verifies both signatures, and publishes a release with **`Hark-<ver>-windows-x64-setup.exe`** (headline) + the portable `Hark-<ver>-windows-x64.exe`. Setup + secret names in `.github/RELEASING.md`.
- **`hark-cli` retired** (0.13.7). Workspace crates: `hark-app, -audio, -autostart, -config, -dictionary, -hotkey, -inject, -keychain, -pipeline, -store, -stt, -update, -voice`.

## What landed this phase (0.13.6 → 0.14.0)

- **In-app update checker + Windows self-update** (`hark-update` crate, commit `e70d4cd`). Checks GitHub `releases/latest`, semver-compares, shows a startup banner + a Settings "Check for updates" control; on Windows downloads the signed exe, verifies its Authenticode signature **and signer-subject against the running app** (WinVerifyTrust) before an in-place `self-replace` swap; macOS links out to the release page. Config: `[updates] check_on_startup` (default on).
- **Push-to-talk recording overlay** with audio-reactive pulse (`b3c317d`) — this satisfies the deferred "floating recording pill" UI item.
- **Record the push-to-talk shortcut** instead of typing the chord (`42bfb92`) — `hark-hotkey::spawn_capture` + `settings/capture.rs`, riding the same low-level hook the pipeline uses (egui can't see L/R modifiers or the Win key). Recording pauses the pipeline's hook (only one keyboard hook at a time).
- **Microphone picker** — backend (`e5be899`) + Settings UI (`6a33396`). `[audio] input_device` by cpal name; unavailable device stays selected ("not connected") and capture falls back to the system default; Rescan re-enumerates. Enumeration stays off the egui/main thread (WASAPI COM).
- **Windows installer + launch-at-login** (`d81aa5e`). New `hark-autostart` crate manages an `HKCU\...\Run` value in-process via `winreg` (Settings → Behavior toggle, default on). Installer `installer/hark.iss` (Inno Setup), per-user `%LOCALAPPDATA%\Programs\Hark`, no UAC.
- **README** rewritten for the BYOK-cloud pivot (retired on-device sherpa-onnx/Parakeet language).

> Note: plan-repo names "WiX/MSI" for Windows, but the team **deliberately chose Inno Setup** (`setup.exe`, per-user, no-UAC). That is done — do not "finish" an MSI thinking it's missing.

## Load-bearing discoveries (full detail + citations in the two plan docs; routed to LL-G)

**Autostart / installer:**
- Write the Run key **in-process (`winreg`)** — never shell to `reg.exe`/`powershell`; the windowless build flashes a focus-stealing console (LL-G HIGH `gui-subsystem-console-child-window.md`).
- **Don't fight Task Manager:** touch only the Run *value*, never the `StartupApproved\Run` flag Windows uses to record a user's "disable".
- Quote the exe path in the Run value; **self-heal to `current_exe()`** on enable so upgrades don't leave a stale path.
- **CI sign order:** sign `hark-app.exe` *before* ISCC bundles it, then sign the produced `setup.exe`. `AppId` GUID is forever. ISCC isn't preinstalled (`choco install innosetup`).

**Update checker:**
- **WDAC/Application Control blocks stale proc-macro DLLs** (`os error 4551`). Don't `cargo clean` proc-macro deps casually here; if you must, clean the leaf too (e.g. `cargo clean -p displaydoc`) to force a fresh DLL.
- `releases/latest` is public but **requires a `User-Agent`** (403 without); 60 req/hr unauthenticated is ample. The multi-MB download **overrides the shared client's 15 s timeout** to 600 s per-request.
- windows-rs 0.62 crypto surface quirks (no `HCRYPTMSG`; raw `*mut c_void` handles) — see plan doc if touching the signature verify.

## Next: remaining Phase 5 (needs a planning session first)

Everything here is the **macOS half** plus one cross-platform gap. Built mac-correct by construction (main-thread UI, tray after loop start, `hark-autostart` no-op stubs off Windows) but **none of it has run on real macOS hardware yet.**

1. **Single-instance guard (cross-platform, do first).** Not implemented today. Launch-at-login + a manual launch can double-run the app (two hotkey hooks, two trays). Named mutex on Windows; lock file / `flock` or equivalent on macOS. Referenced in plan-repo (`hark-app` "single-instance guard") but never built.
2. **macOS packaging + notarization.** No macOS CI job exists (`release.yml` is `windows-latest` only). Need a `.app` bundle → `.dmg`/`.pkg`, `codesign`, `notarytool`, staple. This unblocks the updater's macOS path (currently link-out only because there's no mac artifact).
3. **macOS launch-at-login.** `hark-autostart` is Windows-only. Add the `SMAppService` (or `LaunchAgent`) login item behind the same `reconcile`/Settings-toggle API.
4. **macOS first-run permission flow.** Accessibility (required for the CGEventTap hotkey **and** injection) + microphone. Detect, prompt, and guide the user to System Settings; the pipeline can't function until granted.

## Deferred / open (carried forward)

- **Signed-Windows-hardware validation of self-update still owed** (user): WinVerifyTrust + signer-subject match against a *real* signed release, the `self-replace` swap, and a `CREATE_NO_WINDOW` relaunch with no console flash. The check/compare/asset-selection paths are verified against production data; the install path is not.
- **macOS validation waits for hardware** (carried since Phase 4).
- **WDAC blocks `cargo build --release` on this dev box** (os error 4551 family); release builds happen on the CI Windows runner. The wgpu-vs-glow size/startup measurement is still unrun.
- `actions/checkout@v4` in `release.yml` emits a Node 20 deprecation annotation; bump to `@v5` at leisure (harmless).
- `key_related` detection in `hark-app/src/pipeline.rs` is still a substring heuristic; fine so far.
- Minor UI polish still open: tray icon is programmatic RGBA (no art asset), nav hover feedback. Low priority.
- CP6 per-metric latency figures (release-to-inject p50/p95, per-voice) were never transcribed into the specs.

# Plan — Windows Installer + Auto-Launch at Login

**Phase:** Ship
**Date:** 2026-07-17
**Status:** Planned (not started)

## Problem

The GitHub release ships a bare, signed `hark-app.exe`. Double-clicking it just
runs the portable binary; there is no install step, no Start Menu entry, no
uninstaller, and nothing configures Hark to start with Windows. The user wants:

1. A real **installer** (click → installs), not a portable exe.
2. **Auto-launch at login**, running in the background in the system tray with
   no main window.
3. A **Settings toggle** to turn auto-launch off/on, **on by default**.

## Decisions (locked with the user 2026-07-17)

- **Installer:** Inno Setup, producing a signed `setup.exe`.
- **Scope:** **Per-user**, installs to `%LOCALAPPDATA%\Programs\Hark`, **no admin /
  no UAC** (`PrivilegesRequired=lowest`).
- **Autostart is app-managed**, not installer-hardcoded: a `HKCU\...\Run` value
  the Settings toggle writes/removes and that self-heals to the current exe path.
- Default **on**.

## What already exists (no work needed)

- The app **already launches hidden into the tray** — `main.rs` sets
  `.with_visible(false)` unconditionally; `HarkApp::new` only shows the window
  when it needs attention (onboarding, config error, stopped pipeline). So
  "background, tray, no main window" is already the runtime behavior. We are NOT
  re-implementing hide-on-startup.
- Close-to-hide and the tray menu (Open Settings / voices / Quit) exist from
  Phase 4 CP5 (`crates/hark-app/src/tray/mod.rs`, `app.rs::handle_close`).
- Release signing is working (Azure Trusted Signing, `release.yml`). We extend
  it; we do not rework the auth that took 0.13.1–0.13.4 to stabilize.

---

## Phase S1 — App-managed autostart (Rust)

New crate **`hark-autostart`** (workspace member). Keeps Windows registry code
and platform `cfg` out of the UI crate; leaves a clean seam for the macOS login
item later. Minimal API:

```rust
/// Ensure the OS startup entry matches `enabled`. enabled → create/update the
/// Run value to the current exe; disabled → remove it. Idempotent. Never
/// touches Task Manager's StartupApproved flag. Non-fatal for the caller.
pub fn reconcile(enabled: bool) -> Result<(), Error>;

/// True if the Run value exists AND points at the current exe (optional; for
/// diagnostics/self-test, not the UI source of truth).
pub fn is_enabled() -> Result<bool, Error>;
```

**Windows impl** (`#[cfg(windows)]`), via the **`winreg`** crate (NOT shelling
out to `reg.exe`/`powershell` — see Gotcha 1):

- Key: `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
- Value name: `Hark`
- Value data: `"<current_exe>" --hidden` — path **quoted**, args after the close
  quote. Path from `std::env::current_exe()`, canonicalized.
- Enable = write/overwrite the value (self-heals a stale post-upgrade path).
- Disable = delete the value only. Leave `StartupApproved\Run` untouched.

**Non-Windows impl:** no-op stub returning `Ok(())` so `hark-app` compiles on
macOS/Linux. macOS login item (`SMAppService` / LaunchAgent) is a separate task.

**Config:** add to `hark_config::Settings`:

```rust
pub launch_at_login: bool,   // default true
```

- Defaults to `true` → fresh installs and upgrading users get autostart on.
- Serialize under a new `[startup]` section or top-level; keep it in the
  `#[serde(default)]` model so old config files load (they become "true").
- Round-trip + default unit tests alongside the existing `hark-config` tests.

**Startup wiring** (`hark-app`):

- Parse `--hidden` from argv (accepted; currently equivalent to the default
  always-hidden launch — see Gotcha 10). Keep it so the Run-key value is stable
  if manual-launch-shows-window is added later.
- After settings load, call `hark_autostart::reconcile(settings.launch_at_login)`;
  log the outcome (label only), warn-and-continue on error.

**Testable here:** crate builds + clippy; unit tests for value formatting
(quote + arg join) and the enable/disable decision. Gate any test that writes
the real HKCU behind a distinct test value name (e.g. `HarkTest`) that the test
creates and removes, so it never collides with a real install.

## Phase S2 — Settings toggle (UI)

- Add a checkbox to `behavior_section` in
  `crates/hark-app/src/ui/settings/form.rs` (sits with Theme + Skip-cleanup):
  **"Launch Hark at login (runs in the background)"**, bound to
  `draft.launch_at_login`, with a one-line weak caption.
- The toggle's truth is `settings.launch_at_login` (config); the registry is
  derived — **no per-frame registry reads** (hark-app rule: no OS I/O per frame).
- After any successful `settings::save_to_disk`, call
  `hark_autostart::reconcile(self.settings.launch_at_login)` so flipping the
  toggle + Save updates the OS entry immediately. Surface failures via the
  existing save-notice channel (`set_save_notice`).

## Phase S3 — Inno Setup installer

New `installer/hark.iss`:

- `[Setup]`: stable `AppId={{GUID}}` (generate once, never change — Gotcha 8),
  `AppName=Hark`, `AppVersion` passed via `/DAppVersion=` from CI,
  `PrivilegesRequired=lowest`, `DefaultDirName={autopf}\Hark` (→
  `%LOCALAPPDATA%\Programs\Hark` under lowest privileges — Gotcha 5),
  `OutputBaseFilename=Hark-{#AppVersion}-windows-x64-setup`,
  `WizardStyle=modern`, `SolidCompression=yes`, `ArchitecturesInstallIn64BitMode=x64`.
- `[Files]`: the signed `hark-app.exe` → installed as `Hark.exe` (friendly name
  in Task Manager's Startup tab). Source passed via `/DSourceExe=`.
- `[Icons]`: `{autoprograms}\Hark` → `Hark.exe`; optional desktop icon task.
- `[Registry]`: create the `HKCU\...\Run` `Hark` value (`"...\Hark.exe" --hidden`)
  with `uninstalldeletevalue` so autostart is active immediately post-install AND
  a leftover value can never point at a deleted exe after uninstall. (The app
  reconciles this same value thereafter; both write identical data.)
- `[Run]`: "Launch Hark" checkbox, `Flags: nowait postinstall skipifsilent`,
  runs `Hark.exe` **without** `--hidden` so the first launch shows onboarding.
- Uninstaller is automatic; leaves `%APPDATA%\hark` (config + history) in place —
  user data survives reinstalls.

## Phase S4 — Release workflow + docs

Edit `.github/workflows/release.yml`, inserting **after** the app-exe sign +
verify steps and **before** publish:

1. **Install Inno Setup** on the runner: `choco install innosetup -y` (ISCC is
   not preinstalled — Gotcha 7).
2. **Build the installer:**
   `iscc /DAppVersion=<ver> /DSourceExe=target\release\hark-app.exe installer\hark.iss`
   → `installer\Output\Hark-<ver>-windows-x64-setup.exe`. The bundled exe is
   already signed (Gotcha 6 — sign order).
3. **Sign the installer** with a second `Azure/artifact-signing-action@v2` step
   pointed at `installer\Output`, `files-folder-filter: exe`.
4. **Verify** the installer signature (reuse the `Get-AuthenticodeSignature`
   check).
5. **Publish:** upload the **installer as the headline asset**; also keep
   uploading the portable `Hark-<ver>-windows-x64.exe` for users who want it.

Docs:
- `.github/RELEASING.md`: note the Inno dependency + the two-artifact output.
- `README.md`: update the download/install section (installer is primary).
- CHANGELOG `[Unreleased]` → **Added**: installer + launch-at-login; **bump the
  Minor** version in `package.json` (new feature) at implementation time, per
  `.claude/rules/commit-changelog.md`.

## Validation (real Windows hardware — this machine is build/test only)

1. Run the signed `setup.exe`: **no UAC prompt**; installs to
   `%LOCALAPPDATA%\Programs\Hark`; Start Menu entry present.
2. First launch shows onboarding (fresh config, no key).
3. Task Manager → Startup: `Hark` present + enabled.
4. Reboot / log out+in: Hark starts hidden in the tray, no window.
5. Settings → uncheck "Launch at login" → Save → Run value gone → next login
   does not autostart. Re-check → value restored.
6. Disable via Task Manager Startup tab → relaunch → app does **not** re-enable
   it (StartupApproved flag respected — Gotcha 2).
7. Uninstall: files + Run value removed; `%APPDATA%\hark` (history/config) kept.

---

## Lessons Learned / Gotchas

1. **Never shell out to `reg.exe`/`powershell` to write the key.** The release
   build is `windows_subsystem="windows"` (no console); any console child flashes
   a window that steals focus (LL-G HIGH `kb/rust/gui-subsystem-console-child-window.md`).
   The `winreg` crate writes HKCU in-process with no child, sidestepping this
   entirely. (`CREATE_NO_WINDOW` only matters if we ever *do* spawn a child.)
2. **Don't fight Task Manager.** Windows disables a startup entry by writing a
   flag under `...\StartupApproved\Run`, not by deleting the Run value. Reconcile
   only the Run *value*; never read/write the approval flag, or the app will loop-
   fight a user who disabled it in Task Manager.
3. **Quote the exe path** in the Run value (`"C:\...\Hark.exe" --hidden`);
   unquoted paths with spaces break at login. Args go after the closing quote.
4. **Self-heal to `current_exe()`.** After an upgrade the install path can change;
   enable-reconcile always rewrites the value to the current exe so stale paths
   fix themselves on next launch.
5. **`{autopf}` + `PrivilegesRequired=lowest`** resolves to
   `%LOCALAPPDATA%\Programs` (per-user, no UAC). With admin it becomes Program
   Files — pin `lowest` to guarantee the no-UAC install the user asked for.
6. **CI sign order:** sign `hark-app.exe` **before** ISCC bundles it, then sign
   the produced `setup.exe`. Signing only the installer leaves the extracted exe
   unsigned.
7. **ISCC is not on `windows-latest`** — `choco install innosetup -y` first.
8. **`AppId` GUID is forever** — it keys upgrades and uninstall. Generate once;
   never change it.
9. **macOS parity deferred:** login item via `SMAppService`/LaunchAgent + a
   notarized `.pkg`/`.dmg` is a separate task. Keep the `hark-autostart` API
   cross-platform (no-op stub off Windows) so the UI crate compiles everywhere.
10. **Don't double-implement hide-on-startup.** eframe already launches hidden
    (`main.rs` `.with_visible(false)`). `--hidden` is accepted but currently
    equivalent to the default; it exists to keep the Run-key value stable if a
    future change makes a *manual* launch show the window while autostart stays
    hidden.

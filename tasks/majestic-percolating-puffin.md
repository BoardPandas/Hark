# Plan: In-App Update Checker + Self-Update (GitHub Releases)

## Context

Hark ships as a **single signed portable `.exe`** published to GitHub Releases
(`BoardPandas/Hark`), named `Hark-<version>-windows-x64.exe`
(see [release.yml](.github/workflows/release.yml)). There is currently no way
for a running copy to learn it is out of date; the user must manually watch the
releases page and re-download. This adds:

1. A **"Check for updates"** control in Settings that queries GitHub Releases and,
   when newer, downloads the signed exe, verifies it, and installs it in place.
2. An **automatic check on startup** that shows a dismissible **banner** when an
   update is available, with the same one-click install.

### Decisions locked in with the user
- **Version source:** sync the Cargo workspace version to `package.json` and keep
  both bumping together. (Today they disagree — Cargo `0.1.0` vs package.json
  `0.13.5` — so the compiled-in `env!("CARGO_PKG_VERSION")` would make every build
  look ancient. This is the load-bearing prerequisite. Sync Cargo to whatever
  `package.json` reads at implementation time.)
- **Download safety:** verify the downloaded exe's **Authenticode signature**
  (WinVerifyTrust) and require its signer to match the running exe's signer before
  replacing anything. Refuse to install otherwise.
- **Platform scope:** Windows does the full check → download → install. macOS runs
  the check but "Install" just opens the GitHub release page in a browser (no mac
  build is published yet).

### Knowledge-base notes (LL-G, RULE 1)
- **CREATE_NO_WINDOW (Rust HIGH #2):** the release binary is `windows_subsystem =
  "windows"` (no console). The post-install relaunch must spawn via
  `CommandExt::creation_flags(0x0800_0000)` or a console flashes/steals focus.
- **reqwest transport masking (Rust HIGH #3):** not multipart here (plain GET), but
  still surface timeouts distinctly so a stalled download reports clearly.
- **Drop-order deadlock (Rust HIGH #9):** background work uses **detached threads +
  `mpsc` + `ctx.request_repaint()`** (the exact pattern in
  [settings/test.rs](crates/hark-app/src/ui/settings/test.rs)); nothing is joined
  on the UI thread.

---

## Part A — Version reconciliation (prerequisite)

1. **[Cargo.toml](Cargo.toml):** bump `[workspace.package] version` `0.1.0` → the
   current `package.json` version (`0.13.5` at time of writing) to match it. All crates inherit via `version.workspace = true`, so
   `env!("CARGO_PKG_VERSION")` becomes correct everywhere.
2. **[.claude/rules/commit-changelog.md](.claude/rules/commit-changelog.md):** amend
   step 2 to bump **both** `package.json` and the Cargo workspace version together,
   and stage both.
3. **[release.yml](.github/workflows/release.yml)** "Resolve and verify version"
   step: additionally parse the Cargo workspace version and fail if it disagrees
   with `package.json`/tag — so a future drift fails loudly instead of shipping a
   binary that misreports its version.

---

## Part B — New crate `hark-update`

A small, focused workspace crate (matches the one-concern-per-crate convention).
Register in the root `[workspace] members`. Reuses pins already in the tree:
`reqwest 0.13` (blocking, rustls, webpki-roots, json), `windows 0.62.2`, `serde`,
`serde_json`, `semver`, `thiserror`, `log`, plus `self-replace` (new; purpose-built
for the running-exe swap).

**`crates/hark-update/src/lib.rs`** — public API, all synchronous/blocking (callers
run it on a worker thread):

```
pub struct ReleaseInfo { pub version: Version, pub tag: String,
                         pub notes: String, pub asset_url: String, pub asset_name: String }

/// GET https://api.github.com/repos/BoardPandas/Hark/releases/latest
/// (User-Agent required; unauthenticated, 60/hr). Parse tag_name → semver,
/// pick the `*-windows-x64.exe` asset, compare to `current`. Returns
/// Ok(Some(info)) only when strictly newer, Ok(None) when up to date.
pub fn check(client: &reqwest::blocking::Client, current: &Version) -> Result<Option<ReleaseInfo>>

/// Stream the asset to `<dir-of-current-exe>/<asset_name>.download` (same
/// volume as the exe, required for the in-place swap).
pub fn download(client, &ReleaseInfo) -> Result<PathBuf>

/// Windows: WinVerifyTrust (WTD_UI_NONE) for chain validity + trusted timestamp,
/// then require the staged exe's signer subject to equal the *running* exe's
/// signer subject (self-anchoring; no hardcoded CN). Skipped/Err on non-Windows.
pub fn verify(staged: &Path) -> Result<()>

/// self_replace(staged) to swap the running exe, then return so the caller can
/// relaunch. On failure the staged file is removed and nothing is swapped.
pub fn apply(staged: &Path) -> Result<()>

/// Relaunch current_exe with CREATE_NO_WINDOW, then the caller exits the process.
pub fn relaunch() -> Result<()>
```

- `check`/`download` take `&reqwest::blocking::Client` so hark-app passes the
  existing long-lived `hark_stt::shared_client()` (one client per process rule).
- `verify` lives behind `#[cfg(windows)]`; a `#[cfg(not(windows))]` stub returns an
  `Unsupported` error the UI treats as "open the release page instead".
- `windows` features (cfg(windows) target dep): `Win32_Security_WinTrust`,
  `Win32_Security_Cryptography`, `Win32_Foundation` (mirror hark-hotkey's style).

---

## Part C — hark-app wiring

### C1. Config toggle
[hark-config/src/lib.rs](crates/hark-config/src/lib.rs): add a small
`#[serde(default)] struct Updates { check_on_startup: bool }` (default `true`) and a
`pub updates: Updates` field on `Settings`. Round-trip test alongside the existing
default/round-trip tests.

### C2. Shared updater state machine
New `crates/hark-app/src/update.rs` — an `Updater` owned by `HarkApp`, modeled on
[settings/test.rs](crates/hark-app/src/ui/settings/test.rs):

```
enum Phase { Idle, Checking, UpToDate, Available(ReleaseInfo),
             Downloading, Verifying, ReadyToRestart, Error(String), Dismissed }
```
Methods `start_check(client, ctx)`, `start_install(ctx)` (download→verify→apply,
one worker thread, staged path passed through), `restart()` (relaunch + exit),
`poll()` (drain the mpsc each frame). One instance drives **both** the banner and
the Settings section, so there is a single flow, not two.

Construct in `HarkApp::new` ([app.rs](crates/hark-app/src/app.rs)); if
`settings.updates.check_on_startup`, kick `start_check` with
`hark_stt::shared_client()`. Drain via `updater.poll()` inside `App::logic`.

### C3. Startup banner
[ui/shell.rs](crates/hark-app/src/ui/shell.rs): when `updater` is
`Available`/`Downloading`/`Verifying`/`ReadyToRestart`, render a top
`Panel::top("update-banner")` above the sidebar: icon + "Update available: vX.Y.Z",
an **Install** button (→ `updater.start_install`; on macOS → `ui.ctx().open_url`
to the release page), a **Restart** button once ready, and a **Dismiss** (×). All
colors/spacing via `theme.rs` tokens (accent/SUCCESS), status by icon+label never
color alone (crate rule).

### C4. Settings section
New `crates/hark-app/src/ui/settings/updates.rs`, rendered from
[settings/mod.rs](crates/hark-app/src/ui/settings/mod.rs) `show()` (takes
`&mut Updater`). Shows: current version, a **Check for updates** button, live
result (up-to-date / available + release notes + **Download & install** / progress
spinner / **Restart to finish** / error), and the **"Check automatically on
startup"** toggle bound to `settings.updates.check_on_startup` (Save persists it,
consistent with the rest of the form). Keep the module under ~300 lines (crate rule).

### C5. Cargo wiring
[hark-app/Cargo.toml](crates/hark-app/Cargo.toml): add `hark-update = { path =
"../hark-update" }`. hark-app already depends on `hark-stt` for the shared client.

---

## Files touched (summary)
- **New:** `crates/hark-update/` (Cargo.toml, src/lib.rs [+ small `windows`/`verify`
  module]), `crates/hark-app/src/update.rs`,
  `crates/hark-app/src/ui/settings/updates.rs`.
- **Edit:** root `Cargo.toml` (version + workspace member),
  `crates/hark-app/Cargo.toml`, `crates/hark-app/src/app.rs`,
  `crates/hark-app/src/ui/shell.rs`, `crates/hark-app/src/ui/settings/mod.rs`,
  `crates/hark-config/src/lib.rs`, `.claude/rules/commit-changelog.md`,
  `.github/workflows/release.yml`, `CHANGELOG.md`.

## Out of scope
- macOS auto-install (no mac artifact yet — link-out only).
- Delta/background silent updates, rollback, staged rollout channels.
- Signing changes (the CI signing flow is untouched).

---

## Verification
This machine builds/tests/lints only; the window + real self-replace are validated
on Windows hardware (per crate rules). Plan:

1. **Build/lint:** `cargo build -p hark-update -p hark-app`,
   `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.
2. **Unit tests (`cargo test -p hark-update -p hark-config`), no network:**
   - semver compare: older/equal/newer tag vs current (newer→Some, else None).
   - GitHub JSON parse against a captured `releases/latest` fixture; asset picker
     selects `*-windows-x64.exe`.
   - `Updates` config default = startup-check on; TOML round-trips.
3. **Manual (Windows hardware):** with the running version temporarily set below the
   latest release — start Hark → banner appears; Settings → **Check for updates**
   reports the new version + notes; **Download & install** downloads, verifies the
   signature, swaps the exe, **Restart** relaunches into the new version (verify the
   sidebar version caption bumped). Confirm no console window flashes on relaunch.
   Negative: point verify at an unsigned/tampered exe and confirm it refuses to
   install and reports the failure.
4. **macOS (when available):** check runs; **Install** opens the release page in the
   browser; no self-replace attempted.

## Lessons Learned / Gotchas (route to LL-G)
- **WDAC/Application Control blocks freshly-built proc-macro DLLs (`os error 4551`).**
  This machine enforces an Application Control policy. `cargo clean -p url` forced a
  rebuild of `url`→`idna`→`displaydoc`, and loading the *stale* `displaydoc` proc-macro
  DLL was blocked ("An Application Control policy has blocked this file"). Fix:
  `cargo clean -p displaydoc` to force a fresh DLL, which loads. Do NOT `cargo clean`
  cached proc-macro deps casually here. This also explained the initial misleading
  "can't find crate for url" — a prior interrupted build left `url` un-produced.
- **Raw-string terminator vs Markdown release notes.** A test fixture with
  `"body": "### Added` inside an `r#"..."#` literal closed the raw string early: the
  JSON value's opening `"#` IS the `"#` terminator (and `r##"` fails too, since `"###`
  contains `"##`). Use a normal escaped string for JSON fixtures whose values start
  with `#`.
- **windows-rs 0.62 crypto types:** there is no `HCRYPTMSG` type — the message handle
  is a raw `*mut c_void`; `CryptQueryObject` `phmsg` is `Option<*mut *mut c_void>`.
  `CertCloseStore`/`CryptMsgClose`/`CertFreeCertificateContext` take `Option<...>`
  handles. `CERT_NAME_SIMPLE_DISPLAY_TYPE`/`CMSG_SIGNER_CERT_INFO_PARAM` are plain
  `u32`; `CERT_FIND_SUBJECT_CERT` is a typed `CERT_FIND_FLAGS`.
- **`releases/latest` is public + needs a User-Agent.** Verified live: `BoardPandas/Hark`
  is a public repo, so unauthenticated `GET /releases/latest` works (no token). GitHub
  rejects requests without a `User-Agent` header (403). Unauthenticated limit is 60/hr
  per IP, ample for a once-per-launch check.
- **Download timeout:** the shared STT client caps requests at 15 s total, too short for
  a multi-MB exe; the download overrides `.timeout()` per-request (600 s) while reusing
  the one client.
- Still to validate on signed Windows hardware (user): WinVerifyTrust + signer-subject
  match against a real signed release, `self-replace` swap of the portable exe, and the
  `CREATE_NO_WINDOW` relaunch (confirm no console flash). The GitHub check path,
  version compare, and asset selection are verified against production data.

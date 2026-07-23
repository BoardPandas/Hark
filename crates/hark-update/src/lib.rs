//! In-app update checking and self-update against GitHub Releases.
//!
//! Hark ships as a single signed portable `.exe` published as a GitHub release
//! asset (`Hark-<version>-windows-x64.exe`). This crate:
//!
//! 1. `check` — asks the Releases API for the latest tag and compares it, by
//!    SemVer, to the running version.
//! 2. `download` — streams the signed asset next to the running exe.
//! 3. `verify` (Windows) — WinVerifyTrust plus a signer-subject match against
//!    the running exe, so only a validly-signed build from the same publisher
//!    is ever installed. See [`verify`].
//! 4. `apply` + `relaunch` — swap the running exe (via `self-replace`) and start
//!    the new one with no console window.
//!
//! Everything here is blocking; callers run it on a worker thread and pass the
//! process-wide `reqwest::blocking::Client` (keep-alive + TLS resumption). No
//! API key or audio ever passes through this crate, so nothing here logs
//! secrets.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

mod verify;
pub use verify::verify;

/// GitHub repository that publishes Hark releases.
const REPO_OWNER: &str = "BoardPandas";
const REPO_NAME: &str = "Hark";

/// GitHub requires a User-Agent on every API request or it returns 403.
const USER_AGENT: &str = concat!(
    "hark-update/",
    env!("CARGO_PKG_VERSION"),
    " (BoardPandas/Hark)"
);

/// The published asset is named `Hark-<version>-windows-x64.exe`; match by
/// suffix so a version change in the middle does not break the picker.
const WINDOWS_ASSET_SUFFIX: &str = "-windows-x64.exe";

/// Passed to the process [`relaunch`] spawns so it knows to wait for this
/// (outgoing) instance to release the single-instance lock instead of losing
/// the startup race and exiting. `hark-app` reads it on startup; without it a
/// relaunch left the user with no running Hark after "Download & install".
pub const RELAUNCHED_FLAG: &str = "--relaunched-after-update";

/// A binary download can take much longer than the STT client's 15 s total
/// timeout, so the download request overrides it with a generous ceiling.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("could not parse version {value:?}: {source}")]
    Version {
        value: String,
        source: semver::Error,
    },
    /// The release carries no Windows exe asset (cannot self-install).
    #[error("the latest release has no Windows download")]
    NoWindowsAsset,
    /// Signature verification refused the downloaded file.
    #[error("update verification failed: {0}")]
    Verification(String),
    /// Could not locate the running executable to stage next to / relaunch.
    #[error("cannot locate the running executable: {0}")]
    CurrentExe(String),
}

/// A newer release than the one running.
#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    /// Normalized SemVer string, e.g. `0.14.0`.
    pub version: String,
    /// The git tag, e.g. `v0.14.0`.
    pub tag: String,
    /// Release notes body (may be empty).
    pub notes: String,
    /// The release page on github.com, for the "view release" / macOS path.
    pub html_url: String,
    /// The Windows asset filename, empty if the release has none.
    pub asset_name: String,
    /// Direct download URL for the Windows asset, empty if none.
    pub asset_url: String,
}

impl ReleaseInfo {
    /// True when a Windows asset exists to self-install; false means the UI
    /// should fall back to opening [`ReleaseInfo::html_url`].
    pub fn has_windows_asset(&self) -> bool {
        !self.asset_url.is_empty()
    }
}

// --- GitHub Releases API JSON (only the fields we use) ---

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    html_url: String,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
}

/// Ask GitHub for the latest release and compare it to `current_version`
/// (typically `env!("CARGO_PKG_VERSION")`). Returns `Ok(Some(_))` only when the
/// release is strictly newer, `Ok(None)` when already up to date.
///
/// `releases/latest` excludes drafts and prereleases, so only real releases are
/// ever offered.
pub fn check(
    client: &reqwest::blocking::Client,
    current_version: &str,
) -> Result<Option<ReleaseInfo>, UpdateError> {
    let current = parse_version(current_version)?;
    let url = format!("https://api.github.com/repos/{REPO_OWNER}/{REPO_NAME}/releases/latest");

    let release: GhRelease = client
        .get(&url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()?
        .error_for_status()?
        .json()?;

    let latest = parse_version(release.tag_name.trim_start_matches('v'))?;
    if latest <= current {
        log::info!("update check: running {current} is current (latest {latest})");
        return Ok(None);
    }

    let asset = release
        .assets
        .iter()
        .find(|a| a.name.ends_with(WINDOWS_ASSET_SUFFIX));
    log::info!(
        "update check: {latest} available (running {current}); windows asset: {}",
        asset.is_some()
    );

    Ok(Some(ReleaseInfo {
        version: latest.to_string(),
        tag: release.tag_name,
        notes: release.body.unwrap_or_default(),
        html_url: release.html_url,
        asset_name: asset.map(|a| a.name.clone()).unwrap_or_default(),
        asset_url: asset
            .map(|a| a.browser_download_url.clone())
            .unwrap_or_default(),
    }))
}

/// Download the Windows asset to `<dir-of-running-exe>/<asset_name>.download`.
/// Staging on the same volume as the exe is required for the in-place swap in
/// [`apply`]. Returns the staged path.
pub fn download(
    client: &reqwest::blocking::Client,
    release: &ReleaseInfo,
) -> Result<PathBuf, UpdateError> {
    if !release.has_windows_asset() {
        return Err(UpdateError::NoWindowsAsset);
    }
    let staged = staged_path(&release.asset_name)?;

    let bytes = client
        .get(&release.asset_url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .timeout(DOWNLOAD_TIMEOUT)
        .send()?
        .error_for_status()?
        .bytes()?;

    std::fs::write(&staged, &bytes)?;
    log::info!(
        "staged update: {} ({} bytes)",
        staged.display(),
        bytes.len()
    );
    Ok(staged)
}

/// Replace the running executable with the (already verified) staged file,
/// remove the staging copy, and return the path the running exe occupied. Call
/// [`verify`] first, and pass the returned path to [`relaunch`].
///
/// The path is captured *before* the swap on purpose: once `self_replace` has
/// renamed the running image aside, `current_exe()` follows that rename (on
/// Windows `GetModuleFileNameW` resolves to the renamed backup), so reading it
/// afterward would point at the old binary, not the freshly-installed one.
pub fn apply(staged: &Path) -> Result<PathBuf, UpdateError> {
    // Capture the original path while the running image still lives there.
    let exe = current_exe()?;
    self_replace::self_replace(staged)?;
    // The swap copied the contents into place; the staging file is now spent.
    if let Err(e) = std::fs::remove_file(staged) {
        log::warn!("could not remove staged update {}: {e}", staged.display());
    }
    log::info!("update applied; running exe replaced");
    Ok(exe)
}

/// Launch the freshly-replaced executable at `exe` (the path [`apply`] returned)
/// and return so the caller can exit. Spawns with `CREATE_NO_WINDOW` on Windows
/// so no console flashes (the release binary is `windows_subsystem = "windows"`;
/// LL-G Rust HIGH), and passes [`RELAUNCHED_FLAG`] so the new process waits out
/// the single-instance lock this one still holds until it exits.
pub fn relaunch(exe: &Path) -> Result<(), UpdateError> {
    spawn_detached(exe)?;
    log::info!("relaunched {}", exe.display());
    Ok(())
}

fn staged_path(asset_name: &str) -> Result<PathBuf, UpdateError> {
    let exe = current_exe()?;
    let dir = exe
        .parent()
        .ok_or_else(|| UpdateError::CurrentExe("running exe has no parent directory".into()))?;
    let name = if asset_name.is_empty() {
        "hark-update".to_string()
    } else {
        asset_name.to_string()
    };
    Ok(dir.join(format!("{name}.download")))
}

fn current_exe() -> Result<PathBuf, UpdateError> {
    std::env::current_exe().map_err(|e| UpdateError::CurrentExe(e.to_string()))
}

fn parse_version(value: &str) -> Result<semver::Version, UpdateError> {
    semver::Version::parse(value.trim()).map_err(|source| UpdateError::Version {
        value: value.to_string(),
        source,
    })
}

#[cfg(windows)]
fn spawn_detached(exe: &Path) -> Result<(), UpdateError> {
    use std::os::windows::process::CommandExt;
    // CREATE_NO_WINDOW: no console window for the child (LL-G Rust HIGH #2).
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new(exe)
        .arg(RELAUNCHED_FLAG)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()?;
    Ok(())
}

#[cfg(not(windows))]
fn spawn_detached(exe: &Path) -> Result<(), UpdateError> {
    std::process::Command::new(exe)
        .arg(RELAUNCHED_FLAG)
        .spawn()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(newer: bool) -> ReleaseInfo {
        ReleaseInfo {
            version: if newer {
                "9.9.9".into()
            } else {
                "0.0.1".into()
            },
            tag: "v9.9.9".into(),
            notes: String::new(),
            html_url: String::new(),
            asset_name: "Hark-9.9.9-windows-x64.exe".into(),
            asset_url: "https://example.invalid/a.exe".into(),
        }
    }

    #[test]
    fn newer_tag_beats_current() {
        let cur = parse_version("0.13.5").unwrap();
        assert!(parse_version("0.13.6").unwrap() > cur);
        assert!(parse_version("0.14.0").unwrap() > cur);
        assert!(parse_version("1.0.0").unwrap() > cur);
    }

    #[test]
    fn equal_or_older_tag_is_not_an_update() {
        let cur = parse_version("0.13.5").unwrap();
        assert!(parse_version("0.13.5").unwrap() <= cur);
        assert!(parse_version("0.13.4").unwrap() <= cur);
        assert!(parse_version("0.9.9").unwrap() <= cur);
    }

    #[test]
    fn asset_picker_matches_windows_suffix() {
        let release = GhRelease {
            tag_name: "v0.14.0".into(),
            body: Some("notes".into()),
            html_url: "https://github.com/BoardPandas/Hark/releases/tag/v0.14.0".into(),
            assets: vec![
                GhAsset {
                    name: "Hark-0.14.0-macos.dmg".into(),
                    browser_download_url: "https://example.invalid/mac".into(),
                },
                GhAsset {
                    name: "Hark-0.14.0-windows-x64.exe".into(),
                    browser_download_url: "https://example.invalid/win".into(),
                },
            ],
        };
        let asset = release
            .assets
            .iter()
            .find(|a| a.name.ends_with(WINDOWS_ASSET_SUFFIX))
            .unwrap();
        assert_eq!(asset.browser_download_url, "https://example.invalid/win");
    }

    #[test]
    fn parse_of_latest_release_fixture() {
        // A trimmed real-shape releases/latest payload. Normal escaped string
        // (not raw): the release body starts with `### `, whose leading `"#`
        // would otherwise close an r#"..."# literal early.
        let json = "{\
            \"tag_name\": \"v0.14.0\",\
            \"body\": \"### Added\\n- Update checker\",\
            \"html_url\": \"https://github.com/BoardPandas/Hark/releases/tag/v0.14.0\",\
            \"assets\": [\
                {\"name\": \"Hark-0.14.0-windows-x64.exe\",\
                 \"browser_download_url\": \"https://example.invalid/win\"}\
            ]\
        }";
        let release: GhRelease = serde_json::from_str(json).unwrap();
        assert_eq!(release.tag_name, "v0.14.0");
        assert_eq!(release.assets.len(), 1);
        let latest = parse_version(release.tag_name.trim_start_matches('v')).unwrap();
        assert_eq!(latest.to_string(), "0.14.0");
    }

    #[test]
    fn has_windows_asset_reflects_url() {
        assert!(info(true).has_windows_asset());
        let mut no_asset = info(true);
        no_asset.asset_url = String::new();
        assert!(!no_asset.has_windows_asset());
    }
}

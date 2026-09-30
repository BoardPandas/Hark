//! In-app update state machine, owned by `HarkApp` and driven by both the
//! startup banner ([`crate::ui::shell`]) and the Settings section
//! ([`crate::ui::settings::updates`]). Keeping one instance means the banner and
//! the settings page always agree on where the update is in its lifecycle.
//!
//! Network and disk work run on detached worker threads and report back over an
//! `mpsc` channel, exactly like the test-connection flow
//! ([`crate::ui::settings::test`]); the UI thread only drains results in
//! [`Updater::poll`] and never blocks. The rare update check does not share the
//! pipeline's hot-path client; it builds one per operation via
//! `hark_stt::shared_client`.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

use hark_update::ReleaseInfo;

/// The current version reported by the running binary, kept in lockstep with
/// `package.json` (see the root `Cargo.toml`).
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Where each lifecycle stage renders from. `Available`/`Installing`/`Ready`
/// carry the release so the banner and settings can show its version + notes.
pub enum Phase {
    /// Nothing attempted yet this session.
    Idle,
    /// A check is in flight.
    Checking,
    /// Checked, and the running build is current.
    UpToDate,
    /// A newer release exists.
    Available(ReleaseInfo),
    /// Downloading + verifying the newer release.
    Installing(ReleaseInfo),
    /// Verified and staged; a restart will finish the update.
    Ready {
        release: ReleaseInfo,
        staged: StagedUpdate,
    },
    /// The last check or install failed; the message is user-facing.
    Failed(String),
}

/// Worker -> UI messages.
enum Msg {
    Checked {
        release: Result<Option<ReleaseInfo>, String>,
        install_supported: bool,
    },
    Restarted(Result<(), String>),
    /// download + verify finished; Ok carries the staged path.
    Installed(Result<StagedUpdate, String>),
}

/// Own the download through verification, UI cancellation and worker delivery.
/// Windows Setup takes ownership once launched; the Mac helper has already
/// copied the verified bundle before install returns, so its DMG can be removed.
pub struct StagedUpdate {
    path: PathBuf,
    remove_on_drop: bool,
}

impl StagedUpdate {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            remove_on_drop: true,
        }
    }
}

impl Drop for StagedUpdate {
    fn drop(&mut self) {
        if self.remove_on_drop {
            if let Err(error) = std::fs::remove_file(&self.path) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    log::warn!("Could not remove staged update: {error}");
                }
            }
        }
    }
}

pub struct Updater {
    phase: Phase,
    rx: Option<Receiver<Msg>>,
    /// The banner hides once the user dismisses it; the Settings section still
    /// shows the same state.
    banner_dismissed: bool,
    install_supported: bool,
}

impl Updater {
    pub fn new() -> Self {
        Updater {
            phase: Phase::Idle,
            rx: None,
            banner_dismissed: false,
            install_supported: false,
        }
    }

    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    pub fn current_version(&self) -> &'static str {
        CURRENT_VERSION
    }

    /// The release under consideration, if any stage carries one.
    pub fn release(&self) -> Option<&ReleaseInfo> {
        match &self.phase {
            Phase::Available(r) | Phase::Installing(r) | Phase::Ready { release: r, .. } => Some(r),
            _ => None,
        }
    }

    /// True while a background check or install is running (drives spinners and
    /// disables the buttons).
    pub fn is_busy(&self) -> bool {
        matches!(self.phase, Phase::Checking | Phase::Installing(_))
    }

    /// Whether a matching asset can be installed here. Capability is checked
    /// on the update worker, so rendering never probes signatures or disk.
    pub fn can_self_install(&self) -> bool {
        self.install_supported && self.release().is_some_and(|r| r.has_installable_asset())
    }

    /// Show the banner when an update is pending and the user has not dismissed
    /// it. `Idle`/`Checking`/`UpToDate`/`Failed` never raise the banner (the
    /// Settings section owns those).
    pub fn banner_visible(&self) -> bool {
        !self.banner_dismissed
            && matches!(
                self.phase,
                Phase::Available(_) | Phase::Installing(_) | Phase::Ready { .. }
            )
    }

    pub fn dismiss_banner(&mut self) {
        self.banner_dismissed = true;
    }

    /// Start a background check. No-op while one is already running.
    pub fn start_check(&mut self, ctx: &egui::Context) {
        if self.is_busy() {
            return;
        }
        let client = match hark_stt::shared_client() {
            Ok(c) => c,
            Err(e) => {
                self.phase = Phase::Failed(format!("cannot start update check: {e}"));
                return;
            }
        };
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("hark-update-check".to_string())
            .spawn(move || {
                let result =
                    hark_update::check(&client, CURRENT_VERSION).map_err(|e| e.to_string());
                let install_supported = hark_update::self_install_supported();
                let _ = tx.send(Msg::Checked {
                    release: result,
                    install_supported,
                });
                crate::app::wake_ui(&ctx);
            })
            .expect("spawning the update-check thread cannot fail");
        self.phase = Phase::Checking;
        self.rx = Some(rx);
    }

    /// Download + verify the pending release on a worker thread. Only valid from
    /// `Available` (or a `Failed` retry with a release still in hand).
    pub fn start_install(&mut self, ctx: &egui::Context) {
        if self.is_busy() {
            return;
        }
        let Some(release) = self.release().cloned() else {
            return;
        };
        let client = match hark_stt::shared_client() {
            Ok(c) => c,
            Err(e) => {
                self.phase = Phase::Failed(format!("cannot start download: {e}"));
                return;
            }
        };
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        let job = release.clone();
        std::thread::Builder::new()
            .name("hark-update-install".to_string())
            .spawn(move || {
                let result = (|| {
                    let staged = StagedUpdate::new(hark_update::download(&client, &job)?);
                    hark_update::verify(&staged.path)?;
                    Ok(staged)
                })()
                .map_err(|e: hark_update::UpdateError| e.to_string());
                let _ = tx.send(Msg::Installed(result));
                crate::app::wake_ui(&ctx);
            })
            .expect("spawning the update-install thread cannot fail");
        self.phase = Phase::Installing(release);
        self.rx = Some(rx);
    }

    /// Prepare installation on a worker. Once it is ready to replace this
    /// process, the UI exits on the completion message; failures stay visible.
    ///
    /// Exiting immediately is not tidiness, it is the contract: Setup replaces
    /// `Hark.exe`, and Windows will not overwrite a running image. `hark.iss`
    /// sets `CloseApplications=yes` so the Restart Manager would close us
    /// anyway, but waiting to be killed turns a two-second update into a stall.
    /// Setup starts Hark again itself (`/relaunch=yes`), so nothing here needs
    /// to survive to do it.
    pub fn restart(&mut self, ctx: &egui::Context) {
        if !matches!(self.phase, Phase::Ready { .. }) {
            return;
        }
        let Phase::Ready { staged, release } = std::mem::replace(&mut self.phase, Phase::Idle)
        else {
            unreachable!()
        };
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        match std::thread::Builder::new()
            .name("hark-update-restart".into())
            .spawn(move || {
                // Mac staging mounts/copies/verifies a complete app bundle.
                // Keep that work off the UI thread, including on retry.
                let result = hark_update::install(&staged.path).map_err(|e| e.to_string());
                #[cfg(windows)]
                let staged = {
                    let mut staged = staged;
                    if result.is_ok() {
                        staged.remove_on_drop = false;
                    }
                    staged
                };
                drop(staged);
                let _ = tx.send(Msg::Restarted(result));
                crate::app::wake_ui(&ctx);
            }) {
            Ok(_) => {
                self.phase = Phase::Installing(release);
                self.rx = Some(rx);
            }
            Err(error) => {
                self.phase = Phase::Failed(format!("Could not start installation: {error}"))
            }
        }
    }

    /// Drain any finished background work. Called every frame from `App::logic`.
    pub fn poll(&mut self) {
        let Some(rx) = &self.rx else {
            return;
        };
        let msg = match rx.try_recv() {
            Ok(msg) => msg,
            Err(_) => return,
        };
        self.rx = None;
        if let Msg::Checked {
            install_supported, ..
        } = &msg
        {
            self.install_supported = *install_supported;
        }
        match msg {
            Msg::Restarted(Ok(())) => std::process::exit(0),
            Msg::Restarted(Err(error)) => {
                self.phase = Phase::Failed(format!("Could not install update: {error}"))
            }
            Msg::Checked {
                release: Ok(Some(release)),
                ..
            } => {
                // A fresh check un-dismisses the banner for a real update.
                self.banner_dismissed = false;
                self.phase = Phase::Available(release);
            }
            Msg::Checked {
                release: Ok(None), ..
            } => self.phase = Phase::UpToDate,
            Msg::Checked {
                release: Err(e), ..
            } => self.phase = Phase::Failed(e),
            Msg::Installed(Ok(staged)) => {
                let release = match std::mem::replace(&mut self.phase, Phase::Idle) {
                    Phase::Installing(r) => r,
                    other => {
                        // Shouldn't happen, but keep the release if we still have one.
                        self.phase = other;
                        return;
                    }
                };
                self.phase = Phase::Ready { release, staged };
            }
            Msg::Installed(Err(e)) => self.phase = Phase::Failed(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn download() -> (StagedUpdate, PathBuf) {
        let file = tempfile::NamedTempFile::new().unwrap();
        let (_, path) = file.keep().unwrap();
        (StagedUpdate::new(path.clone()), path)
    }

    #[test]
    fn closing_ui_before_worker_delivery_removes_the_download() {
        let (staged, path) = download();
        let (tx, rx) = mpsc::channel();
        drop(rx);
        assert!(tx.send(Msg::Installed(Ok(staged))).is_err());
        assert!(!path.exists());
    }

    #[test]
    fn replacing_or_dropping_ready_phase_removes_the_download() {
        let (staged, path) = download();
        let mut updater = Updater::new();
        updater.phase = Phase::Ready {
            release: ReleaseInfo {
                version: "99.0.0".into(),
                tag: "v99.0.0".into(),
                notes: String::new(),
                html_url: String::new(),
                asset_name: String::new(),
                asset_url: String::new(),
            },
            staged,
        };
        assert!(path.exists());
        updater.phase = Phase::Checking;
        assert!(!path.exists());
        let (staged, path) = download();
        drop(Msg::Installed(Ok(staged)));
        assert!(!path.exists());
    }
}

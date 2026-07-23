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
        staged: PathBuf,
    },
    /// The last check or install failed; the message is user-facing.
    Failed(String),
}

/// Worker -> UI messages.
enum Msg {
    Checked(Result<Option<ReleaseInfo>, String>),
    /// download + verify finished; Ok carries the staged path.
    Installed(Result<PathBuf, String>),
}

pub struct Updater {
    phase: Phase,
    rx: Option<Receiver<Msg>>,
    /// The banner hides once the user dismisses it; the Settings section still
    /// shows the same state.
    banner_dismissed: bool,
}

impl Updater {
    pub fn new() -> Self {
        Updater {
            phase: Phase::Idle,
            rx: None,
            banner_dismissed: false,
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

    /// Whether the newer release can be installed in place here (Windows with a
    /// matching asset); otherwise the UI offers "View release" instead.
    pub fn can_self_install(&self) -> bool {
        cfg!(windows) && self.release().is_some_and(|r| r.has_windows_asset())
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
                let _ = tx.send(Msg::Checked(result));
                ctx.request_repaint();
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
                    let staged = hark_update::download(&client, &job)?;
                    hark_update::verify(&staged)?;
                    Ok(staged)
                })()
                .map_err(|e: hark_update::UpdateError| e.to_string());
                let _ = tx.send(Msg::Installed(result));
                ctx.request_repaint();
            })
            .expect("spawning the update-install thread cannot fail");
        self.phase = Phase::Installing(release);
        self.rx = Some(rx);
    }

    /// Apply the staged update and relaunch. On success the process exits and
    /// never returns; a failure lands back in `Failed`.
    pub fn restart(&mut self) {
        let Phase::Ready { staged, .. } = &self.phase else {
            return;
        };
        let staged = staged.clone();
        let exe = match hark_update::apply(&staged) {
            Ok(exe) => exe,
            Err(e) => {
                self.phase = Phase::Failed(format!("could not apply the update: {e}"));
                return;
            }
        };
        if let Err(e) = hark_update::relaunch(&exe) {
            // The exe is already swapped; the next manual launch is the new
            // version. Surface why the auto-relaunch did not happen.
            self.phase = Phase::Failed(format!(
                "update installed, but relaunch failed ({e}). Reopen Hark to finish."
            ));
            return;
        }
        std::process::exit(0);
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
        match msg {
            Msg::Checked(Ok(Some(release))) => {
                // A fresh check un-dismisses the banner for a real update.
                self.banner_dismissed = false;
                self.phase = Phase::Available(release);
            }
            Msg::Checked(Ok(None)) => self.phase = Phase::UpToDate,
            Msg::Checked(Err(e)) => self.phase = Phase::Failed(e),
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

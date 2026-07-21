//! Model download lifecycle as seen from the UI.
//!
//! Same shape as [`crate::update::Updater`]: the network and disk work runs on
//! a detached worker thread and reports back over an `mpsc` channel, which the
//! UI thread drains in [`ModelDownload::poll`]. The UI thread never blocks and
//! never touches the filesystem in a paint pass.
//!
//! Two differences from the updater, both forced by size: the transfer is
//! ~670 MB rather than a few MB, so progress is reported continuously and the
//! whole thing is cancellable and resumable.

use hark_local_stt::{ModelSpec, ModelStatus, Progress};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;

/// Where the download is in its lifecycle.
pub enum Phase {
    /// Nothing running. The model may or may not be on disk; ask `status()`.
    Idle,
    Downloading {
        done_bytes: u64,
        total_bytes: u64,
    },
    /// Finished this session (or found already complete).
    Done,
    /// Cancelled by the user. Partial files are kept for resume.
    Cancelled,
    Failed(String),
}

/// Worker -> UI messages.
enum Msg {
    Progress(u64, u64),
    Finished(Result<(), String>),
}

pub struct ModelDownload {
    phase: Phase,
    rx: Option<Receiver<Msg>>,
    cancel: Arc<AtomicBool>,
    spec: &'static ModelSpec,
    /// Cached disk status. Refreshed on demand rather than per frame, since
    /// it stats four files.
    status: ModelStatus,
}

impl ModelDownload {
    pub fn new(spec: &'static ModelSpec) -> Self {
        ModelDownload {
            phase: Phase::Idle,
            rx: None,
            cancel: Arc::new(AtomicBool::new(false)),
            spec,
            status: spec.status(),
        }
    }

    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    pub fn spec(&self) -> &'static ModelSpec {
        self.spec
    }

    pub fn status(&self) -> ModelStatus {
        self.status
    }

    pub fn is_busy(&self) -> bool {
        matches!(self.phase, Phase::Downloading { .. })
    }

    /// Re-stat the model directory. Call after a download, a delete, or when
    /// the settings page is first shown — not every frame.
    pub fn refresh_status(&mut self) {
        self.status = self.spec.status();
    }

    /// Begin (or resume) the download on a worker thread. No-op while one is
    /// already running.
    pub fn start(&mut self, ctx: &egui::Context) {
        if self.is_busy() {
            return;
        }
        let client = match hark_stt::shared_client() {
            Ok(c) => c,
            Err(e) => {
                self.phase = Phase::Failed(format!("cannot start the download: {e}"));
                return;
            }
        };
        let dir = match self.spec.dir() {
            Ok(d) => d,
            Err(e) => {
                self.phase = Phase::Failed(e.to_string());
                return;
            }
        };

        // A fresh flag per run: a cancel from a previous run must not
        // immediately kill this one.
        self.cancel = Arc::new(AtomicBool::new(false));
        let cancel = self.cancel.clone();
        let spec = self.spec;
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        self.phase = Phase::Downloading {
            done_bytes: 0,
            total_bytes: spec.total_bytes(),
        };
        self.rx = Some(rx);

        std::thread::Builder::new()
            .name("hark-model-download".to_string())
            .spawn(move || {
                let mut on_progress = |p: Progress| {
                    let _ = tx.send(Msg::Progress(p.done_bytes, p.total_bytes));
                    // Wake the UI thread; the downloader already throttles
                    // these to ~10/s so this cannot become a repaint storm.
                    ctx.request_repaint();
                };
                let result =
                    hark_local_stt::download(spec, &dir, &client, &cancel, &mut on_progress)
                        .map_err(|e| e.to_string());
                let _ = tx.send(Msg::Finished(result));
                ctx.request_repaint();
            })
            .expect("spawning the download thread cannot fail");
    }

    /// Ask the worker to stop. Partial files stay on disk so a later start
    /// resumes rather than restarting ~670 MB.
    pub fn cancel(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Delete the downloaded weights. Refuses while a download is running,
    /// so the worker can never be writing into a directory being removed.
    pub fn delete(&mut self) -> Result<(), String> {
        if self.is_busy() {
            return Err("Cancel the download before deleting the model.".to_string());
        }
        let dir = self.spec.dir().map_err(|e| e.to_string())?;
        hark_local_stt::remove(&dir).map_err(|e| e.to_string())?;
        self.phase = Phase::Idle;
        self.refresh_status();
        Ok(())
    }

    /// Drain worker messages. Called once per frame from the settings page.
    pub fn poll(&mut self) {
        let Some(rx) = &self.rx else {
            return;
        };
        let mut finished = false;
        while let Ok(msg) = rx.try_recv() {
            match msg {
                Msg::Progress(done, total) => {
                    self.phase = Phase::Downloading {
                        done_bytes: done,
                        total_bytes: total,
                    };
                }
                Msg::Finished(Ok(())) => {
                    self.phase = Phase::Done;
                    finished = true;
                }
                Msg::Finished(Err(e)) => {
                    // Cancellation is a user action, not a failure to report
                    // in red.
                    self.phase = if e.contains("cancelled") {
                        Phase::Cancelled
                    } else {
                        Phase::Failed(e)
                    };
                    finished = true;
                }
            }
        }
        if finished {
            self.rx = None;
            self.refresh_status();
        }
    }

    /// Fraction complete for the progress bar, or `None` when not downloading.
    pub fn fraction(&self) -> Option<f32> {
        match self.phase {
            Phase::Downloading {
                done_bytes,
                total_bytes,
            } if total_bytes > 0 => Some((done_bytes as f64 / total_bytes as f64) as f32),
            _ => None,
        }
    }
}

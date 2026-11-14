//! Startup recovery: repair what a crash left open in the meetings folder,
//! and finish again any meeting whose finishing was cut short.

use super::Coordinator;
use crate::meeting::finish::{self, FinishJob, FINISHING_MARKER};
use crate::meeting::pending;
use hark_meeting::{SessionState, Transcript};
use std::sync::{Arc, Mutex};

impl Coordinator {
    /// Startup: repair spools a crash left open, finish or undo an
    /// interrupted compression, then let the storage cap run.
    pub(super) fn recover(&self) {
        match hark_audio::spool::recover_all(&self.meetings_dir) {
            Ok(results) => {
                for (path, outcome) in results {
                    match outcome {
                        Ok(hark_audio::spool::Recovery::Repaired { samples }) => {
                            log::info!(
                                "meeting spool repaired: {} ({samples} samples)",
                                path.display()
                            )
                        }
                        Ok(_) => {}
                        Err(e) => log::warn!("meeting spool {} not checked: {e}", path.display()),
                    }
                }
            }
            Err(e) => log::warn!("meeting spool recovery skipped: {e}"),
        }
        if let Ok(entries) = std::fs::read_dir(&self.meetings_dir) {
            for entry in entries.flatten() {
                if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                    continue;
                }
                match hark_audio::recover_meeting_dir(&entry.path()) {
                    Ok(hark_audio::RecoverAction::None) => {}
                    Ok(action) => log::info!(
                        "meeting archive recovery in {}: {}",
                        entry.path().display(),
                        recover_label(&action)
                    ),
                    Err(e) => log::warn!(
                        "meeting archive recovery failed in {}: {e}",
                        entry.path().display()
                    ),
                }
                self.refinish(&entry.path());
                if entry.path().join(pending::MARKER).exists() {
                    self.retries.resume(&entry.file_name().to_string_lossy());
                }
            }
        }
        self.enforce_cap();
    }

    /// Finish again a meeting whose finishing was cut short (its folder still
    /// has the marker). The live transcript is not available here, so notes
    /// are written only when the final pass produces a transcript; the
    /// recording, the archive and the storage cap are handled either way.
    fn refinish(&self, dir: &std::path::Path) {
        if !dir.join(FINISHING_MARKER).exists() {
            return;
        }
        let Some(id) = dir.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            return;
        };
        log::info!("meeting {id}: finishing was interrupted; finishing it now");
        if let Ok(mut p) = self.protected.lock() {
            p.push(id.clone());
        }
        finish::spawn(FinishJob {
            recorded: super::super::recorder::Recorded {
                // The archive may already have replaced the spools: the
                // finisher recorded the length in the marker for that case.
                me_samples: super::super::recorder::spool_samples(
                    &dir.join(hark_audio::spool::ME_FILE),
                )
                .max(finish::marked_samples(dir)),
                them_samples: super::super::recorder::spool_samples(
                    &dir.join(hark_audio::spool::THEM_FILE),
                ),
                id,
                dir: dir.to_path_buf(),
            },
            live: None,
            transcript: Arc::new(Mutex::new(Transcript::new())),
            settings: Box::new(self.settings.clone()),
            events: self.events.clone(),
            protected: self.protected.clone(),
            session: SessionState::Finalizing,
            retries: self.retries.queue(),
        });
    }
}

fn recover_label(action: &hark_audio::RecoverAction) -> &'static str {
    match action {
        hark_audio::RecoverAction::None => "nothing to do",
        hark_audio::RecoverAction::RemovedPartial => "removed a partial archive",
        hark_audio::RecoverAction::FinishedCompression => "finished an interrupted compression",
        hark_audio::RecoverAction::DiscardedBadArchive => "discarded a bad archive, kept the WAVs",
    }
}

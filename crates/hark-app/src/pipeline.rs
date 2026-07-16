//! Pipeline lifecycle as seen from the UI: start/stop, the advisory event
//! stream, and the status the footer renders. The pipeline itself runs on
//! worker threads (hark-pipeline); this module never blocks the UI thread.

use hark_config::Settings;
use hark_pipeline::{FailStage, PipelineEvent, PipelineHandle};
use std::sync::mpsc::{self, Receiver};

/// What the status footer shows. There is no silent dead state: the pipeline
/// is either listening (and says for what), mid-dictation, or stopped with a
/// visible cause.
#[derive(Clone, PartialEq, Eq)]
pub enum PipelineStatus {
    /// Running; waiting for the chord.
    Idle,
    /// Chord held; capturing audio.
    Recording,
    /// Chord released; request in flight.
    Processing,
    /// Running, but the last dictation failed. Sticky until the next
    /// dictation starts.
    Errored { detail: String, key_related: bool },
    /// Not running (no key, startup failure, or config error).
    Stopped { detail: String, key_related: bool },
}

/// Owns the `PipelineHandle` and the UI-side end of the event stream.
pub struct PipelineController {
    handle: Option<PipelineHandle>,
    events: Option<Receiver<PipelineEvent>>,
    status: PipelineStatus,
}

impl PipelineController {
    pub fn new() -> Self {
        PipelineController {
            handle: None,
            events: None,
            status: PipelineStatus::Stopped {
                detail: "Not started".to_string(),
                key_related: false,
            },
        }
    }

    pub fn status(&self) -> &PipelineStatus {
        &self.status
    }

    pub fn is_running(&self) -> bool {
        self.handle.is_some()
    }

    /// Resolve the key and start the pipeline. On failure the app keeps
    /// running with a visible cause in the footer.
    pub fn start(&mut self, settings: &Settings, ctx: &egui::Context) {
        self.stop();
        let provider = settings.provider.kind.label();
        let api_key = match hark_keychain::resolve_key(provider) {
            Ok(k) => k,
            Err(e) => {
                // Missing/unreadable key: by construction key-related.
                self.status = PipelineStatus::Stopped {
                    detail: e.to_string(),
                    key_related: true,
                };
                return;
            }
        };
        let (tx, rx) = mpsc::channel();
        match hark_pipeline::run(settings, api_key, tx) {
            Ok(handle) => {
                self.handle = Some(handle);
                self.events = Some(spawn_repaint_pump(rx, ctx.clone()));
                self.status = PipelineStatus::Idle;
            }
            Err(e) => {
                self.status = PipelineStatus::Stopped {
                    detail: e.to_string(),
                    key_related: false,
                };
            }
        }
    }

    /// Drop the handle; its Drop stops hook -> worker -> capture in order.
    /// The worker's sender drops with it, which ends the repaint pump.
    pub fn stop(&mut self) {
        self.handle = None;
        self.events = None;
    }

    /// Stop (if running) and surface a non-key cause in the footer, e.g. a
    /// config file that failed to parse.
    pub fn mark_stopped(&mut self, detail: String) {
        self.stop();
        self.status = PipelineStatus::Stopped {
            detail,
            key_related: false,
        };
    }

    /// Drain pending events; called from `App::logic` every frame. Never
    /// blocks: `try_recv` only. An `Errored` status is naturally sticky: it
    /// stands until the next event (the next dictation) replaces it.
    pub fn drain_events(&mut self) {
        let Some(rx) = &self.events else { return };
        while let Ok(event) = rx.try_recv() {
            self.status = next_status(event);
        }
    }
}

/// Pure event -> status mapping (the testable seam).
fn next_status(event: PipelineEvent) -> PipelineStatus {
    match event {
        PipelineEvent::Recording => PipelineStatus::Recording,
        PipelineEvent::Processing => PipelineStatus::Processing,
        // CP4 forwards the record to the storage thread; for now reaching
        // Idle is the whole story the footer needs.
        PipelineEvent::Injected(_) => PipelineStatus::Idle,
        PipelineEvent::Failed { stage, detail } => match stage {
            // Informational ends (nothing was heard / nothing came back):
            // back to listening, not an error banner.
            FailStage::Gated | FailStage::EmptyTranscript => PipelineStatus::Idle,
            FailStage::Audio | FailStage::Transcribe | FailStage::Inject => {
                PipelineStatus::Errored {
                    // Crude but effective: auth errors say "check your API
                    // key", keychain errors name the keychain.
                    key_related: detail.to_ascii_lowercase().contains("key"),
                    detail,
                }
            }
        },
    }
}

/// Forward pipeline events onto a UI-side channel, waking the event loop per
/// event (`request_repaint` is the sanctioned cross-thread wake-up). Exits
/// when the pipeline drops its sender or the UI drops its receiver; zero
/// idle cost either way.
fn spawn_repaint_pump(rx: Receiver<PipelineEvent>, ctx: egui::Context) -> Receiver<PipelineEvent> {
    let (tx, ui_rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("hark-ui-event-pump".to_string())
        .spawn(move || {
            while let Ok(event) = rx.recv() {
                if tx.send(event).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        })
        .expect("spawning the event pump thread cannot fail");
    ui_rx
}

#[cfg(test)]
mod tests {
    use super::*;
    use hark_pipeline::DictationRecord;

    fn record() -> DictationRecord {
        DictationRecord {
            raw_text: "raw".to_string(),
            final_text: "final".to_string(),
            voice: "clean".to_string(),
            stt_provider: "deepgram".to_string(),
            stt_model: "nova-3".to_string(),
            cleanup_model: None,
            audio_ms: 900,
            stt_ms: 300,
            cleanup_ms: None,
            total_ms: 450,
        }
    }

    #[test]
    fn happy_path_walks_recording_processing_idle() {
        assert!(matches!(
            next_status(PipelineEvent::Recording),
            PipelineStatus::Recording
        ));
        assert!(matches!(
            next_status(PipelineEvent::Processing),
            PipelineStatus::Processing
        ));
        assert!(matches!(
            next_status(PipelineEvent::Injected(record())),
            PipelineStatus::Idle
        ));
    }

    #[test]
    fn gated_and_empty_transcripts_return_to_idle_not_error() {
        for stage in [FailStage::Gated, FailStage::EmptyTranscript] {
            let s = next_status(PipelineEvent::Failed {
                stage,
                detail: "informational".to_string(),
            });
            assert!(matches!(s, PipelineStatus::Idle), "stage {stage:?}");
        }
    }

    #[test]
    fn auth_failures_are_key_related_errors() {
        let s = next_status(PipelineEvent::Failed {
            stage: FailStage::Transcribe,
            detail: "authentication rejected by deepgram: check your API key".to_string(),
        });
        match s {
            PipelineStatus::Errored {
                key_related,
                detail,
            } => {
                assert!(key_related);
                assert!(detail.contains("deepgram"));
            }
            _ => panic!("expected Errored"),
        }
    }

    #[test]
    fn network_failures_are_errors_without_the_settings_jump() {
        let s = next_status(PipelineEvent::Failed {
            stage: FailStage::Transcribe,
            detail: "request to deepgram timed out after 15000 ms".to_string(),
        });
        assert!(matches!(
            s,
            PipelineStatus::Errored {
                key_related: false,
                ..
            }
        ));
    }

    #[test]
    fn the_next_dictation_replaces_a_sticky_error() {
        // Errored has no special-case handling: any newer event wins.
        assert!(matches!(
            next_status(PipelineEvent::Recording),
            PipelineStatus::Recording
        ));
    }
}

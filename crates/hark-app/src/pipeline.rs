//! Pipeline lifecycle as seen from the UI: start/stop, the advisory event
//! stream, and the status the footer renders. The pipeline itself runs on
//! worker threads (hark-pipeline); this module never blocks the UI thread.

use crate::storage::{self, RecordPolicy, StorageCmd};
use hark_config::Settings;
use hark_pipeline::{FailStage, LevelMeter, PipelineEvent, PipelineHandle};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;

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
    /// Reading the on-device model into RAM. Only the first dictation after a
    /// pipeline start sees this (the engine then stays resident), but it lasts
    /// seconds, so it gets its own state rather than looking like a hang.
    LoadingModel,
    /// Running, but the last dictation failed. Sticky until the next
    /// dictation starts.
    Errored { detail: String, key_related: bool },
    /// Running, and the last dictation ended in something worth saying out
    /// loud but which is not a failure: we captured nothing loud enough to be
    /// speech. Distinct from `Errored` because nothing is broken and the tray
    /// must not go red — but distinct from `Idle` because silently doing
    /// nothing is precisely the experience that leaves users convinced the app
    /// cannot hear them. Sticky until the next dictation starts.
    Hint { detail: String },
    /// Not running (no key, startup failure, or config error).
    Stopped { detail: String, key_related: bool },
}

/// Owns the `PipelineHandle` and the UI-side end of the event stream.
pub struct PipelineController {
    handle: Option<PipelineHandle>,
    events: Option<Receiver<PipelineEvent>>,
    status: PipelineStatus,
    /// Successful dictations this app session (survives restarts; the Get
    /// Started card retires on the first one).
    injected: u64,
    /// Command lane to the storage thread; `None` when storage failed to
    /// open (dictation still works, nothing persists).
    storage: Option<Sender<StorageCmd>>,
    /// Live mic-level meter for the recording overlay's audio-reactive pulse;
    /// `None` while the pipeline is stopped. Read every frame the overlay
    /// paints, so it is a cheap `Arc`, not a per-frame lookup.
    level: Option<Arc<LevelMeter>>,
}

impl PipelineController {
    pub fn new(storage: Option<Sender<StorageCmd>>) -> Self {
        PipelineController {
            handle: None,
            events: None,
            status: PipelineStatus::Stopped {
                detail: "Not started".to_string(),
                key_related: false,
            },
            injected: 0,
            storage,
            level: None,
        }
    }

    pub fn status(&self) -> &PipelineStatus {
        &self.status
    }

    pub fn is_running(&self) -> bool {
        self.handle.is_some()
    }

    /// The live mic-level meter, or `None` while the pipeline is stopped. The
    /// recording overlay reads it every frame to drive its audio-reactive
    /// pulse; the `Arc` is cheap to clone into the overlay's paint closure.
    pub fn level_meter(&self) -> Option<Arc<LevelMeter>> {
        self.level.clone()
    }

    pub fn injected_count(&self) -> u64 {
        self.injected
    }

    /// Resolve the key and start the pipeline. On failure the app keeps
    /// running with a visible cause in the footer.
    pub fn start(&mut self, settings: &Settings, ctx: &egui::Context) {
        self.stop();
        // Every (re)start is a policy application point: retention changed
        // in a save takes effect now, and app startup prunes old entries
        // before the first dictation.
        if let Some(tx) = &self.storage {
            let _ = tx.send(StorageCmd::Prune(storage::retention(settings)));
        }
        let provider = settings.provider.kind.label();
        // Primary-mode local STT contacts no provider, so a missing key must
        // not stop the pipeline — running keyless is the entire point of that
        // mode. Every other mode still needs one.
        let api_key = match hark_keychain::resolve_key(provider) {
            Ok(k) => k,
            Err(_) if !settings.local_stt.mode.uses_cloud() => String::new(),
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
                // The record policy travels with this pipeline run: settings
                // changes restart the pipeline, so the pump never needs a
                // shared, mutable view of the config.
                let tee = self
                    .storage
                    .clone()
                    .map(|tx| (tx, storage::record_policy(settings)));
                self.level = Some(handle.level_meter());
                self.handle = Some(handle);
                self.events = Some(spawn_repaint_pump(rx, ctx.clone(), tee));
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
        self.level = None;
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
            if matches!(event, PipelineEvent::Injected(_)) {
                self.injected += 1;
            }
            self.status = next_status(event);
        }
    }
}

/// Pure event -> status mapping (the testable seam).
fn next_status(event: PipelineEvent) -> PipelineStatus {
    match event {
        PipelineEvent::Recording => PipelineStatus::Recording,
        PipelineEvent::Processing => PipelineStatus::Processing,
        PipelineEvent::LoadingLocalModel => PipelineStatus::LoadingModel,
        // CP4 forwards the record to the storage thread; for now reaching
        // Idle is the whole story the footer needs.
        PipelineEvent::Injected(_) => PipelineStatus::Idle,
        PipelineEvent::Failed { stage, detail } => match stage {
            // A tap on the chord is the user's own doing and needs no reply;
            // an empty transcript means the provider heard nothing to write.
            FailStage::GatedTooShort | FailStage::EmptyTranscript => PipelineStatus::Idle,
            // But "we captured nothing loud enough to be speech" is worth
            // saying, with the way to fix it one click away: the usual cause
            // is the wrong input device selected, or one turned down.
            FailStage::GatedTooQuiet => PipelineStatus::Hint {
                detail: "Didn't catch that. Check your microphone.".to_string(),
            },
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
/// event (`request_repaint` is the sanctioned cross-thread wake-up), and tee
/// `Injected` records to the storage thread. Both sends are non-blocking
/// (unbounded channels), so the UI lane never waits on storage or vice
/// versa. The injection already happened before the event was emitted, so a
/// DB write can never precede it (CP4 acceptance). Exits when the pipeline
/// drops its sender or the UI drops its receiver; zero idle cost either way.
fn spawn_repaint_pump(
    rx: Receiver<PipelineEvent>,
    ctx: egui::Context,
    storage: Option<(Sender<StorageCmd>, RecordPolicy)>,
) -> Receiver<PipelineEvent> {
    let (tx, ui_rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("hark-ui-event-pump".to_string())
        .spawn(move || {
            while let Ok(event) = rx.recv() {
                if let (PipelineEvent::Injected(record), Some((storage_tx, policy))) =
                    (&event, &storage)
                {
                    let _ = storage_tx.send(StorageCmd::Record {
                        record: Box::new(record.clone()),
                        capture: policy.capture,
                        retention: policy.retention,
                    });
                }
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
    fn misfires_and_empty_transcripts_return_to_idle_not_error() {
        for stage in [FailStage::GatedTooShort, FailStage::EmptyTranscript] {
            let s = next_status(PipelineEvent::Failed {
                stage,
                detail: "informational".to_string(),
            });
            assert!(matches!(s, PipelineStatus::Idle), "stage {stage:?}");
        }
    }

    /// "We heard nothing" must neither vanish (the old behaviour, which left
    /// users unable to tell a quiet mic from a broken app) nor raise an error
    /// banner: it is a hint, with the settings jump attached.
    #[test]
    fn a_too_quiet_capture_hints_rather_than_erroring_or_vanishing() {
        let s = next_status(PipelineEvent::Failed {
            stage: FailStage::GatedTooQuiet,
            detail: "no speech detected".to_string(),
        });
        match s {
            PipelineStatus::Hint { detail } => {
                assert!(detail.contains("microphone"), "unhelpful hint: {detail}")
            }
            other => panic!("expected a hint, got {:?}", std::mem::discriminant(&other)),
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
    fn drain_counts_injections_and_stop_does_not_reset_the_count() {
        let (tx, rx) = mpsc::channel();
        let mut controller = PipelineController::new(None);
        controller.events = Some(rx);
        tx.send(PipelineEvent::Injected(record())).unwrap();
        tx.send(PipelineEvent::Recording).unwrap();
        tx.send(PipelineEvent::Injected(record())).unwrap();
        controller.drain_events();
        assert_eq!(controller.injected_count(), 2);
        // The Get Started card keys off "ever dictated this session";
        // a settings-save restart must not forget that.
        controller.stop();
        assert_eq!(controller.injected_count(), 2);
    }

    #[test]
    fn the_next_dictation_replaces_a_sticky_error() {
        // Errored has no special-case handling: any newer event wins.
        assert!(matches!(
            next_status(PipelineEvent::Recording),
            PipelineStatus::Recording
        ));
    }

    #[test]
    fn pump_tees_injected_records_to_storage_and_forwards_every_event() {
        let policy = RecordPolicy {
            capture: false,
            retention: hark_store::Retention {
                max_entries: 7,
                max_age_days: 3,
            },
        };
        let (pipeline_tx, pipeline_rx) = mpsc::channel();
        let (storage_tx, storage_rx) = mpsc::channel();
        let ui_rx = spawn_repaint_pump(
            pipeline_rx,
            egui::Context::default(),
            Some((storage_tx, policy)),
        );

        pipeline_tx.send(PipelineEvent::Recording).unwrap();
        pipeline_tx.send(PipelineEvent::Processing).unwrap();
        pipeline_tx.send(PipelineEvent::Injected(record())).unwrap();
        drop(pipeline_tx); // pump drains, then exits

        let ui_events: Vec<_> = ui_rx.iter().collect();
        assert_eq!(ui_events.len(), 3, "the UI lane sees every event");

        let teed: Vec<_> = storage_rx.iter().collect();
        assert_eq!(teed.len(), 1, "only Injected reaches storage");
        match &teed[0] {
            StorageCmd::Record {
                record,
                capture,
                retention,
            } => {
                assert_eq!(record.final_text, "final");
                assert!(!capture, "the pump forwards the run's policy");
                assert_eq!(retention.max_entries, 7);
                assert_eq!(retention.max_age_days, 3);
            }
            _ => panic!("expected a Record command"),
        }
    }

    #[test]
    fn pump_without_storage_still_forwards_events() {
        let (pipeline_tx, pipeline_rx) = mpsc::channel();
        let ui_rx = spawn_repaint_pump(pipeline_rx, egui::Context::default(), None);
        pipeline_tx.send(PipelineEvent::Injected(record())).unwrap();
        drop(pipeline_tx);
        assert_eq!(ui_rx.iter().count(), 1);
    }
}

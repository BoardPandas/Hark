use super::*;
use hark_pipeline::DictationRecord;

/// The accounts `carry_key_with` asked to copy, if it asked at all.
fn carry_asked(toml: &str, migrations: hark_config::Migrations) -> Option<(String, String)> {
    let settings = Settings::from_toml(toml).unwrap();
    let asked = std::cell::RefCell::new(None);
    carry_key_with(&settings, &migrations, |from, to| {
        *asked.borrow_mut() = Some((from.to_string(), to.to_string()));
        Ok(hark_keychain::CopyOutcome::SourceMissing)
    });
    asked.into_inner()
}

#[test]
fn a_groq_key_carry_is_retried_on_launches_after_the_migration() {
    // The migrating launch's keychain failed; the file is already v7 and
    // reports no migration. The key must still be carried.
    let groq = "[provider]\nkind = \"openai-compatible\"\n\
                    base_url = \"https://api.groq.com/openai/v1\"\nmodel = \"whisper-large-v3-turbo\"";
    let expected = Some(("groq".to_string(), "openai-compatible".to_string()));
    assert_eq!(
        carry_asked(groq, hark_config::Migrations::default()),
        expected
    );
    // A just-converted proxy endpoint is carried on its migrating launch.
    let proxy =
        "[provider]\nkind = \"openai-compatible\"\nbase_url = \"https://groq-proxy.example/v1\"";
    let migrated = hark_config::Migrations {
        stt_from_groq: true,
        cleanup_from_groq: true,
    };
    assert_eq!(carry_asked(proxy, migrated), expected);
}

#[test]
fn no_keychain_is_touched_when_no_groq_key_is_owed() {
    for toml in [
        "",
        "[provider]\nkind = \"openai\"",
        "[provider]\nkind = \"openai-compatible\"\nbase_url = \"http://localhost:8000/v1\"",
    ] {
        assert_eq!(
            carry_asked(toml, hark_config::Migrations::default()),
            None,
            "{toml:?}"
        );
    }
}

#[test]
fn a_failed_carry_is_logged_not_fatal() {
    let settings = Settings::from_toml(
        "[provider]\nkind = \"openai-compatible\"\nbase_url = \"https://api.groq.com/openai/v1\"",
    )
    .unwrap();
    carry_key_with(&settings, &hark_config::Migrations::default(), |from, _| {
        Err(hark_keychain::KeyError::Backend {
            account: from.to_string(),
            detail: "locked".to_string(),
        })
    });
}

#[test]
fn meeting_toggles_survive_an_absent_dictation_worker() {
    use hark_hotkey::{PttEvent, ShortcutEvent};
    let (tx, rx) = mpsc::channel();
    let (ptt_tx, ptt_rx) = mpsc::channel();
    drop(ptt_rx); // Missing provider key: no dictation worker exists.
    let meetings = spawn_shortcut_pump(rx, ptt_tx, egui::Context::default()).unwrap();
    tx.send(ShortcutEvent::Dictation(PttEvent::Down)).unwrap();
    tx.send(ShortcutEvent::MeetingToggle).unwrap();
    tx.send(ShortcutEvent::Dictation(PttEvent::Up)).unwrap();
    tx.send(ShortcutEvent::MeetingToggle).unwrap();
    drop(tx);
    assert_eq!(meetings.iter().count(), 2);
}

#[test]
fn meeting_toggle_never_waits_for_dictation_to_drain_its_input() {
    use hark_hotkey::{PttEvent, ShortcutEvent};
    let (tx, rx) = mpsc::channel();
    let (ptt_tx, ptt_rx) = mpsc::channel();
    let meetings = spawn_shortcut_pump(rx, ptt_tx, egui::Context::default()).unwrap();
    tx.send(ShortcutEvent::Dictation(PttEvent::Down)).unwrap();
    tx.send(ShortcutEvent::MeetingToggle).unwrap();
    meetings
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    assert_eq!(ptt_rx.try_recv(), Ok(PttEvent::Down));
    drop(tx);
    assert_eq!(
        ptt_rx.recv_timeout(std::time::Duration::from_secs(1)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    );
}

#[test]
fn disabled_meetings_never_bind_a_shortcut() {
    let mut settings = Settings::default();
    settings.meeting.enabled = false;
    settings.meeting.toggle_key = Some("LCtrl+F11".into());
    assert_eq!(meeting_chord(&settings).unwrap(), None);
    if !hark_pipeline::meeting::meetings_supported() {
        settings.meeting.enabled = true;
        assert_eq!(meeting_chord(&settings).unwrap(), None);
    }
}

fn record() -> Box<DictationRecord> {
    Box::new(DictationRecord {
        raw_text: "raw".to_string(),
        final_text: "final".to_string(),
        voice: "clean".to_string(),
        stt_provider: "deepgram".to_string(),
        stt_model: "nova-3".to_string(),
        cleanup_model: None,
        invocation: None,
        audio_ms: 900,
        stt_ms: 300,
        cleanup_ms: None,
        total_ms: 450,
        spellbook_replacements: Some(0),
        foreground_app: None,
    })
}

#[test]
fn happy_path_walks_recording_processing_idle() {
    assert!(matches!(
        next_status(PipelineEvent::Recording).expect("moves the status"),
        PipelineStatus::Recording
    ));
    assert!(matches!(
        next_status(PipelineEvent::Processing).expect("moves the status"),
        PipelineStatus::Processing
    ));
    assert!(matches!(
        next_status(PipelineEvent::Injected(record())).expect("moves the status"),
        PipelineStatus::Idle
    ));
}

#[test]
fn informational_endings_return_to_idle_not_error() {
    for stage in [
        FailStage::GatedTooShort,
        FailStage::EmptyTranscript,
        FailStage::Abandoned,
    ] {
        let s = next_status(PipelineEvent::Failed {
            stage,
            detail: "informational".to_string(),
        })
        .expect("moves the status");
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
    })
    .expect("moves the status");
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
    })
    .expect("moves the status");
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
    })
    .expect("moves the status");
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
fn an_interception_report_persists_without_moving_the_status() {
    let (tx, rx) = mpsc::channel();
    let mut controller = PipelineController::new(None);
    controller.events = Some(rx);
    tx.send(PipelineEvent::Recording).unwrap();
    tx.send(PipelineEvent::ShortcutIntercepted {
        key: "F12".to_string(),
    })
    .unwrap();
    controller.drain_events();
    // It lands mid-hold, and the footer must still say Recording.
    assert!(matches!(controller.status(), PipelineStatus::Recording));
    assert_eq!(controller.shortcut_warning(), Some("F12"));
    // The next dictation replaces the status, not the conflict.
    tx.send(PipelineEvent::Injected(record())).unwrap();
    controller.drain_events();
    assert_eq!(controller.shortcut_warning(), Some("F12"));
    // Saving a new shortcut restarts the pipeline, which clears it.
    controller.stop();
    assert_eq!(controller.shortcut_warning(), None);
}

#[test]
fn the_next_dictation_replaces_a_sticky_error() {
    // Errored has no special-case handling: any newer event wins.
    assert!(matches!(
        next_status(PipelineEvent::Recording).expect("moves the status"),
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
        None,
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

fn policy(capture: bool) -> RecordPolicy {
    RecordPolicy {
        capture,
        retention: hark_store::Retention {
            max_entries: 7,
            max_age_days: 3,
        },
    }
}

fn not_pasted() -> PipelineEvent {
    PipelineEvent::NotPasted {
        record: record(),
        detail: "no focus".to_string(),
    }
}

#[test]
fn undelivered_records_reach_history_only_when_capture_is_on() {
    for event in [not_pasted(), PipelineEvent::Recovered(record())] {
        assert!(
            matches!(
                storage_command(&event, &policy(true)),
                Some(StorageCmd::RecordUndelivered { .. })
            ),
            "capture on stores a flagged row"
        );
        assert!(
            storage_command(&event, &policy(false)).is_none(),
            "capture off stores nothing, not even a counter"
        );
    }
}

#[test]
fn a_delivered_record_is_still_recorded_with_capture_off() {
    assert!(matches!(
        storage_command(&PipelineEvent::Injected(record()), &policy(false)),
        Some(StorageCmd::Record { capture: false, .. })
    ));
    assert!(storage_command(&PipelineEvent::ClipHeld { held: true }, &policy(true)).is_none());
}

#[test]
fn a_failed_paste_says_where_the_text_went() {
    match next_status(not_pasted()).expect("moves the status") {
        PipelineStatus::Errored { detail, .. } => assert!(detail.contains("History")),
        _ => panic!("expected Errored"),
    }
}

#[test]
fn a_failed_transcription_points_at_history_and_keeps_the_key_hint() {
    let status = next_status(PipelineEvent::Failed {
        stage: FailStage::Transcribe,
        detail: "authentication rejected by deepgram: check your API key".to_string(),
    })
    .expect("moves the status");
    match status {
        PipelineStatus::Errored {
            detail,
            key_related,
        } => {
            assert!(key_related);
            assert!(detail.ends_with("Try again from History."), "{detail}");
            assert!(!detail.contains(".."));
        }
        _ => panic!("expected Errored"),
    }
}

#[test]
fn held_clip_bookkeeping_leaves_the_footer_alone_and_a_recovery_ends_the_wait() {
    assert!(next_status(PipelineEvent::ClipHeld { held: true }).is_none());
    assert!(next_status(PipelineEvent::ClipHeld { held: false }).is_none());
    assert!(matches!(
        next_status(PipelineEvent::Recovered(record())),
        Some(PipelineStatus::Idle)
    ));
}

#[test]
fn the_controller_keeps_the_banner_slot_in_step_and_drops_audio_on_stop() {
    use crate::ui::history::recovery::Undelivered;
    let (tx, rx) = mpsc::channel();
    let mut controller = PipelineController::new(None);
    controller.events = Some(rx);

    tx.send(not_pasted()).unwrap();
    controller.drain_events();
    assert!(matches!(
        controller.recovery().snapshot(),
        Some(Undelivered::Text(t)) if t == "final"
    ));

    tx.send(PipelineEvent::ClipHeld { held: true }).unwrap();
    controller.drain_events();
    assert!(matches!(
        controller.recovery().snapshot(),
        Some(Undelivered::Audio { retrying: false })
    ));

    // Restarting the pipeline (a settings save) frees the worker's clip.
    controller.stop();
    assert!(controller.recovery().snapshot().is_none());
}

#[test]
fn a_banner_request_with_no_pipeline_clears_instead_of_waiting_forever() {
    let (tx, rx) = mpsc::channel();
    let mut controller = PipelineController::new(None);
    controller.events = Some(rx);
    tx.send(PipelineEvent::ClipHeld { held: true }).unwrap();
    controller.drain_events();
    controller.recovery().retry();
    controller.drain_events();
    assert!(controller.recovery().snapshot().is_none());
    assert_eq!(controller.recovery().take_request(), None);
}

#[test]
fn pump_without_storage_still_forwards_events() {
    let (pipeline_tx, pipeline_rx) = mpsc::channel();
    let ui_rx = spawn_repaint_pump(pipeline_rx, egui::Context::default(), None, None);
    pipeline_tx.send(PipelineEvent::Injected(record())).unwrap();
    drop(pipeline_tx);
    assert_eq!(ui_rx.iter().count(), 1);
}

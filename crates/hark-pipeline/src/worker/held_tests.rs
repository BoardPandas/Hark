//! Never losing the words: a failed paste hands the text to the UI, a failed
//! transcription keeps the one clip in memory, and a retry of that clip yields
//! text that is delivered to the UI but never pasted.

use super::recovery_tests::worker;
use super::*;
use crate::lifecycle::RunControl;
use hark_stt::{SttError, Transcript};
use std::collections::VecDeque;
use std::sync::mpsc;
use std::sync::Mutex;

/// Pops one scripted result per call and counts the calls.
struct Scripted {
    script: Mutex<VecDeque<Result<Transcript, SttError>>>,
    calls: Arc<AtomicU64>,
}

impl SttProvider for Scripted {
    fn transcribe(&self, _: &[u8]) -> Result<Transcript, SttError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.script
            .lock()
            .unwrap()
            .pop_front()
            .expect("the test scripted a result for every call")
    }
    fn label(&self) -> &str {
        "test"
    }
}

fn words(text: &str) -> Result<Transcript, SttError> {
    Ok(Transcript {
        text: text.to_string(),
        cleaned: None,
        request_ms: 1,
    })
}

fn auth_failure() -> Result<Transcript, SttError> {
    Err(SttError::Auth {
        provider: "test".into(),
        status: 401,
        detail: String::new(),
    })
}

struct Rig {
    worker: Worker,
    events: mpsc::Receiver<PipelineEvent>,
    provider_calls: Arc<AtomicU64>,
}

fn rig(script: Vec<Result<Transcript, SttError>>) -> Rig {
    let calls = Arc::new(AtomicU64::new(0));
    let provider = Scripted {
        script: Mutex::new(script.into()),
        calls: calls.clone(),
    };
    let mut worker = worker(Box::new(provider), Arc::new(RunControl::default()));
    let (tx, events) = mpsc::channel();
    worker.events = tx;
    Rig {
        worker,
        events,
        provider_calls: calls,
    }
}

/// One dictation of the 1 s clip the helper worker's ring holds.
fn dictate_once(
    worker: &mut Worker,
    inject: impl FnOnce(&str, &InjectSettings) -> Result<(), hark_inject::InjectError>,
) {
    dictate(
        worker,
        16_000,
        32_000,
        PipelineState::Transcribing,
        LiveTurn::default(),
        None,
        inject,
    );
}

fn ok_inject(_: &str, _: &InjectSettings) -> Result<(), hark_inject::InjectError> {
    Ok(())
}

fn drain(events: &mpsc::Receiver<PipelineEvent>) -> Vec<PipelineEvent> {
    events.try_iter().collect()
}

fn held_flags(events: &[PipelineEvent]) -> Vec<bool> {
    events
        .iter()
        .filter_map(|e| match e {
            PipelineEvent::ClipHeld { held } => Some(*held),
            _ => None,
        })
        .collect()
}

#[test]
fn a_failed_paste_delivers_the_finished_text_to_the_ui() {
    let mut rig = rig(vec![words("pasted nowhere")]);
    dictate_once(&mut rig.worker, |_, _| {
        Err(hark_inject::InjectError::Typing("no focus".into()))
    });

    let events = drain(&rig.events);
    let record = events
        .iter()
        .find_map(|e| match e {
            PipelineEvent::NotPasted { record, detail } => {
                assert!(detail.contains("no focus"));
                Some(record)
            }
            _ => None,
        })
        .expect("the text is handed over instead of dropped");
    assert_eq!(record.final_text, "pasted nowhere");
    assert_eq!(record.raw_text, "pasted nowhere");
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, PipelineEvent::Injected(_))),
        "a failed paste is not a delivery"
    );
    assert!(rig.worker.held.is_none(), "the audio was transcribed fine");
}

#[test]
fn a_failed_transcription_holds_exactly_one_clip_in_memory() {
    let mut rig = rig(vec![auth_failure(), auth_failure()]);
    dictate_once(&mut rig.worker, |_, _| panic!("nothing to inject"));

    let held = rig.worker.held.as_ref().expect("the clip is kept");
    assert_eq!(held.samples.len(), 16_000, "one second at 16 kHz");
    assert_eq!(held.audio_ms, 1_000);
    let events = drain(&rig.events);
    assert!(matches!(
        events[..],
        [
            PipelineEvent::Failed {
                stage: FailStage::Transcribe,
                ..
            },
            PipelineEvent::ClipHeld { held: true },
        ]
    ));

    // A second failure replaces the first: never more than one.
    dictate_once(&mut rig.worker, |_, _| panic!("nothing to inject"));
    assert!(rig.worker.held.is_some());
}

#[test]
fn dictations_that_produced_nothing_to_retry_keep_no_clip() {
    // Empty transcript.
    let mut rig_empty = rig(vec![words("   ")]);
    dictate_once(&mut rig_empty.worker, |_, _| panic!("nothing to inject"));
    assert!(rig_empty.worker.held.is_none());

    // Gated: a hold too short to be speech never reaches a provider at all.
    let mut rig_gated = rig(vec![]);
    dictate(
        &mut rig_gated.worker,
        16_000,
        16_100,
        PipelineState::Transcribing,
        LiveTurn::default(),
        None,
        |_, _| panic!("nothing to inject"),
    );
    assert!(rig_gated.worker.held.is_none());
    assert_eq!(rig_gated.provider_calls.load(Ordering::Relaxed), 0);
}

#[test]
fn a_retired_pipeline_keeps_no_clip() {
    let mut rig = rig(vec![auth_failure()]);
    // A provider that is cancelled mid-request reports a failure too.
    rig.worker.control.cancel();
    dictate_once(&mut rig.worker, |_, _| panic!("nothing to inject"));
    assert!(rig.worker.held.is_none());
}

#[test]
fn a_later_successful_dictation_drops_the_held_clip() {
    let mut rig = rig(vec![auth_failure(), words("second try")]);
    dictate_once(&mut rig.worker, |_, _| panic!("nothing to inject"));
    assert!(rig.worker.held.is_some());
    drain(&rig.events);

    dictate_once(&mut rig.worker, ok_inject);
    assert!(rig.worker.held.is_none());
    let events = drain(&rig.events);
    assert_eq!(held_flags(&events), [false], "the UI is told it is gone");
}

#[test]
fn a_later_dictation_whose_paste_fails_still_drops_the_held_clip() {
    // Its words exist now (as a NotPasted record), so the old audio is stale.
    let mut rig = rig(vec![auth_failure(), words("second try")]);
    dictate_once(&mut rig.worker, |_, _| panic!("nothing to inject"));
    dictate_once(&mut rig.worker, |_, _| {
        Err(hark_inject::InjectError::Typing("no focus".into()))
    });
    assert!(rig.worker.held.is_none());
}

#[test]
fn a_successful_retry_delivers_an_unpasted_record_and_spends_the_clip() {
    let mut rig = rig(vec![auth_failure(), words("recovered words")]);
    dictate_once(&mut rig.worker, |_, _| panic!("nothing to inject"));
    drain(&rig.events);

    // `retry_requested` has no inject argument: a retry cannot paste.
    retry_requested(&mut rig.worker, PipelineState::Idle);

    let events = drain(&rig.events);
    // Processing first: the chord is refused for the length of the retry, and
    // the pill has to say why.
    assert!(matches!(
        events[..],
        [PipelineEvent::Processing, PipelineEvent::Recovered(_)]
    ));
    let PipelineEvent::Recovered(record) = &events[1] else {
        unreachable!("matched above");
    };
    assert_eq!(record.final_text, "recovered words");
    assert_eq!(record.audio_ms, 1_000);
    assert!(rig.worker.held.is_none(), "a recovered clip is spent");
    assert!(
        rig.worker.control.start_cycle(),
        "the retry released the cycle it took"
    );
}

#[test]
fn a_failed_retry_keeps_the_clip_and_reports_the_failure() {
    let mut rig = rig(vec![auth_failure(), auth_failure()]);
    dictate_once(&mut rig.worker, |_, _| panic!("nothing to inject"));
    drain(&rig.events);

    retry_requested(&mut rig.worker, PipelineState::Idle);

    assert!(rig.worker.held.is_some());
    let events = drain(&rig.events);
    assert!(matches!(
        events[..],
        [
            PipelineEvent::Processing,
            PipelineEvent::Failed {
                stage: FailStage::Transcribe,
                ..
            },
            PipelineEvent::ClipHeld { held: true },
        ]
    ));
    assert_eq!(rig.provider_calls.load(Ordering::Relaxed), 2);
    // And it can be tried again.
    assert!(rig.worker.control.start_cycle());
}

#[test]
fn a_retry_gets_one_cloud_attempt_not_two() {
    // A normal dictation retries a timeout once. A retry of the held clip
    // must not: the worker refuses push-to-talk for as long as it runs.
    let timeout = || {
        Err(SttError::Timeout {
            provider: "test".into(),
            configured_ms: 6_000,
        })
    };
    let mut rig = rig(vec![auth_failure(), timeout(), words("unreached")]);
    dictate_once(&mut rig.worker, |_, _| panic!("nothing to inject"));

    retry_requested(&mut rig.worker, PipelineState::Idle);

    assert_eq!(rig.provider_calls.load(Ordering::Relaxed), 2);
    assert!(
        rig.worker.held.is_some(),
        "the clip survives for another try"
    );
}

#[test]
fn an_empty_retry_keeps_the_clip() {
    let mut rig = rig(vec![auth_failure(), words("  ")]);
    dictate_once(&mut rig.worker, |_, _| panic!("nothing to inject"));
    drain(&rig.events);

    retry_requested(&mut rig.worker, PipelineState::Idle);

    assert!(rig.worker.held.is_some());
    assert_eq!(held_flags(&drain(&rig.events)), [true]);
}

#[test]
fn a_retry_is_declined_unless_the_pipeline_is_idle() {
    let mut rig = rig(vec![auth_failure()]);
    dictate_once(&mut rig.worker, |_, _| panic!("nothing to inject"));
    drain(&rig.events);

    // Mid-hold.
    retry_requested(&mut rig.worker, PipelineState::Recording { down_abs: 5 });
    // Idle, but a hold has already been admitted at the input boundary.
    assert!(rig.worker.control.start_cycle());
    retry_requested(&mut rig.worker, PipelineState::Idle);

    assert_eq!(
        rig.provider_calls.load(Ordering::Relaxed),
        1,
        "no request was made for either declined retry"
    );
    assert!(rig.worker.held.is_some(), "declining keeps the clip");
    assert_eq!(held_flags(&drain(&rig.events)), [true, true]);
}

#[test]
fn a_retry_with_nothing_held_answers_so_the_ui_cannot_stick() {
    let mut rig = rig(vec![]);
    retry_requested(&mut rig.worker, PipelineState::Idle);
    assert_eq!(held_flags(&drain(&rig.events)), [false]);
}

#[test]
fn dismiss_and_retry_arrive_over_the_worker_input_channel() {
    let mut rig = rig(vec![words("through the channel")]);
    let clip = || HeldClip {
        samples: vec![0.25; 16_000],
        audio_ms: 1_000,
        foreground_app: None,
    };
    rig.worker.held = Some(clip());
    let control = rig.worker.control.clone();
    let (tx, raw) = mpsc::channel();
    let (routed, commands) =
        crate::input::route(raw, rig.worker.consumer.reader(), control).unwrap();
    let events = rig.events;
    let thread = std::thread::spawn(move || run(rig.worker, routed));
    let receive = || {
        events
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
    };

    // A retry reaches the worker and recovers the clip.
    commands.send(WorkerInput::RetryHeld).unwrap();
    assert!(matches!(receive(), PipelineEvent::Processing));
    assert!(matches!(receive(), PipelineEvent::Recovered(_)));

    // Nothing is left to retry now.
    commands.send(WorkerInput::RetryHeld).unwrap();
    assert!(matches!(receive(), PipelineEvent::ClipHeld { held: false }));

    drop(tx);
    drop(commands);
    thread.join().unwrap();
}

#[test]
fn a_dismissed_clip_cannot_be_retried() {
    let mut rig = rig(vec![]);
    rig.worker.held = Some(HeldClip {
        samples: vec![0.25; 16_000],
        audio_ms: 1_000,
        foreground_app: None,
    });
    let control = rig.worker.control.clone();
    let (tx, raw) = mpsc::channel();
    let (routed, commands) =
        crate::input::route(raw, rig.worker.consumer.reader(), control).unwrap();
    let events = rig.events;
    let calls = rig.provider_calls.clone();
    let thread = std::thread::spawn(move || run(rig.worker, routed));

    commands.send(WorkerInput::DismissHeld).unwrap();
    commands.send(WorkerInput::RetryHeld).unwrap();
    assert!(matches!(
        events
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap(),
        PipelineEvent::ClipHeld { held: false }
    ));
    assert_eq!(calls.load(Ordering::Relaxed), 0);

    drop(tx);
    drop(commands);
    thread.join().unwrap();
}

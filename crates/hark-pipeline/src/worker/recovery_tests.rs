use super::*;
use crate::lifecycle::RunControl;
use std::cell::Cell;
use std::sync::mpsc;

struct RetiringProvider {
    control: Arc<RunControl>,
    calls: Arc<AtomicU64>,
    timeout: bool,
}

impl SttProvider for RetiringProvider {
    fn transcribe(&self, _: &[u8]) -> Result<Transcript, SttError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.control.cancel();
        if self.timeout {
            Err(SttError::Timeout {
                provider: "test".into(),
                configured_ms: 1,
            })
        } else {
            Ok(Transcript {
                text: "a completed test dictation".into(),
                cleaned: None,
                request_ms: 1,
            })
        }
    }
    fn label(&self) -> &str {
        "test"
    }
}

fn worker(provider: Box<dyn SttProvider>, control: Arc<RunControl>) -> Worker {
    let (producer, consumer) = hark_audio::ring::ring(48_000);
    producer.push(&vec![0.25; 48_000]);
    let (events, _) = mpsc::channel();
    Worker {
        consumer,
        sample_rate: 16_000,
        window: WindowParams {
            preroll_ms: 0,
            tail_ms: 0,
            ..Default::default()
        },
        inject: InjectSettings::default(),
        provider: Some(provider),
        live: None,
        cloud_label: "test".into(),
        local: None,
        corrector: Corrector::new(&[]),
        expander: Expander::new(&[]),
        cleanup: None,
        prewarm_url: String::new(),
        client: hark_stt::shared_client().unwrap(),
        stt_model: "test".into(),
        strip_single_word_period: false,
        track_apps: false,
        events,
        recording: Arc::new(AtomicBool::new(false)),
        control,
        discontinuities: Arc::new(AtomicU64::new(0)),
    }
}

#[test]
fn retiring_during_stt_prevents_late_injection() {
    let control = Arc::new(RunControl::default());
    let provider = RetiringProvider {
        control: control.clone(),
        calls: Arc::new(AtomicU64::new(0)),
        timeout: false,
    };
    let mut worker = worker(Box::new(provider), control);
    let injected = Cell::new(false);
    let state = dictate(
        &mut worker,
        16_000,
        32_000,
        PipelineState::Transcribing,
        LiveTurn::default(),
        None,
        |_, _| {
            injected.set(true);
            Ok(())
        },
    );
    assert_eq!(state, PipelineState::Idle);
    assert!(
        !injected.get(),
        "a retired provider result must never reach the focused application"
    );
}

#[test]
fn retiring_during_timeout_prevents_a_second_request() {
    let control = Arc::new(RunControl::default());
    let calls = Arc::new(AtomicU64::new(0));
    let provider = RetiringProvider {
        control: control.clone(),
        calls: calls.clone(),
        timeout: true,
    };
    let mut worker = worker(Box::new(provider), control);
    dictate(
        &mut worker,
        16_000,
        32_000,
        PipelineState::Transcribing,
        LiveTurn::default(),
        None,
        |_, _| panic!("no transcript"),
    );
    assert_eq!(
        calls.load(Ordering::Relaxed),
        1,
        "retired work cannot start a retry"
    );
}

struct TimeoutProvider(Arc<AtomicU64>);
impl SttProvider for TimeoutProvider {
    fn transcribe(&self, _: &[u8]) -> Result<Transcript, SttError> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Err(SttError::Timeout {
            provider: "test".into(),
            configured_ms: 1,
        })
    }
    fn label(&self) -> &str {
        "test"
    }
}

struct BrokenLive {
    refuse_start: bool,
}
impl LiveStt for BrokenLive {
    fn start_session(&self) -> Result<Box<dyn hark_stt::LiveSession>, SttError> {
        if self.refuse_start {
            return Err(SttError::Http {
                provider: "test".into(),
                detail: "connection refused".into(),
            });
        }
        Ok(Box::new(BrokenSession))
    }
}
struct BrokenSession;
impl hark_stt::LiveSession for BrokenSession {
    fn push(&mut self, _: &[f32]) -> Result<(), SttError> {
        Err(SttError::Http {
            provider: "test".into(),
            detail: "connection closed".into(),
        })
    }
    fn finish(&mut self) -> Result<Transcript, SttError> {
        panic!("failed session cannot finish")
    }
}

#[test]
fn failed_live_open_or_mid_hold_send_allows_only_one_batch_replay() {
    for refuse_start in [false, true] {
        let calls = Arc::new(AtomicU64::new(0));
        let mut worker = worker(
            Box::new(TimeoutProvider(calls.clone())),
            Arc::new(RunControl::default()),
        );
        let adapter = BrokenLive { refuse_start };
        let mut turn = LiveTurn::start(
            Some(&adapter),
            &worker.consumer,
            16_000,
            16_000,
            &worker.window,
        );
        if !refuse_start {
            assert!(turn.push_available().is_err());
        }
        assert!(turn.pump.is_none());
        dictate(
            &mut worker,
            16_000,
            32_000,
            PipelineState::Transcribing,
            turn,
            None,
            |_, _| panic!("no transcript"),
        );
        assert_eq!(
            calls.load(Ordering::Relaxed),
            1,
            "live attempt already spent the first cloud request (refuse_start={refuse_start})"
        );
    }
}

struct RetiringCleaner(Arc<RunControl>);
impl CleanupProvider for RetiringCleaner {
    fn clean(&self, _: &str) -> Result<hark_voice::Cleaned, hark_voice::CleanupError> {
        self.0.cancel();
        Ok(hark_voice::Cleaned {
            text: "a completed test dictation".into(),
            request_ms: 1,
        })
    }
    fn label(&self) -> &str {
        "test"
    }
}

struct SuccessfulProvider;
impl SttProvider for SuccessfulProvider {
    fn transcribe(&self, _: &[u8]) -> Result<Transcript, SttError> {
        Ok(Transcript {
            text: "a completed test dictation".into(),
            cleaned: None,
            request_ms: 1,
        })
    }
    fn label(&self) -> &str {
        "test"
    }
}

#[test]
fn retiring_during_cleanup_prevents_late_injection() {
    let control = Arc::new(RunControl::default());
    let mut worker = worker(Box::new(SuccessfulProvider), control.clone());
    worker.cleanup = Some(CleanupPlan {
        cleaner: Box::new(RetiringCleaner(control)),
        voice: Voice::Clean,
        model: "test".into(),
        skip_below_words: 0,
        max_expansion_ratio: 2.0,
        prewarm_url: None,
    });
    dictate(
        &mut worker,
        16_000,
        32_000,
        PipelineState::Transcribing,
        LiveTurn::default(),
        None,
        |_, _| panic!("cancelled cleanup must not inject"),
    );
}

#[test]
fn worker_reopens_admission_after_a_gated_hold() {
    let control = Arc::new(RunControl::default());
    let mut worker = worker(
        Box::new(TimeoutProvider(Arc::new(AtomicU64::new(0)))),
        control.clone(),
    );
    worker.provider = None; // No prewarm or external calls in this test.
    let (events, received) = mpsc::channel();
    let receive = || {
        received
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
    };
    worker.events = events;
    let (tx, raw) = mpsc::channel();
    let routed = crate::input::route(raw, worker.consumer.reader(), control).unwrap();
    let thread = std::thread::spawn(move || run(worker, routed));
    tx.send(PttEvent::Down).unwrap();
    assert!(matches!(receive(), PipelineEvent::Recording));
    tx.send(PttEvent::Up).unwrap();
    assert!(matches!(receive(), PipelineEvent::Processing));
    assert!(matches!(
        receive(),
        PipelineEvent::Failed {
            stage: FailStage::GatedTooShort,
            ..
        }
    ));
    tx.send(PttEvent::Intercepted(hark_hotkey::PttKeyCode::LCtrl))
        .unwrap();
    assert!(matches!(
        receive(),
        PipelineEvent::ShortcutIntercepted { .. }
    ));
    tx.send(PttEvent::Down).unwrap();
    assert!(matches!(receive(), PipelineEvent::Recording));
    drop(tx);
    thread.join().unwrap();
}

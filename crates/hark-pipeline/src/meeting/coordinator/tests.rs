use super::devices::{choose_mic, me_silence_text};
use super::*;

#[test]
fn dropping_the_handle_stops_even_while_a_watcher_owns_a_sender() {
    let (tx, rx) = mpsc::channel();
    let watcher_tx = tx.clone();
    let (done_tx, done) = mpsc::channel();
    let stopped = Arc::new(AtomicBool::new(false));
    let worker_stopped = stopped.clone();
    let thread = std::thread::spawn(move || {
        let _done = done_tx;
        let _watcher_tx = watcher_tx;
        if matches!(
            rx.recv_timeout(Duration::from_secs(1)),
            Ok(Command::Shutdown)
        ) {
            worker_stopped.store(true, Ordering::Release);
        }
    });
    drop(MeetingHandle {
        tx: Some(tx),
        thread: Some(thread),
        done,
    });
    assert!(stopped.load(Ordering::Acquire));
}

#[test]
fn detection_deadlines_wake_before_the_slow_backstop() {
    assert_eq!(
        detection_wait(DETECT_BACKSTOP, Some(5_000), false),
        Duration::from_secs(5)
    );
    assert_eq!(
        detection_wait(DETECT_BACKSTOP, Some(0), false),
        Duration::ZERO
    );
    assert_eq!(
        detection_wait(DETECT_BACKSTOP, None, false),
        DETECT_BACKSTOP
    );
}

#[test]
fn capture_drain_and_auto_stop_deadline_both_bound_the_wait() {
    assert_eq!(
        detection_wait(DETECT_BACKSTOP, Some(15_000), true),
        DRAIN_EVERY
    );
    assert_eq!(
        detection_wait(DETECT_BACKSTOP, Some(15), true),
        Duration::from_millis(15)
    );
    assert_eq!(
        detection_wait(Duration::from_millis(2), Some(15), true),
        Duration::from_millis(2)
    );
}

#[test]
fn detection_mode_follows_enabled_and_auto_detect() {
    let mut s = Settings::default();
    s.meeting.auto_detect = AutoDetect::Auto;
    assert_eq!(detect_config(&s, "x").mode, DetectMode::Auto);
    s.meeting.enabled = false;
    assert_eq!(detect_config(&s, "x").mode, DetectMode::Off);
    s.meeting.enabled = true;
    s.meeting.auto_detect = AutoDetect::Off;
    assert_eq!(detect_config(&s, "x").mode, DetectMode::Off);
}

#[test]
fn the_built_in_app_list_applies_until_the_user_edits_it() {
    let mut s = Settings::default();
    assert_eq!(
        detect_config(&s, "x").apps.len(),
        detect::DEFAULT_APPS.len()
    );
    s.meeting.detect_apps = Some(vec!["zoom.exe".to_string()]);
    assert_eq!(detect_config(&s, "x").apps, ["zoom.exe"]);
}

/// A coordinator over `dir` with every network step switched off.
fn offline(dir: &std::path::Path) -> (Coordinator, Receiver<MeetingEvent>) {
    let mut settings = Settings::default();
    settings.meeting.final_pass = hark_config::FinalPass::None;
    settings.meeting.summary = false;
    settings.meeting.compress_audio = false;
    let (tx, rx) = mpsc::channel();
    let coordinator = Coordinator {
        detector: Detector::new(detect_config(&settings, "x")),
        settings,
        self_exe: "x".to_string(),
        events: tx,
        meetings_dir: dir.to_path_buf(),
        active: None,
        prompted: None,
        protected: Arc::new(Mutex::new(Vec::new())),
        retries: Retries::disabled(),
        refreshes: Refreshes::default(),
        probe_failed: false,
        probe_failing: false,
    };
    (coordinator, rx)
}

#[test]
fn an_interrupted_finish_is_completed_at_startup() {
    let dir = tempfile::tempdir().expect("tempdir");
    let meeting = dir.path().join("20260927-101500");
    let mut spool =
        hark_audio::spool::SpoolWriter::create(&meeting.join(hark_audio::spool::ME_FILE))
            .expect("spool");
    spool.append(&vec![0i16; 16_000]).expect("append");
    spool.close().expect("close");
    std::fs::write(meeting.join(FINISHING_MARKER), b"").expect("marker");
    // A finished meeting next to it: no marker, so it must be left alone.
    std::fs::create_dir(dir.path().join("20260926-090000")).expect("mkdir");

    let (coordinator, rx) = offline(dir.path());
    coordinator.recover();
    let mut finished = Vec::new();
    while let Ok(event) = rx.recv_timeout(Duration::from_secs(10)) {
        if let MeetingEvent::Finished { id, audio_bytes } = event {
            finished.push((id, audio_bytes));
            break;
        }
    }
    assert_eq!(finished.len(), 1, "exactly the marked meeting is finished");
    assert_eq!(finished[0].0, "20260927-101500");
    assert!(finished[0].1 >= 32_000, "the spool is still its audio");
    // The marker goes once the results are sent.
    let deadline = Instant::now() + Duration::from_secs(5);
    while meeting.join(FINISHING_MARKER).exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!meeting.join(FINISHING_MARKER).exists());
}

#[test]
fn the_mic_is_the_setting_then_the_call_apps_then_the_communications_default() {
    let unused = || -> Option<String> { panic!("looked up after a choice was made") };
    let shure = || Some("Microphone (Shure MV7+)".to_string());
    let chat = || Some("Microphone (Chat-Audeze Maxwell2 ANC)".to_string());
    assert_eq!(
        choose_mic(Some("Mic X".into()), unused, unused)
            .0
            .as_deref(),
        Some("Mic X")
    );
    // 2026-10-14: Teams on the Shure, the communications default muted.
    assert_eq!(choose_mic(None, shure, unused).0, shure());
    assert_eq!(choose_mic(None, || None, chat).0, chat());
    assert_eq!(choose_mic(None, || None, || None).0, None);
}

#[test]
fn a_silent_mic_notice_names_the_mic_the_call_app_uses() {
    assert_eq!(
        me_silence_text(
            Some("Microphone (Chat-Audeze Maxwell2 ANC)"),
            Some("Microsoft Teams"),
            Some("Microphone (Shure MV7+)")
        ),
        "Hark hears nothing from Microphone (Chat-Audeze Maxwell2 ANC). Microsoft Teams is \
         using Microphone (Shure MV7+); switch in Settings > Meetings."
    );
    let same = me_silence_text(Some("Mic"), Some("Zoom"), Some("Mic"));
    assert!(same.contains("not muted"), "{same}");
    let unknown = me_silence_text(None, None, None);
    assert!(unknown.starts_with("Hark hears nothing from your default microphone."));
}

#[test]
fn meeting_ids_are_unique_within_the_folder() {
    // A fixed start time: two clock reads straddled a second on a CI
    // runner, so the second id got a new timestamp instead of a suffix.
    let dir = tempfile::tempdir().expect("tempdir");
    let base = "20260929-140127";
    assert_eq!(unique_in(dir.path(), base), base);
    std::fs::create_dir(dir.path().join(base)).expect("mkdir");
    assert_eq!(unique_in(dir.path(), base), "20260929-140127-2");
    std::fs::create_dir(dir.path().join("20260929-140127-2")).expect("mkdir");
    assert_eq!(unique_in(dir.path(), base), "20260929-140127-3");
}

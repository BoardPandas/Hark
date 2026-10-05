//! The meeting coordinator thread: detection (PipeWire-driven on Linux,
//! polled on Windows and macOS) with timed deadlines, the active recording's
//! capture drain, and commands from the UI.
//! Nothing here waits on the network; the live transcriber and the finisher
//! own that.

use super::finish::{self, FinishJob, FINISHING_MARKER};
use super::live::{self, Engine};
use super::recorder::Recorder;
use super::{app_display_name, meetings_supported, Answer, MeetingEvent, StartedMeeting, Trigger};
use hark_audio::LoopbackTarget;
use hark_config::{AutoDetect, Settings, SystemSource};
use hark_meeting::detect::{self, DetectConfig, DetectMode, Detector, Verdict};
use hark_meeting::{advance, Action, ChunkParams, Event, SessionState, Transcript};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const DRAIN_EVERY: Duration = Duration::from_millis(100);
const DETECT_FALLBACK: Duration = Duration::from_millis(detect::POLL_MS);
/// Change notifications do not cover browser window-title changes.
const DETECT_BACKSTOP: Duration = Duration::from_secs(10);
const WATCH_RETRY: Duration = Duration::from_secs(30);
/// Plan §4.9 rule 3: while recording, re-check the cap every 60 s.
const CAP_EVERY: Duration = Duration::from_secs(60);
/// How long quitting waits for the coordinator to close the spools.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(3);

enum Command {
    StartManual,
    Stop,
    Toggle,
    Rerun { id: String, audio_ms: u64 },
    Answer(Answer),
    Settings(Box<Settings>),
    DetectionChanged,
    Shutdown,
}

/// The UI's handle. Dropping it stops a recording in progress (the audio and
/// the live transcript so far are kept; the after-call work is skipped) and
/// joins the coordinator, bounded.
pub struct MeetingHandle {
    tx: Option<Sender<Command>>,
    thread: Option<JoinHandle<()>>,
    /// Disconnects when the coordinator returns; never carries a value.
    done: Receiver<()>,
}

impl MeetingHandle {
    pub fn start_manual(&self) {
        self.send(Command::StartManual);
    }

    pub fn stop(&self) {
        self.send(Command::Stop);
    }

    pub fn toggle(&self) {
        self.send(Command::Toggle);
    }

    /// Explicitly re-run the final pass using the retained meeting audio.
    pub fn rerun(&self, id: &str, audio_ms: u64) {
        self.send(Command::Rerun {
            id: id.into(),
            audio_ms,
        });
    }

    pub fn answer(&self, answer: Answer) {
        self.send(Command::Answer(answer));
    }

    /// Apply saved settings without interrupting a meeting in progress: they
    /// govern detection now and the next meeting's capture.
    pub fn update_settings(&self, settings: &Settings) {
        self.send(Command::Settings(Box::new(settings.clone())));
    }

    fn send(&self, cmd: Command) {
        if let Some(tx) = &self.tx {
            // Only fails once the coordinator has exited (shutdown).
            let _ = tx.send(cmd);
        }
    }
}

impl Drop for MeetingHandle {
    fn drop(&mut self) {
        // A change watcher owns another sender. Explicit shutdown must
        // precede waiting: channel disconnection alone can no longer stop us.
        self.send(Command::Shutdown);
        self.tx.take();
        if let Some(thread) = self.thread.take() {
            match self.done.recv_timeout(SHUTDOWN_GRACE) {
                Err(RecvTimeoutError::Timeout) => {
                    log::warn!("meetings: coordinator still busy at quit; not waiting for it")
                }
                _ => {
                    let _ = thread.join();
                }
            }
        }
    }
}

/// Start the coordinator. `Err` on platforms without meeting capture, or
/// with no data directory to record into.
pub fn run(settings: &Settings, events: Sender<MeetingEvent>) -> Result<MeetingHandle, String> {
    if !meetings_supported() {
        return Err("Meeting notes are not available on this platform yet.".to_string());
    }
    let meetings_dir = hark_config::default_data_dir()
        .ok_or_else(|| "No OS data directory found; meetings cannot be recorded.".to_string())?
        .join("meetings");
    let self_exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let (tx, rx) = mpsc::channel();
    let (done_tx, done) = mpsc::channel::<()>();
    let settings = settings.clone();
    let thread = std::thread::Builder::new()
        .name("hark-meeting-coordinator".to_string())
        .spawn({
            let watch_tx = tx.clone();
            move || {
                let _done = done_tx;
                // The recorder's AEC graph is thread-local (!Send). Construct
                // its owner here, so it never crosses a thread boundary.
                let coordinator = Coordinator {
                    detector: Detector::new(detect_config(&settings, &self_exe)),
                    settings,
                    self_exe,
                    events,
                    meetings_dir,
                    active: None,
                    prompted: None,
                    protected: Arc::new(Mutex::new(Vec::new())),
                    probe_failed: false,
                    probe_failing: false,
                };
                coordinator.run(rx, watch_tx);
            }
        })
        .map_err(|e| format!("cannot start meeting mode: {e}"))?;
    Ok(MeetingHandle {
        tx: Some(tx),
        thread: Some(thread),
        done,
    })
}

struct Active {
    recorder: Recorder,
    session: SessionState,
    live: Option<live::Live>,
    detected: bool,
    /// The detected app's display name, for the pending-stop notice.
    app_name: Option<String>,
    /// An auto-stop has been announced to the UI (and not cancelled).
    stop_announced: bool,
}

struct Coordinator {
    settings: Settings,
    self_exe: String,
    events: Sender<MeetingEvent>,
    meetings_dir: PathBuf,
    detector: Detector,
    active: Option<Active>,
    /// The app the open prompt is about.
    prompted: Option<String>,
    /// Recording + finishing ids: the storage cap never evicts these.
    protected: Arc<Mutex<Vec<String>>>,
    /// This platform has no mic probe; stop polling it (logged once).
    probe_failed: bool,
    /// The last probe failed (logged once). Retried at each backstop poll,
    /// never at a deadline, so a persistent fault cannot spin this thread.
    probe_failing: bool,
}

impl Coordinator {
    fn run(mut self, rx: Receiver<Command>, watch_tx: Sender<Command>) {
        self.recover();
        let notification_pending = Arc::new(AtomicBool::new(false));
        let mut watcher = None;
        let mut watch_failed = false;
        let mut next_watch = Instant::now();
        let mut next_backstop = Instant::now();
        let mut next_cap = Instant::now() + CAP_EVERY;
        loop {
            let now = Instant::now();
            let watch_alive = watcher
                .as_ref()
                .is_some_and(hark_meeting::probe::ChangeWatcher::is_alive);
            if !watch_alive && now >= next_watch {
                watcher.take();
                let tx = watch_tx.clone();
                let pending = notification_pending.clone();
                match hark_meeting::probe::ChangeWatcher::start(move || {
                    // Several graph events can describe one mic transition.
                    // At most one undrained wakeup is enough for a fresh snapshot.
                    if !pending.swap(true, Ordering::AcqRel) {
                        let _ = tx.send(Command::DetectionChanged);
                    }
                }) {
                    Ok(started) => {
                        watcher = Some(started);
                        watch_failed = false;
                        log::info!("meeting detection: change notifications active");
                    }
                    Err(error) => {
                        if !watch_failed {
                            if error.kind() == std::io::ErrorKind::Unsupported {
                                // Windows and macOS: polling is the design, not a fault.
                                log::info!(
                                    "meeting detection: polling every {} ms",
                                    detect::POLL_MS
                                );
                            } else {
                                log::warn!(
                                    "meeting change notifications unavailable ({error}); using polling"
                                );
                            }
                        }
                        watch_failed = true;
                    }
                }
                next_watch = now + WATCH_RETRY;
            }
            let mut wait = detection_wait(
                next_backstop.saturating_duration_since(now),
                self.observation_deadline(monotonic_ms()),
                self.active.is_some(),
            );
            if watcher.as_ref().is_none_or(|w| !w.is_alive()) {
                wait = wait.min(next_watch.saturating_duration_since(now));
            }
            let mut refresh = false;
            match rx.recv_timeout(wait) {
                Ok(Command::Shutdown) => break,
                Ok(Command::DetectionChanged) => {
                    notification_pending.store(false, Ordering::Release);
                    refresh = true;
                }
                Ok(cmd) => self.command(cmd),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            self.pump();
            let now = Instant::now();
            if refresh
                || now >= next_backstop
                || self.observation_deadline(monotonic_ms()) == Some(0)
            {
                self.detect();
            }
            if now >= next_backstop {
                let interval = if watcher.as_ref().is_some_and(|w| w.is_alive()) {
                    DETECT_BACKSTOP
                } else {
                    DETECT_FALLBACK
                };
                next_backstop = now + interval;
            }
            if self.active.is_some() && now >= next_cap {
                self.enforce_cap();
                next_cap = now + CAP_EVERY;
            }
        }
        // Retire the sender-holding watcher before closing capture and letting
        // this coordinator's completion channel disconnect. Only Linux has
        // one; elsewhere the stub owns nothing.
        #[cfg(target_os = "linux")]
        drop(watcher);
        // Quitting: keep what was recorded; the after-call work would outlive
        // the process, so it is skipped (the meeting keeps its live lines).
        if let Some(active) = self.active.take() {
            self.close(active, false);
        }
    }

    fn command(&mut self, cmd: Command) {
        match cmd {
            // The receive loop handles these before dispatching UI commands.
            Command::DetectionChanged | Command::Shutdown => {}
            Command::StartManual => self.start(Trigger::Manual, None),
            Command::Stop => {
                if let Some(active) = self.active.take() {
                    self.detector.stopped();
                    self.close(active, true);
                }
            }
            Command::Rerun { id, audio_ms } => {
                let Ok(mut protected) = self.protected.lock() else {
                    return;
                };
                if protected.contains(&id) {
                    return;
                }
                protected.push(id.clone());
                drop(protected);
                super::rerun::spawn(super::rerun::Job {
                    id,
                    audio_ms,
                    root: self.meetings_dir.clone(),
                    settings: self.settings.clone(),
                    events: self.events.clone(),
                    protected: self.protected.clone(),
                });
            }
            Command::Toggle => {
                // Decide on this thread's current state, not an asynchronously
                // repainted UI status. Repeated physical presses serialize.
                if self.active.is_some() {
                    self.command(Command::Stop);
                } else if self.settings.meeting.enabled {
                    self.start(Trigger::Manual, None);
                }
            }
            Command::Answer(answer) => {
                self.detector.answer(answer);
                let app = self.prompted.take();
                if answer == Answer::Start {
                    self.start(Trigger::Ask, app);
                }
            }
            Command::Settings(settings) => {
                self.detector
                    .set_config(detect_config(&settings, &self.self_exe));
                self.settings = *settings;
            }
        }
    }

    fn observation_deadline(&self, now_ms: u64) -> Option<u64> {
        let watching =
            self.settings.meeting.enabled && self.settings.meeting.auto_detect != AutoDetect::Off;
        let auto_stopping = self.active.as_ref().is_some_and(|a| a.detected);
        if self.probe_failed || self.probe_failing || !(watching || auto_stopping) {
            return None;
        }
        self.detector.next_observation_in_ms(now_ms)
    }

    fn detect(&mut self) {
        // Probing only matters to offer a meeting or to auto-stop a detected
        // one; with detection off and nothing detected, skip the probe.
        let watching =
            self.settings.meeting.enabled && self.settings.meeting.auto_detect != AutoDetect::Off;
        let auto_stopping = self.active.as_ref().is_some_and(|a| a.detected);
        if self.probe_failed || !(watching || auto_stopping) {
            return;
        }
        let snapshot = match hark_meeting::probe::snapshot() {
            Ok(s) => {
                if std::mem::take(&mut self.probe_failing) {
                    log::info!("meeting detection: probe working again");
                }
                s
            }
            Err(e) if e.kind() == std::io::ErrorKind::Unsupported => {
                log::warn!("meeting detection off for this session: {e}");
                self.probe_failed = true;
                return;
            }
            // Core Audio refuses while the Windows audio service restarts (a
            // driver update, a Bluetooth headset): retry at the next poll.
            Err(e) => {
                if !self.probe_failing {
                    log::warn!("meeting detection probe failed ({e}); retrying");
                }
                self.probe_failing = true;
                return;
            }
        };
        let now_ms = monotonic_ms();
        match self.detector.observe(&snapshot, now_ms) {
            Verdict::None => {}
            Verdict::Prompt(app) => {
                log::info!("meeting detected: {app} (asking)");
                self.prompted = Some(app.clone());
                let name = app_display_name(&app);
                let _ = self.events.send(MeetingEvent::Prompt { app, name });
            }
            Verdict::Retract => {
                self.prompted = None;
                let _ = self.events.send(MeetingEvent::PromptRetracted);
            }
            Verdict::Start(app) => {
                log::info!("meeting detected: {app} (starting)");
                self.start(Trigger::Auto, Some(app));
            }
            Verdict::Stop => {
                log::info!("meeting app released the mic; stopping");
                if let Some(active) = self.active.take() {
                    self.close(active, true);
                }
            }
        }
        self.announce_pending_stop(now_ms);
    }

    /// Tell the UI once when the detected app lets go of the mic (and when it
    /// takes it back), so hanging up visibly leads to the notes stopping.
    fn announce_pending_stop(&mut self, now_ms: u64) {
        let pending = self.detector.stop_pending_ms(now_ms);
        let Some(active) = self.active.as_mut() else {
            return;
        };
        if !active.detected {
            return;
        }
        let id = active.recorder.id.clone();
        match (pending, active.stop_announced) {
            (Some(ms), false) => {
                active.stop_announced = true;
                let at_ms = jiff::Timestamp::now().as_millisecond() + ms as i64;
                let app = active
                    .app_name
                    .clone()
                    .unwrap_or_else(|| "The meeting app".to_string());
                log::info!("meeting {id}: {app} released the mic; stopping in {ms} ms");
                let _ = self
                    .events
                    .send(MeetingEvent::AutoStopPending { id, at_ms, app });
            }
            (None, true) => {
                active.stop_announced = false;
                log::info!("meeting {id}: the app took the mic back; not stopping");
                let _ = self.events.send(MeetingEvent::AutoStopCancelled { id });
            }
            _ => {}
        }
    }

    fn start(&mut self, trigger: Trigger, app: Option<String>) {
        if self.active.is_some() {
            return;
        }
        // Whose audio "Them" is follows the trigger (a manual start records
        // everything except Hark), decided before a manual start adopts a call.
        let loopback = Some(self.loopback_target(app.as_deref()));
        let app = if trigger == Trigger::Manual {
            let (verdict, adopted) = self.detector.started_manually();
            if verdict == Verdict::Retract {
                self.prompted = None;
                let _ = self.events.send(MeetingEvent::PromptRetracted);
            }
            if let Some(adopted) = &adopted {
                log::info!(
                    "manual meeting adopts the {adopted} call: it stops when that call ends"
                );
            }
            adopted
        } else {
            app
        };
        let (session, action) = advance(SessionState::Idle, Event::Start);
        debug_assert_eq!(action, Action::StartCapture);

        let id = new_meeting_id(&self.meetings_dir);
        let dir = self.meetings_dir.join(&id);
        let started_ms = jiff::Timestamp::now().as_millisecond();
        let (engine, provider_label) = self.live_engine(&id);
        let live = match engine {
            Some(engine) => {
                let corrector =
                    hark_spellbook::Corrector::new(&self.settings.spellbook.corrector_entries());
                match live::spawn(id.clone(), engine, corrector, self.events.clone()) {
                    Ok(live) => Some(live),
                    Err(e) => {
                        self.notice(Some(&id), e);
                        None
                    }
                }
            }
            None => None,
        };
        let mic_device = self
            .settings
            .meeting
            .mic_device
            .clone()
            .or_else(hark_audio::communications_default_device);
        let jobs = live.as_ref().map(|l| l.jobs.clone());
        match Recorder::start(
            id.clone(),
            dir,
            mic_device,
            loopback,
            self.settings.meeting.echo_cancellation,
            jobs,
            ChunkParams {
                silence_rms: self.settings.audio.silence_rms,
                ..ChunkParams::default()
            },
        ) {
            Ok((recorder, notices)) => {
                log::info!(
                    "meeting {id} started ({}; system audio: {})",
                    trigger.label(),
                    recorder.has_system_audio()
                );
                if let Ok(mut p) = self.protected.lock() {
                    p.push(id.clone());
                }
                if let Err(e) = std::fs::write(recorder.dir.join(FINISHING_MARKER), b"") {
                    // Costs only the re-finish after a crash; recording goes on.
                    log::warn!("meeting {id}: finishing marker not written ({e})");
                }
                let _ = self.events.send(MeetingEvent::Started(StartedMeeting {
                    id: id.clone(),
                    started_ms,
                    trigger,
                    app: app.clone(),
                    stt_provider: provider_label,
                    system_audio: recorder.has_system_audio(),
                }));
                for text in notices {
                    self.notice(Some(&id), text);
                }
                self.active = Some(Active {
                    recorder,
                    session,
                    live,
                    detected: app.is_some(),
                    app_name: app.as_deref().map(app_display_name),
                    stop_announced: false,
                });
            }
            Err(detail) => {
                let (_, action) = advance(session, Event::Fail(hark_meeting::Failure::Capture));
                debug_assert_eq!(action, Action::ReleaseCapture);
                log::error!("meeting could not start: {detail}");
                self.detector.stopped();
                let _ = self.events.send(MeetingEvent::Failed { id: None, detail });
            }
        }
    }

    /// What transcribes the live chunks, and the label stored with the
    /// meeting. `None` engine = no live transcript (setting off, or no key).
    fn live_engine(&self, id: &str) -> (Option<Engine>, String) {
        let label = self.settings.provider.kind.label().to_string();
        if !self.settings.meeting.live_transcript {
            return (None, label);
        }
        if !self.settings.local_stt.mode.uses_cloud() {
            return (
                Some(Engine::Local(Box::new(self.settings.clone()))),
                "local".to_string(),
            );
        }
        let built = hark_keychain::resolve_key(&label)
            .map_err(|e| e.to_string())
            .and_then(|key| crate::provider_config(&self.settings, key).map_err(|e| e.to_string()))
            .and_then(|cfg| {
                let client = hark_stt::shared_client().map_err(|e| e.to_string())?;
                // The batch contract, as dictation's replay uses: a chunk is a
                // finished clip, never a live session.
                hark_stt::build_meeting_chunks(&crate::fallback_config(&cfg), client)
                    .map_err(|e| e.to_string())
            });
        match built {
            Ok(provider) => (Some(Engine::Cloud(provider)), label),
            Err(e) => {
                self.notice(
                    Some(id),
                    format!(
                        "No live transcript for this meeting ({e}). The audio is still recorded."
                    ),
                );
                (None, label)
            }
        }
    }

    /// Whose audio "Them" is: the detected app's process tree when the
    /// setting asks for it and the app can be found, else everything except
    /// Hark (a manual start always).
    fn loopback_target(&self, app: Option<&str>) -> LoopbackTarget {
        let everything_but_hark = LoopbackTarget::ExcludeTree(std::process::id());
        let Some(app) = app else {
            return everything_but_hark;
        };
        if self.settings.meeting.system_source != SystemSource::App {
            return everything_but_hark;
        }
        let exe = detect::target_exe(app);
        match hark_meeting::probe::processes()
            .ok()
            .and_then(|procs| detect::root_pid(&procs, &exe))
        {
            Some(pid) => LoopbackTarget::IncludeTree(pid),
            None => {
                log::warn!("meeting app {exe} has no running process; capturing all system audio");
                everything_but_hark
            }
        }
    }

    fn pump(&mut self) {
        let Some(active) = &mut self.active else {
            return;
        };
        match active.recorder.pump() {
            Ok(lost) => {
                let id = active.recorder.id.clone();
                let empty = active.recorder.is_empty();
                for track in lost {
                    log::warn!("meeting {id}: {:?} track lost", track.channel);
                    self.notice(Some(&id), track.detail);
                }
                if empty {
                    if let Some(active) = self.active.take() {
                        self.detector.stopped();
                        self.close(active, true);
                    }
                }
            }
            Err(e) => {
                let id = active.recorder.id.clone();
                log::error!("meeting {id}: recording failed ({e}); stopping");
                self.notice(
                    Some(&id),
                    format!("Recording stopped: {e}. Everything up to here is kept."),
                );
                if let Some(active) = self.active.take() {
                    self.detector.stopped();
                    self.close(active, true);
                }
            }
        }
    }

    /// Stop capture and hand the meeting to a finisher (or, at quit, just
    /// close it).
    fn close(&mut self, active: Active, finish_it: bool) {
        let Active {
            recorder,
            session,
            live,
            detected,
            ..
        } = active;
        let (session, action) = advance(session, Event::Stop);
        debug_assert_eq!(action, Action::Finalize);
        let id = recorder.id.clone();
        let dir = recorder.dir.clone();
        let ended_ms = jiff::Timestamp::now().as_millisecond();
        // Close the spools first: the recorder drops its sender to the live
        // transcriber, which then drains its queue and exits.
        let recorded = match recorder.stop() {
            Ok(r) => r,
            Err(e) => {
                log::error!("meeting {id}: closing the recording failed ({e})");
                super::recorder::Recorded {
                    id: id.clone(),
                    dir,
                    me_samples: 0,
                    them_samples: 0,
                }
            }
        };
        log::info!(
            "meeting {id} stopped (detected: {detected}; {} + {} samples)",
            recorded.me_samples,
            recorded.them_samples
        );
        let _ = self.events.send(MeetingEvent::Stopped {
            id: id.clone(),
            ended_ms,
        });
        let (jobs, thread, transcript) = match live {
            Some(l) => (Some(l.jobs), Some(l.thread), l.transcript),
            None => (None, None, Arc::new(Mutex::new(Transcript::new()))),
        };
        // The recorder held the other sender clone; this one goes too.
        drop(jobs);
        if !finish_it {
            return;
        }
        finish::spawn(FinishJob {
            recorded,
            live: thread,
            transcript,
            settings: Box::new(self.settings.clone()),
            events: self.events.clone(),
            protected: self.protected.clone(),
            session,
        });
    }

    fn enforce_cap(&self) {
        let protected = self.protected.lock().map(|p| p.clone()).unwrap_or_default();
        let _ = self
            .events
            .send(MeetingEvent::EnforceAudioCap { protected });
    }

    fn notice(&self, id: Option<&str>, text: String) {
        let _ = self.events.send(MeetingEvent::Notice {
            id: id.map(str::to_string),
            text,
        });
    }

    /// Startup: repair spools a crash left open, finish or undo an
    /// interrupted compression, then let the storage cap run.
    fn recover(&self) {
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
            recorded: super::recorder::Recorded {
                me_samples: super::recorder::spool_samples(&dir.join(hark_audio::spool::ME_FILE)),
                them_samples: super::recorder::spool_samples(
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
        });
    }
}

fn detection_wait(backstop: Duration, deadline_ms: Option<u64>, recording: bool) -> Duration {
    let wait = deadline_ms
        .map(Duration::from_millis)
        .map_or(backstop, |deadline| deadline.min(backstop));
    if recording {
        wait.min(DRAIN_EVERY)
    } else {
        wait
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

fn detect_config(settings: &Settings, self_exe: &str) -> DetectConfig {
    let m = &settings.meeting;
    DetectConfig {
        mode: match (m.enabled, m.auto_detect) {
            (false, _) | (_, AutoDetect::Off) => DetectMode::Off,
            (true, AutoDetect::Ask) => DetectMode::Ask,
            (true, AutoDetect::Auto) => DetectMode::Auto,
        },
        apps: m
            .detect_apps
            .clone()
            .unwrap_or_else(|| detect::DEFAULT_APPS.iter().map(|s| s.to_string()).collect()),
        auto_stop_after_ms: m.auto_stop_after_ms(),
        self_exe: self_exe.to_string(),
    }
}

/// A folder-safe id from the local start time, unique within `dir`.
fn new_meeting_id(dir: &std::path::Path) -> String {
    unique_in(
        dir,
        &jiff::Zoned::now().strftime("%Y%m%d-%H%M%S").to_string(),
    )
}

/// `base`, or `base-2`, `base-3`, ...: the first name not already in `dir`.
fn unique_in(dir: &std::path::Path, base: &str) -> String {
    let mut id = base.to_string();
    let mut n = 2;
    while dir.join(&id).exists() {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
}

/// Milliseconds on a monotonic clock, for the detector.
fn monotonic_ms() -> u64 {
    use std::sync::OnceLock;
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

#[cfg(test)]
mod tests {
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
}

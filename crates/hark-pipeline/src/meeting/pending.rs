//! A final pass that could not reach Deepgram is kept and retried later.
//!
//! The schedule lives in a marker file in the meeting's folder, the same
//! survive-a-restart idea as the finishing marker: nothing about a meeting
//! may depend on the app staying open. The database's
//! `transcript_status` mirrors it for the UI; the marker is what drives the
//! retries, so the coordinator never needs the database.
//!
//! One scheduler thread per coordinator sleeps until the next retry is due.
//! A retry takes the meeting's `protected` slot like any other final pass,
//! so it never runs alongside the finisher or a re-run the user started.

use super::status::TranscriptStatus;
use super::MeetingEvent;
use hark_config::{FinalPass, Settings};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// In a meeting's folder while a retry is scheduled.
pub(super) const MARKER: &str = ".transcript-pending";
/// The waits before each retry: 1, 5, 15 and 60 minutes, then daily. Six
/// retries in all; after the last one fails, Hark stops trying.
const BACKOFF_MS: [i64; 6] = [
    60_000,
    5 * 60_000,
    15 * 60_000,
    60 * 60_000,
    24 * 60 * 60_000,
    24 * 60 * 60_000,
];
/// The scheduler re-reads the wall clock at least this often, so a laptop
/// that slept through a retry time catches up soon after it wakes.
const RECHECK: Duration = Duration::from_secs(60);
/// A retry that finds its meeting busy waits this long.
const BUSY_WAIT_MS: i64 = 60_000;
/// A retry found at startup runs this soon, whatever its schedule said:
/// the relaunch may well be on a working connection.
const STARTUP_DELAY_MS: i64 = 30_000;

/// The schedule persisted in [`MARKER`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Pending {
    /// Retries already made (the first final pass is not one).
    pub retries: u32,
    /// Unix ms the next retry is due.
    pub next_ms: i64,
    /// The recording's length, for clamping the provider's timestamps.
    pub audio_ms: u64,
    /// The finisher made no notes (it had no transcript to make them from),
    /// so a successful retry should.
    pub needs_notes: bool,
}

impl Pending {
    /// The schedule after the first final pass failed at `now_ms`.
    pub fn first(now_ms: i64, audio_ms: u64, needs_notes: bool) -> Self {
        Pending {
            retries: 0,
            next_ms: now_ms + BACKOFF_MS[0],
            audio_ms,
            needs_notes,
        }
    }

    /// The schedule after a retry failed at `now_ms`, or `None` when that was
    /// the last one.
    pub fn after_failed_retry(self, now_ms: i64) -> Option<Self> {
        let retries = self.retries + 1;
        BACKOFF_MS.get(retries as usize).map(|wait| Pending {
            retries,
            next_ms: now_ms + wait,
            ..self
        })
    }

    pub fn status(self) -> TranscriptStatus {
        TranscriptStatus::Pending {
            retry_at_ms: Some(self.next_ms),
        }
    }

    fn to_text(self) -> String {
        format!(
            "retries={}\nnext_ms={}\naudio_ms={}\nneeds_notes={}\n",
            self.retries, self.next_ms, self.audio_ms, self.needs_notes as u8
        )
    }

    fn parse(text: &str) -> Option<Self> {
        let field = |name: &str| {
            text.lines()
                .find_map(|line| line.strip_prefix(name)?.strip_prefix('='))
                .map(str::trim)
        };
        Some(Pending {
            retries: field("retries")?.parse().ok()?,
            next_ms: field("next_ms")?.parse().ok()?,
            audio_ms: field("audio_ms")?.parse().ok()?,
            needs_notes: field("needs_notes")? == "1",
        })
    }
}

/// The meeting's schedule, if it has one. A damaged marker is treated as a
/// first failure due now, never silently dropped.
pub(super) fn read(dir: &Path) -> Option<Pending> {
    let text = std::fs::read_to_string(dir.join(MARKER)).ok()?;
    Some(Pending::parse(&text).unwrap_or(Pending {
        retries: 0,
        next_ms: 0,
        audio_ms: 0,
        needs_notes: false,
    }))
}

/// Write via a temporary file and a rename, so a crash mid-write cannot
/// leave a half-written schedule.
pub(super) fn write(dir: &Path, pending: Pending) -> std::io::Result<()> {
    let tmp = dir.join(format!("{MARKER}.tmp"));
    std::fs::write(&tmp, pending.to_text())?;
    std::fs::rename(&tmp, dir.join(MARKER))
}

pub(super) fn remove(dir: &Path) {
    match std::fs::remove_file(dir.join(MARKER)) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => log::warn!(
            "pending-transcript marker not removed in {} ({e})",
            dir.display()
        ),
    }
}

enum Msg {
    /// Look at this meeting's marker and (re)schedule it.
    Check {
        id: String,
        startup: bool,
    },
    Settings(Box<Settings>),
    Shutdown,
}

/// The coordinator's handle on its scheduler.
pub(super) struct Retries {
    tx: Sender<Msg>,
    stop: Arc<AtomicBool>,
}

/// What a finisher holds to hand a meeting to the scheduler.
#[derive(Clone)]
pub(super) struct Queue(Sender<Msg>);

impl Queue {
    pub fn schedule(&self, id: &str) {
        // Only fails once the scheduler has gone (quitting); the marker is
        // on disk, and the next start picks it up.
        let _ = self.0.send(Msg::Check {
            id: id.to_string(),
            startup: false,
        });
    }
}

impl Retries {
    /// Start the scheduler. It waits for work and does nothing until a
    /// meeting is scheduled.
    pub fn spawn(
        root: PathBuf,
        settings: Settings,
        events: Sender<MeetingEvent>,
        protected: Arc<Mutex<Vec<String>>>,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let scheduler = Scheduler {
            root,
            settings,
            events,
            protected,
            stop: stop.clone(),
            due: BTreeMap::new(),
        };
        if let Err(e) = std::thread::Builder::new()
            .name("hark-meeting-retry".to_string())
            .spawn(move || scheduler.run(rx))
        {
            // The markers stay on disk; the next start tries again.
            log::error!("cannot start the meeting retry scheduler: {e}");
        }
        Retries { tx, stop }
    }

    /// A handle with no thread behind it, for tests that never retry.
    #[cfg(test)]
    pub fn disabled() -> Self {
        let (tx, _) = mpsc::channel();
        Retries {
            tx,
            stop: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn queue(&self) -> Queue {
        Queue(self.tx.clone())
    }

    /// A meeting found with a marker at startup.
    pub fn resume(&self, id: &str) {
        let _ = self.tx.send(Msg::Check {
            id: id.to_string(),
            startup: true,
        });
    }

    pub fn update_settings(&self, settings: &Settings) {
        let _ = self.tx.send(Msg::Settings(Box::new(settings.clone())));
    }

    /// Stop scheduling. A retry already uploading finishes on its own (the
    /// finisher does the same); the markers keep everything else.
    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::Release);
        let _ = self.tx.send(Msg::Shutdown);
    }
}

struct Scheduler {
    root: PathBuf,
    settings: Settings,
    events: Sender<MeetingEvent>,
    protected: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    /// Meeting id -> unix ms its next retry is due.
    due: BTreeMap<String, i64>,
}

impl Scheduler {
    fn run(mut self, rx: Receiver<Msg>) {
        loop {
            let msg = match self.due.values().min() {
                // Nothing scheduled: sleep until something is.
                None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
                Some(&next) => {
                    let wait = (next - unix_now_ms()).clamp(0, RECHECK.as_millis() as i64);
                    rx.recv_timeout(Duration::from_millis(wait as u64))
                }
            };
            match msg {
                Ok(Msg::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                Ok(Msg::Settings(settings)) => self.settings = *settings,
                Ok(Msg::Check { id, startup }) => self.check(&id, startup),
                Err(RecvTimeoutError::Timeout) => {}
            }
            let now = unix_now_ms();
            let ready: Vec<String> = self
                .due
                .iter()
                .filter(|(_, &at)| at <= now)
                .map(|(id, _)| id.clone())
                .collect();
            for id in ready {
                if self.stop.load(Ordering::Acquire) {
                    return;
                }
                self.attempt(&id);
            }
        }
    }

    fn check(&mut self, id: &str, startup: bool) {
        match read(&self.root.join(id)) {
            Some(pending) => {
                let at = if startup {
                    pending.next_ms.min(unix_now_ms() + STARTUP_DELAY_MS)
                } else {
                    pending.next_ms
                };
                log::info!(
                    "meeting {id}: final pass retry {} scheduled",
                    pending.retries + 1
                );
                self.due.insert(id.to_string(), at);
            }
            None => {
                self.due.remove(id);
            }
        }
    }

    fn attempt(&mut self, id: &str) {
        let dir = self.root.join(id);
        // The marker is the truth: a re-run that succeeded removed it, and a
        // finisher that failed again rewrote it (and rescheduled).
        let Some(pending) = read(&dir) else {
            self.due.remove(id);
            return;
        };
        match self.protected.lock() {
            Ok(mut ids) if !ids.iter().any(|x| x == id) => ids.push(id.to_string()),
            _ => {
                self.due
                    .insert(id.to_string(), unix_now_ms() + BUSY_WAIT_MS);
                return;
            }
        }
        self.due.remove(id);
        if self.settings.meeting.final_pass != FinalPass::Deepgram {
            // Turned off since: no upload the user no longer wants.
            log::info!("meeting {id}: final pass turned off; retries dropped");
            remove(&dir);
            self.status(id, None);
        } else {
            self.retry(id, &dir, pending);
        }
        if let Ok(mut ids) = self.protected.lock() {
            ids.retain(|x| x != id);
            let _ = self.events.send(MeetingEvent::EnforceAudioCap {
                protected: ids.clone(),
            });
        }
    }

    fn retry(&mut self, id: &str, dir: &Path, pending: Pending) {
        log::info!(
            "meeting {id}: final pass retry {} of {}",
            pending.retries + 1,
            BACKOFF_MS.len()
        );
        match super::rerun::deepgram_pass(&self.root, id, pending.audio_ms, &self.settings) {
            Ok(segments) => {
                let lines = super::finish::NotesInput::of(&segments);
                let speakers = super::speakers::lines_of(&segments);
                let _ = self.events.send(MeetingEvent::Refined {
                    id: id.to_string(),
                    segments,
                });
                super::speakers::name_speakers(&self.settings, id, &speakers, &self.events);
                if pending.needs_notes && self.settings.meeting.summary {
                    super::finish::send_notes(&self.settings, id, &lines, &self.events);
                }
                remove(dir);
                self.status(id, None);
                self.notice(id, "The transcript for this meeting is ready.".to_string());
            }
            Err(failure) if failure.retryable() => {
                match pending.after_failed_retry(unix_now_ms()) {
                    Some(next) => match write(dir, next) {
                        Ok(()) => {
                            self.due.insert(id.to_string(), next.next_ms);
                            self.status(id, Some(next.status()));
                        }
                        Err(e) => {
                            log::warn!("meeting {id}: retry not rescheduled ({e})");
                            self.status(id, Some(TranscriptStatus::Failed(failure)));
                        }
                    },
                    None => {
                        log::info!("meeting {id}: final pass retries exhausted");
                        remove(dir);
                        self.status(id, Some(TranscriptStatus::Failed(failure)));
                        self.notice(
                            id,
                            "Deepgram could not be reached after several tries. Open the \
                             meeting and choose Transcribe recording to try again."
                                .to_string(),
                        );
                    }
                }
            }
            Err(failure) => {
                remove(dir);
                self.status(id, Some(TranscriptStatus::Failed(failure)));
                self.notice(id, failure.message().to_string());
            }
        }
    }

    fn status(&self, id: &str, status: Option<TranscriptStatus>) {
        let _ = self.events.send(MeetingEvent::TranscriptStatus {
            id: id.to_string(),
            status,
        });
    }

    fn notice(&self, id: &str, text: String) {
        let _ = self.events.send(MeetingEvent::Notice {
            id: Some(id.to_string()),
            text,
        });
    }
}

pub(super) fn unix_now_ms() -> i64 {
    jiff::Timestamp::now().as_millisecond()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: i64 = 60_000;

    #[test]
    fn the_backoff_is_1_5_15_60_minutes_then_daily_and_stops_after_six() {
        let mut p = Pending::first(0, 1_000, false);
        let mut waits = vec![p.next_ms];
        let mut now = p.next_ms;
        while let Some(next) = p.after_failed_retry(now) {
            waits.push(next.next_ms - now);
            now = next.next_ms;
            p = next;
        }
        assert_eq!(
            waits,
            [
                MIN,
                5 * MIN,
                15 * MIN,
                60 * MIN,
                24 * 60 * MIN,
                24 * 60 * MIN
            ]
        );
        assert_eq!(p.retries, 5, "the sixth retry is the last");
        assert_eq!(p.after_failed_retry(now), None);
    }

    #[test]
    fn the_schedule_round_trips_through_the_marker() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read(dir.path()), None);
        let p = Pending {
            retries: 3,
            next_ms: 1_760_000_000_000,
            audio_ms: 2_434_000,
            needs_notes: true,
        };
        write(dir.path(), p).unwrap();
        assert_eq!(read(dir.path()), Some(p));
        assert!(!dir.path().join(format!("{MARKER}.tmp")).exists());
        remove(dir.path());
        assert_eq!(read(dir.path()), None);
        remove(dir.path());
    }

    #[test]
    fn a_damaged_marker_is_retried_now_rather_than_forgotten() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(MARKER), "retries=x").unwrap();
        let p = read(dir.path()).expect("still pending");
        assert_eq!((p.retries, p.next_ms), (0, 0));
    }

    #[test]
    fn a_retry_keeps_the_recording_facts() {
        let p = Pending::first(10, 5_000, true)
            .after_failed_retry(MIN)
            .unwrap();
        assert_eq!((p.audio_ms, p.needs_notes, p.retries), (5_000, true, 1));
        assert_eq!(
            p.status(),
            TranscriptStatus::Pending {
                retry_at_ms: Some(MIN + 5 * MIN)
            }
        );
    }

    fn scheduler(root: &Path) -> (Scheduler, Receiver<MeetingEvent>) {
        let (events, rx) = mpsc::channel();
        let scheduler = Scheduler {
            root: root.to_path_buf(),
            settings: Settings::default(),
            events,
            protected: Arc::new(Mutex::new(Vec::new())),
            stop: Arc::new(AtomicBool::new(false)),
            due: BTreeMap::new(),
        };
        (scheduler, rx)
    }

    #[test]
    fn startup_brings_a_distant_retry_forward_and_a_missing_marker_unschedules() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("m");
        std::fs::create_dir(&dir).unwrap();
        let far = unix_now_ms() + 24 * 60 * MIN;
        write(
            &dir,
            Pending {
                retries: 4,
                next_ms: far,
                audio_ms: 1,
                needs_notes: false,
            },
        )
        .unwrap();
        let (mut s, _rx) = scheduler(root.path());
        s.check("m", false);
        assert_eq!(s.due.get("m"), Some(&far));
        s.check("m", true);
        assert!(s.due["m"] <= unix_now_ms() + STARTUP_DELAY_MS);
        remove(&dir);
        s.check("m", false);
        assert!(s.due.is_empty());
    }

    #[test]
    fn a_busy_meeting_is_postponed_without_spending_a_retry() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("m");
        std::fs::create_dir(&dir).unwrap();
        let p = Pending::first(0, 1_000, false);
        write(&dir, p).unwrap();
        let (mut s, rx) = scheduler(root.path());
        s.protected.lock().unwrap().push("m".to_string());
        s.due.insert("m".to_string(), 0);
        s.attempt("m");
        assert!(s.due["m"] > unix_now_ms(), "tried again later");
        assert_eq!(read(&dir), Some(p), "no retry was spent");
        assert!(rx.try_recv().is_err(), "nothing reported");
    }

    #[test]
    fn a_turned_off_final_pass_drops_the_schedule() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("m");
        std::fs::create_dir(&dir).unwrap();
        write(&dir, Pending::first(0, 1_000, false)).unwrap();
        let (mut s, rx) = scheduler(root.path());
        s.settings.meeting.final_pass = FinalPass::None;
        s.due.insert("m".to_string(), 0);
        s.attempt("m");
        assert_eq!(read(&dir), None);
        assert!(s.due.is_empty());
        assert!(matches!(
            rx.try_recv(),
            Ok(MeetingEvent::TranscriptStatus { status: None, .. })
        ));
        assert!(s.protected.lock().unwrap().is_empty(), "slot released");
    }
}

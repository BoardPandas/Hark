//! Meeting mode as the UI sees it: start/stop, the detection prompt, the live
//! transcript of the meeting in progress, and notices. The work itself runs on
//! the hark-pipeline meeting threads; this module never blocks the UI thread.
//!
//! Same shape as `pipeline.rs`: a pump thread receives worker events, tees the
//! database writes to the storage worker (the one writer), forwards the rest
//! to the UI, and wakes the loop with `wake_ui` — so a hidden window still
//! records every line and shows the prompt.

use crate::storage::meetings::MeetingCmd;
use crate::storage::StorageCmd;
use hark_config::Settings;
use hark_pipeline::meeting::{self, LiveSegment, MeetingEvent, MeetingHandle};
use hark_store::{MeetingSegment, NewMeeting};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Instant;

pub use hark_pipeline::meeting::Answer;

/// Notices kept for the Meetings page; older ones drop off.
const MAX_NOTICES: usize = 5;

// Debug is safe: ids, times and status labels, never meeting content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MeetingStatus {
    /// Meetings are off here (unsupported platform, disabled, or failed to start).
    Unavailable(String),
    Idle,
    Recording {
        id: String,
        /// Unix ms.
        started_ms: i64,
        system_audio: bool,
    },
}

/// One live transcript line for the meeting in progress (or the last one).
pub struct LiveLine {
    pub channel: u8,
    pub start_ms: u64,
    pub text: String,
}

/// An open detection prompt.
pub struct Prompt {
    pub name: String,
    pub shown: Instant,
}

pub struct MeetingController {
    handle: Option<MeetingHandle>,
    events: Option<Receiver<MeetingEvent>>,
    storage: Option<Sender<StorageCmd>>,
    /// Read by the pump for every cap check, so a lowered cap applies at once.
    cap_bytes: Arc<AtomicU64>,
    status: MeetingStatus,
    live_id: Option<String>,
    live: Vec<LiveLine>,
    /// Stopped meetings still being refined, summarized or archived.
    finishing: Vec<String>,
    prompt: Option<Prompt>,
    notices: Vec<String>,
    /// The detected app hung up: (unix ms the notes stop at, app name).
    auto_stop: Option<(i64, String)>,
}

impl MeetingController {
    pub fn new(storage: Option<Sender<StorageCmd>>) -> Self {
        MeetingController {
            handle: None,
            events: None,
            storage,
            cap_bytes: Arc::new(AtomicU64::new(0)),
            status: MeetingStatus::Unavailable("Not started".to_string()),
            live_id: None,
            live: Vec::new(),
            finishing: Vec::new(),
            prompt: None,
            notices: Vec::new(),
            auto_stop: None,
        }
    }

    /// Close out meetings a previous run left open, then start the
    /// coordinator (if meetings are supported and enabled). Called once at
    /// startup; later settings changes go through [`Self::apply_settings`].
    pub fn start(&mut self, settings: &Settings, ctx: &egui::Context) {
        if let Some(tx) = &self.storage {
            let _ = tx.send(StorageCmd::Meeting(MeetingCmd::Recover));
        }
        self.launch(settings, ctx);
    }

    fn launch(&mut self, settings: &Settings, ctx: &egui::Context) {
        self.cap_bytes
            .store(settings.meeting.audio_cap_bytes(), Ordering::Relaxed);
        if !meeting::meetings_supported() {
            self.status = MeetingStatus::Unavailable(
                "Meeting notes are available on Windows for now.".to_string(),
            );
            return;
        }
        if !settings.meeting.enabled {
            self.status =
                MeetingStatus::Unavailable("Meeting notes are turned off in Settings.".to_string());
            return;
        }
        let (tx, rx) = mpsc::channel();
        match meeting::run(settings, tx) {
            Ok(handle) => {
                self.handle = Some(handle);
                self.events = Some(spawn_pump(
                    rx,
                    ctx.clone(),
                    self.storage.clone(),
                    self.cap_bytes.clone(),
                ));
                self.status = MeetingStatus::Idle;
            }
            Err(detail) => self.status = MeetingStatus::Unavailable(detail),
        }
    }

    /// Saved settings: applied live, never by restarting the coordinator
    /// mid-meeting (that would cut the recording).
    pub fn apply_settings(&mut self, settings: &Settings, ctx: &egui::Context) {
        self.cap_bytes
            .store(settings.meeting.audio_cap_bytes(), Ordering::Relaxed);
        match (&self.handle, settings.meeting.enabled) {
            (Some(handle), true) => handle.update_settings(settings),
            (Some(_), false) if !self.is_recording() => {
                self.handle = None;
                self.events = None;
                self.prompt = None;
                self.status = MeetingStatus::Unavailable(
                    "Meeting notes are turned off in Settings.".to_string(),
                );
            }
            // Turned off mid-meeting: this meeting finishes first.
            (Some(handle), false) => handle.update_settings(settings),
            (None, true) => self.launch(settings, ctx),
            (None, false) => self.launch(settings, ctx),
        }
    }

    pub fn status(&self) -> &MeetingStatus {
        &self.status
    }

    pub fn is_available(&self) -> bool {
        self.handle.is_some()
    }

    pub fn is_recording(&self) -> bool {
        matches!(self.status, MeetingStatus::Recording { .. })
    }

    pub fn start_manual(&self) {
        if let Some(h) = &self.handle {
            h.start_manual();
        }
    }

    pub fn stop(&self) {
        if let Some(h) = &self.handle {
            h.stop();
        }
    }

    /// The user answered the detection prompt.
    pub fn answer(&mut self, answer: Answer) {
        self.prompt = None;
        if let Some(h) = &self.handle {
            h.answer(answer);
        }
    }

    pub fn prompt(&self) -> Option<&Prompt> {
        self.prompt.as_ref()
    }

    /// Live lines of the meeting in progress, or of the one that just ended.
    pub fn live(&self) -> (Option<&str>, &[LiveLine]) {
        (self.live_id.as_deref(), &self.live)
    }

    pub fn finishing(&self) -> &[String] {
        &self.finishing
    }

    /// When a detected meeting will stop on its own because its app released
    /// the mic, and which app: (unix ms, name).
    pub fn auto_stop(&self) -> Option<(i64, &str)> {
        self.auto_stop.as_ref().map(|(at, app)| (*at, app.as_str()))
    }

    pub fn notices(&self) -> &[String] {
        &self.notices
    }

    pub fn dismiss_notices(&mut self) {
        self.notices.clear();
    }

    /// Drain pending events; called from `App::logic` every frame. Never blocks.
    pub fn drain_events(&mut self) {
        let Some(rx) = &self.events else { return };
        let events: Vec<MeetingEvent> = rx.try_iter().collect();
        for event in events {
            self.on_event(event);
        }
    }

    fn on_event(&mut self, event: MeetingEvent) {
        match event {
            MeetingEvent::Prompt { name, .. } => {
                self.prompt = Some(Prompt {
                    name,
                    shown: Instant::now(),
                })
            }
            MeetingEvent::PromptRetracted => self.prompt = None,
            MeetingEvent::Started(s) => {
                self.prompt = None;
                self.auto_stop = None;
                self.live_id = Some(s.id.clone());
                self.live.clear();
                self.status = MeetingStatus::Recording {
                    id: s.id,
                    started_ms: s.started_ms,
                    system_audio: s.system_audio,
                };
            }
            MeetingEvent::Segment { id, segment } => {
                if self.live_id.as_deref() == Some(id.as_str()) {
                    insert_live(&mut self.live, segment);
                }
            }
            MeetingEvent::AutoStopPending { at_ms, app, .. } => {
                self.auto_stop = Some((at_ms, app));
            }
            MeetingEvent::AutoStopCancelled { .. } => self.auto_stop = None,
            MeetingEvent::Stopped { id, .. } => {
                self.auto_stop = None;
                if matches!(&self.status, MeetingStatus::Recording { id: r, .. } if *r == id) {
                    self.status = MeetingStatus::Idle;
                }
                if !self.finishing.contains(&id) {
                    self.finishing.push(id);
                }
            }
            MeetingEvent::Finished { id, .. } => self.finishing.retain(|x| x != &id),
            MeetingEvent::Notice { text, .. } => self.push_notice(text),
            MeetingEvent::Failed { detail, .. } => {
                self.push_notice(format!("The meeting could not be recorded: {detail}"))
            }
            // Database-only events: the pump already sent them to storage.
            MeetingEvent::Refined { .. }
            | MeetingEvent::Notes { .. }
            | MeetingEvent::EnforceAudioCap { .. } => {}
        }
    }

    fn push_notice(&mut self, text: String) {
        if self.notices.last() == Some(&text) {
            return;
        }
        self.notices.push(text);
        if self.notices.len() > MAX_NOTICES {
            self.notices.remove(0);
        }
    }
}

/// Keep the live pane in timeline order as out-of-order chunks land.
fn insert_live(lines: &mut Vec<LiveLine>, s: LiveSegment) {
    let at = lines.partition_point(|l| (l.start_ms, l.channel) <= (s.start_ms, s.channel));
    lines.insert(
        at,
        LiveLine {
            channel: s.channel,
            start_ms: s.start_ms,
            text: s.text,
        },
    );
}

/// Forward worker events to the UI and tee the database writes to storage.
/// Ends when the coordinator and every finisher have dropped their senders.
fn spawn_pump(
    rx: Receiver<MeetingEvent>,
    ctx: egui::Context,
    storage: Option<Sender<StorageCmd>>,
    cap_bytes: Arc<AtomicU64>,
) -> Receiver<MeetingEvent> {
    let (tx, ui_rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("hark-meeting-pump".to_string())
        .spawn(move || {
            for event in rx {
                if let (Some(storage), Some(cmd)) = (&storage, storage_cmd(&event, &cap_bytes)) {
                    let _ = storage.send(StorageCmd::Meeting(cmd));
                }
                // The UI may be gone at shutdown; storage writes above still land.
                let _ = tx.send(event);
                crate::app::wake_ui(&ctx);
            }
        })
        .expect("spawning the meeting pump cannot fail");
    ui_rx
}

/// The database write an event implies, if any.
fn storage_cmd(event: &MeetingEvent, cap_bytes: &AtomicU64) -> Option<MeetingCmd> {
    Some(match event {
        MeetingEvent::Started(s) => MeetingCmd::Started(NewMeeting {
            id: s.id.clone(),
            started_ms: s.started_ms,
            trigger: s.trigger.label().to_string(),
            app_hint: s.app.clone(),
            stt_provider: s.stt_provider.clone(),
        }),
        MeetingEvent::Segment { id, segment } => MeetingCmd::Segment {
            id: id.clone(),
            segment: stored(segment),
        },
        MeetingEvent::Stopped { id, ended_ms } => MeetingCmd::Stopped {
            id: id.clone(),
            ended_ms: *ended_ms,
        },
        MeetingEvent::Refined { id, segments } => MeetingCmd::Refined {
            id: id.clone(),
            segments: segments.iter().map(stored).collect(),
        },
        MeetingEvent::Notes {
            id,
            notes_json,
            title,
        } => MeetingCmd::Notes {
            id: id.clone(),
            notes_json: notes_json.clone(),
            title: title.clone(),
        },
        MeetingEvent::Finished { id, audio_bytes } => MeetingCmd::Finished {
            id: id.clone(),
            audio_bytes: *audio_bytes,
        },
        MeetingEvent::EnforceAudioCap { protected } => MeetingCmd::EnforceCap {
            cap_bytes: cap_bytes.load(Ordering::Relaxed),
            protected: protected.clone(),
        },
        MeetingEvent::Prompt { .. }
        | MeetingEvent::PromptRetracted
        | MeetingEvent::AutoStopPending { .. }
        | MeetingEvent::AutoStopCancelled { .. }
        | MeetingEvent::Notice { .. }
        | MeetingEvent::Failed { .. } => return None,
    })
}

fn stored(s: &LiveSegment) -> MeetingSegment {
    MeetingSegment {
        start_ms: s.start_ms as i64,
        end_ms: s.end_ms as i64,
        channel: s.channel,
        speaker: s.speaker,
        text: s.text.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(channel: u8, start_ms: u64, text: &str) -> LiveSegment {
        LiveSegment {
            channel,
            speaker: None,
            start_ms,
            end_ms: start_ms + 30_000,
            text: text.to_string(),
        }
    }

    #[test]
    fn live_lines_stay_in_timeline_order() {
        let mut lines = Vec::new();
        insert_live(&mut lines, seg(1, 30_000, "b"));
        insert_live(&mut lines, seg(0, 0, "a"));
        insert_live(&mut lines, seg(0, 30_000, "c"));
        let order: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(order, ["a", "c", "b"], "Me before Them on a tie");
    }

    #[test]
    fn content_events_become_database_writes_and_ui_events_do_not() {
        let cap = AtomicU64::new(42);
        assert!(matches!(
            storage_cmd(
                &MeetingEvent::EnforceAudioCap {
                    protected: vec!["a".into()]
                },
                &cap
            ),
            Some(MeetingCmd::EnforceCap { cap_bytes: 42, .. })
        ));
        assert!(storage_cmd(&MeetingEvent::PromptRetracted, &cap).is_none());
        assert!(matches!(
            storage_cmd(
                &MeetingEvent::Segment {
                    id: "m".into(),
                    segment: seg(1, 5, "x")
                },
                &cap
            ),
            Some(MeetingCmd::Segment { .. })
        ));
    }

    #[test]
    fn a_pending_auto_stop_shows_until_cancelled_or_stopped() {
        let mut c = MeetingController::new(None);
        c.on_event(MeetingEvent::AutoStopPending {
            id: "m".into(),
            at_ms: 1_000,
            app: "Teams".into(),
        });
        assert_eq!(c.auto_stop(), Some((1_000, "Teams")));
        c.on_event(MeetingEvent::AutoStopCancelled { id: "m".into() });
        assert_eq!(c.auto_stop(), None);
        c.on_event(MeetingEvent::AutoStopPending {
            id: "m".into(),
            at_ms: 2_000,
            app: "Teams".into(),
        });
        c.on_event(MeetingEvent::Stopped {
            id: "m".into(),
            ended_ms: 2_000,
        });
        assert_eq!(c.auto_stop(), None);
    }

    #[test]
    fn a_stopped_meeting_is_finishing_until_finished() {
        let mut c = MeetingController::new(None);
        c.status = MeetingStatus::Recording {
            id: "m".into(),
            started_ms: 0,
            system_audio: true,
        };
        c.on_event(MeetingEvent::Stopped {
            id: "m".into(),
            ended_ms: 1,
        });
        assert_eq!(c.status, MeetingStatus::Idle);
        assert_eq!(c.finishing(), ["m"]);
        c.on_event(MeetingEvent::Finished {
            id: "m".into(),
            audio_bytes: 0,
        });
        assert!(c.finishing().is_empty());
    }
}

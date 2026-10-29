//! Rewriting a meeting's notes after its lines moved between speakers (spec
//! §2.7), and after a re-run of the final pass. One refresh runs per meeting
//! at a time; a newer request replaces the queued one, and a result that a
//! newer request has superseded, or that a re-run has made meaningless, is
//! discarded instead of written. The title is never touched.
//!
//! The worker only re-summarizes. Carrying the user's ticks across happens
//! in the storage worker, against the notes as stored when the result
//! lands, so a tick made while the refresh ran is not lost.

use super::finish::NotesInput;
use super::speakers::{failure_slug, notes_provider};
use super::MeetingEvent;
use hark_config::Settings;
use hark_meeting::SpeakerKey;
use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

/// What to summarize: every line with the speaker it is shown as now.
/// Carries transcript text: no `Debug`.
pub struct NotesRequest {
    pub id: String,
    /// `(start_ms, effective speaker, text)` in transcript order.
    pub lines: Vec<(u64, SpeakerKey, String)>,
    /// Unix ms the request was made; the stale stamp clears only if it is
    /// not newer than this.
    pub requested_ms: i64,
}

#[derive(Default)]
struct Slot {
    running: bool,
    queued: Option<NotesRequest>,
    /// A re-run started while this one ran: its result means nothing now.
    discard: bool,
}

/// What a finished refresh does with its result.
enum Done {
    Write,
    Drop,
    /// A newer request was waiting: drop this result and run that one.
    Next(NotesRequest),
}

/// Per-meeting refresh state, pure so the coalescing is testable.
#[derive(Default)]
struct Slots(HashMap<String, Slot>);

impl Slots {
    /// `Some` when the request should start now; otherwise it is queued,
    /// replacing any request already waiting.
    fn request(&mut self, request: NotesRequest) -> Option<NotesRequest> {
        let slot = self.0.entry(request.id.clone()).or_default();
        if slot.running {
            slot.queued = Some(request);
            None
        } else {
            slot.running = true;
            slot.discard = false;
            Some(request)
        }
    }

    fn finished(&mut self, id: &str) -> Done {
        let Some(slot) = self.0.get_mut(id) else {
            return Done::Drop;
        };
        if let Some(next) = slot.queued.take() {
            slot.discard = false;
            return Done::Next(next);
        }
        let discard = slot.discard;
        self.0.remove(id);
        if discard {
            Done::Drop
        } else {
            Done::Write
        }
    }

    /// A re-run started: whatever is running or waiting for `id` is moot.
    fn cancel(&mut self, id: &str) {
        if let Some(slot) = self.0.get_mut(id) {
            slot.queued = None;
            slot.discard = true;
        }
    }
}

/// Shared by the coordinator (requests, re-run cancels) and the re-run
/// worker (its own refresh once the new transcript is saved).
#[derive(Clone, Default)]
pub(super) struct Refreshes(Arc<Mutex<Slots>>);

impl Refreshes {
    pub fn request(
        &self,
        request: NotesRequest,
        settings: &Settings,
        events: &Sender<MeetingEvent>,
    ) {
        let Some(start) = self.lock(|s| s.request(request)).flatten() else {
            return;
        };
        let (refreshes, settings, events) = (self.clone(), settings.clone(), events.clone());
        let id = start.id.clone();
        let spawned = std::thread::Builder::new()
            .name("hark-meeting-notes".into())
            .spawn(move || refreshes.run(start, &settings, &events));
        if let Err(e) = spawned {
            log::warn!("meeting {id}: notes refresh not started ({e})");
            self.lock(|s| s.0.remove(&id));
        }
    }

    pub fn cancel(&self, id: &str) {
        self.lock(|s| s.cancel(id));
    }

    fn lock<R>(&self, f: impl FnOnce(&mut Slots) -> R) -> Option<R> {
        self.0.lock().ok().map(|mut s| f(&mut s))
    }

    fn run(&self, mut request: NotesRequest, settings: &Settings, events: &Sender<MeetingEvent>) {
        loop {
            let id = request.id.clone();
            let _ = events.send(MeetingEvent::NotesRefreshStarted { id: id.clone() });
            let result = summarize(settings, &request);
            match self.lock(|s| s.finished(&id)).unwrap_or(Done::Drop) {
                Done::Write => {
                    let _ = events.send(match result {
                        Ok(notes_json) => MeetingEvent::NotesRefreshed {
                            id,
                            notes_json,
                            requested_ms: request.requested_ms,
                        },
                        Err(reason) => {
                            log::info!("meeting {id}: notes refresh failed ({reason})");
                            MeetingEvent::NotesRefreshFailed { id, reason }
                        }
                    });
                    return;
                }
                Done::Drop => {
                    log::info!("meeting {id}: superseded notes refresh discarded");
                    return;
                }
                Done::Next(next) => request = next,
            }
        }
    }
}

/// The new notes as JSON, or a slug saying why not.
fn summarize(settings: &Settings, request: &NotesRequest) -> Result<String, &'static str> {
    if !settings.meeting.summary {
        return Err("notes-off");
    }
    let mut input = NotesInput::default();
    for (start_ms, key, text) in &request.lines {
        input.push(*start_ms, *key, text);
    }
    let config = notes_provider(settings).map_err(|_| "no-provider")?;
    let client = hark_stt::shared_client().map_err(|_| "network")?;
    hark_voice::summarize(
        &client,
        &config,
        &input.lines.join("\n"),
        settings.meeting.summary_template.as_deref(),
        &input.labels,
    )
    .map(|notes| notes.to_json())
    .map_err(|e| failure_slug(&e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(id: &str, n: i64) -> NotesRequest {
        NotesRequest {
            id: id.into(),
            lines: Vec::new(),
            requested_ms: n,
        }
    }

    #[test]
    fn five_quick_requests_run_at_most_two_jobs_and_only_the_last_is_written() {
        let mut slots = Slots::default();
        let mut started = Vec::new();
        for n in 1..=5 {
            if let Some(r) = slots.request(req("m", n)) {
                started.push(r.requested_ms);
            }
        }
        assert_eq!(started, [1], "the rest wait behind the first");
        // The first finishes: its result is dropped and the newest runs.
        let Done::Next(next) = slots.finished("m") else {
            panic!("the queued request must run next");
        };
        assert_eq!(next.requested_ms, 5);
        assert!(matches!(slots.finished("m"), Done::Write));
        assert!(slots.0.is_empty(), "nothing left once written");
        assert_eq!(slots.request(req("m", 6)).map(|r| r.requested_ms), Some(6));
    }

    #[test]
    fn a_rerun_discards_the_running_refresh_and_its_queue() {
        let mut slots = Slots::default();
        assert!(slots.request(req("m", 1)).is_some());
        assert!(slots.request(req("m", 2)).is_none());
        slots.cancel("m");
        assert!(matches!(slots.finished("m"), Done::Drop));
        // A request made after the re-run (its own refresh) runs and writes.
        assert!(slots.request(req("m", 3)).is_some());
        assert!(matches!(slots.finished("m"), Done::Write));
    }

    #[test]
    fn a_request_after_a_rerun_waits_for_the_moot_one_then_writes() {
        let mut slots = Slots::default();
        slots.request(req("m", 1));
        slots.cancel("m");
        assert!(slots.request(req("m", 2)).is_none(), "one job per meeting");
        assert!(matches!(slots.finished("m"), Done::Next(r) if r.requested_ms == 2));
        assert!(matches!(slots.finished("m"), Done::Write));
    }

    #[test]
    fn meetings_never_coalesce_with_each_other() {
        let mut slots = Slots::default();
        assert!(slots.request(req("a", 1)).is_some());
        assert!(slots.request(req("b", 1)).is_some());
        slots.cancel("a");
        assert!(matches!(slots.finished("b"), Done::Write));
        assert!(matches!(slots.finished("a"), Done::Drop));
    }

    #[test]
    fn with_notes_off_a_refresh_fails_without_asking_anyone() {
        let mut settings = Settings::default();
        settings.meeting.summary = false;
        let request = NotesRequest {
            id: "m".into(),
            lines: vec![(0, SpeakerKey::Me, "hello".into())],
            requested_ms: 1,
        };
        assert_eq!(summarize(&settings, &request), Err("notes-off"));
    }
}

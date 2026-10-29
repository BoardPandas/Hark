//! One meeting: title, notes (with tickable action items), speakers, the
//! transcript, sharing, and delete. Cached on (write generation, id).

use super::assign::Assignments;
use super::notes;
use super::share::{self, ShareAction, Sharing};
use super::speakers::SpeakersCard;
use super::transcript::TranscriptView;
use crate::storage::meetings::{roster_of, MeetingCmd};
use crate::storage::{StorageCmd, StorageHandle};
use crate::theme;
use crate::ui::{format, widgets};
use egui::{RichText, TextEdit, Ui};
use hark_meeting::Roster;
use hark_store::MeetingDetail;
use hark_voice::MeetingNotes;
use jiff::tz::TimeZone;
use std::sync::mpsc::{self, Receiver, TryRecvError};

pub(super) struct DetailView {
    /// (generation, id) the cache reflects.
    key: Option<(u64, String)>,
    detail: Option<MeetingDetail>,
    /// Built with `detail`, on the same cache key: labels and colors.
    roster: Option<Roster>,
    /// As stored: speaker tokens, never names. Ticks are written here.
    notes: Option<MeetingNotes>,
    /// The roster's `(key, default label, display label)` table the notes
    /// render through; built with `roster`.
    note_names: Vec<(String, String, String)>,
    error: Option<String>,
    title: String,
    speakers: SpeakersCard,
    transcript: TranscriptView,
    assignments: Assignments,
    confirm: Option<widgets::Confirm>,
    confirm_rerun: Option<widgets::Confirm>,
    sharing: Sharing,
    deleting: Option<(String, Receiver<Result<(), String>>)>,
    delete_error: Option<(String, String)>,
    deleted: Option<String>,
    /// Meetings whose notes a move just made stale: rewrite them next frame.
    refresh_due: Vec<String>,
}

impl DetailView {
    pub fn new() -> Self {
        DetailView {
            key: None,
            detail: None,
            roster: None,
            notes: None,
            note_names: Vec::new(),
            error: None,
            title: String::new(),
            speakers: SpeakersCard::new(),
            transcript: TranscriptView::new(),
            assignments: Assignments::new(),
            confirm: None,
            confirm_rerun: None,
            sharing: Sharing::new(),
            deleting: None,
            delete_error: None,
            deleted: None,
            refresh_due: Vec::new(),
        }
    }

    pub fn poll(&mut self) {
        self.sharing.poll();
        let stale = self.assignments.poll();
        self.refresh_due.extend(stale);
        if let Some((id, reply)) = &self.deleting {
            let result = match reply.try_recv() {
                Ok(result) => result,
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => Err("Deletion could not be confirmed because the storage worker is unavailable. Reopen Hark and check the meeting before trying again.".to_string()),
            };
            match result {
                Ok(()) => self.deleted = Some(id.clone()),
                Err(error) => self.delete_error = Some((id.clone(), error)),
            }
            self.deleting = None;
        }
    }

    /// Returns true when the user went back to the list (or deleted it).
    pub fn show(
        &mut self,
        ui: &mut Ui,
        storage: &StorageHandle,
        id: &str,
        tz: &TimeZone,
        meetings: &mut crate::meeting::MeetingController,
    ) -> bool {
        if self.deleted.as_deref() == Some(id) {
            self.deleted = None;
            return true;
        }
        // Read what the storage worker just committed: the request names
        // every line by the speaker it is shown as now.
        for due in std::mem::take(&mut self.refresh_due) {
            if let Ok(Some(fresh)) = storage.reader().meeting(&due) {
                meetings.refresh_notes(notes::request(&fresh));
            }
        }
        self.refresh(storage, id);
        let back = ui.button("‹ All meetings").clicked();
        ui.add_space(theme::GAP);
        if let Some((_, error)) = self
            .delete_error
            .as_ref()
            .filter(|(failed_id, _)| failed_id == id)
        {
            ui.label(RichText::new(error).color(theme::warning(ui.visuals())));
        }
        if self
            .deleting
            .as_ref()
            .is_some_and(|(pending_id, _)| pending_id == id)
        {
            ui.label("Deleting meeting…");
        }
        if let Some(error) = &self.error {
            widgets::empty_state(
                ui,
                theme::icons::WARNING,
                "This meeting cannot be read.",
                error,
            );
            return back;
        }
        // Taken for the frame and put back at the end, so the helpers below can
        // borrow `self` mutably while reading it.
        let Some(taken) = self.detail.take() else {
            widgets::empty_state(
                ui,
                theme::icons::MAGNIFYING_GLASS,
                "Meeting not found.",
                "It may have been deleted.",
            );
            return back;
        };
        let detail = &taken;

        // Title, editable in place; saved on Enter or focus loss.
        let response = ui.add(
            TextEdit::singleline(&mut self.title)
                .font(egui::TextStyle::Heading)
                .desired_width(f32::INFINITY),
        );
        if response.lost_focus()
            && self.title.trim() != detail.summary.title.as_deref().unwrap_or("").trim()
        {
            storage.send(StorageCmd::Meeting(MeetingCmd::Rename {
                id: id.to_string(),
                title: self.title.clone(),
            }));
        }
        ui.label(RichText::new(meta(detail, tz)).small().weak());
        if detail.summary.audio_evicted_ms.is_some() {
            ui.label(
                RichText::new(
                    "Audio removed to stay under your storage cap. The transcript and notes stay.",
                )
                .small()
                .weak(),
            );
        }
        ui.add_space(theme::GAP);

        let mut action = None;
        let running = meetings.finishing().iter().any(|i| i == id);
        let mut busy = running || detail.summary.ended_ms.is_none() || self.deleting.is_some();
        let pass = FinalPassAction::for_meeting(detail);
        ui.horizontal(|ui| {
            action = share::menu(ui, !busy && detail.summary.audio_evicted_ms.is_none());
            if ui
                .add_enabled(
                    !busy
                        && meetings.is_available()
                        && detail.summary.audio_evicted_ms.is_none()
                        && detail.summary.audio_bytes > 0,
                    egui::Button::new(pass.label),
                )
                .clicked()
            {
                self.confirm_rerun = Some(widgets::Confirm::new(pass.title, pass.body, pass.label));
            }
            if ui
                .add_enabled(
                    !busy,
                    egui::Button::new(theme::icon_label_job(
                        ui.style(),
                        theme::icons::TRASH,
                        "Delete",
                    )),
                )
                .clicked()
            {
                self.confirm = Some(widgets::Confirm::new(
                    "Delete this meeting?",
                    "Its notes, transcript and audio are removed from this device.",
                    "Delete meeting",
                ));
            }
        });
        // In flight: this session's progress line. Otherwise what the
        // database says, which survives a restart; this session's one-shot
        // result (a success, or a save that failed) only when it says nothing.
        let persisted = status_line(
            detail.summary.transcript_status.as_deref(),
            jiff::Timestamp::now().as_millisecond(),
            tz,
        );
        match (running, meetings.rerun_status(id), persisted) {
            (true, Some(progress), _) => {
                ui.label(RichText::new(progress).small());
            }
            (_, _, Some((icon, text))) => {
                ui.label(theme::icon_label_job(ui.style(), icon, &text));
            }
            (_, Some(result), None) => {
                ui.label(RichText::new(result).small());
            }
            _ => {}
        }
        if let Some(confirm) = &mut self.confirm_rerun {
            match confirm.show(ui, "meeting-rerun") {
                Some(true) => {
                    let duration = detail
                        .summary
                        .ended_ms
                        .unwrap_or(detail.summary.started_ms)
                        .saturating_sub(detail.summary.started_ms)
                        .max(0) as u64;
                    meetings.rerun(id, duration);
                    busy = true;
                    self.confirm_rerun = None;
                }
                Some(false) => self.confirm_rerun = None,
                None => {}
            }
        }
        if let Some(status) = self.sharing.status() {
            ui.label(RichText::new(status).small().weak());
        }
        if let Some(action) = action {
            self.run_share(ui.ctx(), action, detail, id, tz);
        }
        self.sharing.show(ui.ctx());
        ui.add_space(theme::SECTION_GAP);

        if let (Some(stored), Some(roster)) = (&self.notes, &self.roster) {
            let stale = notes::stale_line(detail, meetings.notes_status(id));
            let names = notes::Names {
                roster,
                table: &self.note_names,
            };
            let action = notes::card(ui, stored, &names, stale);
            if let Some((index, done)) = action.toggled {
                self.tick(storage, id, index, done);
            }
            if action.update {
                meetings.refresh_notes(notes::request(detail));
            }
            ui.add_space(theme::SECTION_GAP);
        }
        if busy {
            self.speakers.reset();
            self.transcript.reset();
        }
        if let Some(roster) = &self.roster {
            let (rename, reassign) = ui
                .add_enabled_ui(!busy, |ui| {
                    let rename = self.speakers.show(ui, roster, &detail.speakers);
                    if let Some(error) = self.assignments.error(id) {
                        ui.label(RichText::new(error).color(theme::warning(ui.visuals())));
                    }
                    (rename, self.transcript.show(ui, detail, roster))
                })
                .inner;
            if let Some((key, name)) = rename {
                storage.send(StorageCmd::Meeting(MeetingCmd::SetSpeakerName {
                    id: id.to_string(),
                    key: key.to_string(),
                    name,
                }));
            }
            if let Some(choice) = reassign {
                self.assignments.send(storage, id, choice);
            }
        }

        if let Some(confirm) = &mut self.confirm {
            match confirm.show(ui, "meeting-delete") {
                Some(true) => {
                    let (reply, receiver) = mpsc::channel();
                    self.deleting = Some((id.to_string(), receiver));
                    self.delete_error = None;
                    self.deleted = None;
                    storage.send(StorageCmd::Meeting(MeetingCmd::Delete {
                        id: id.to_string(),
                        reply,
                    }));
                    self.confirm = None;
                }
                Some(false) => self.confirm = None,
                None => {}
            }
        }
        self.detail = Some(taken);
        back
    }

    fn refresh(&mut self, storage: &StorageHandle, id: &str) {
        let key = (storage.generation(), id.to_string());
        if self.key.as_ref() == Some(&key) {
            return;
        }
        let fresh_meeting = self.key.as_ref().is_none_or(|(_, old)| old != id);
        match storage.reader().meeting(id) {
            Ok(detail) => {
                self.notes = detail
                    .as_ref()
                    .and_then(|d| d.notes_json.as_deref())
                    .and_then(|json| MeetingNotes::from_json(json).ok());
                if fresh_meeting {
                    self.confirm = None;
                    self.confirm_rerun = None;
                    self.title = detail
                        .as_ref()
                        .and_then(|d| d.summary.title.clone())
                        .unwrap_or_default();
                    self.speakers.reset();
                    self.transcript.reset();
                    self.assignments.clear_error();
                }
                self.roster = detail.as_ref().map(roster_of);
                self.note_names = self
                    .roster
                    .as_ref()
                    .map(Roster::notes_names)
                    .unwrap_or_default();
                self.detail = detail;
                self.error = None;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        self.key = Some(key);
    }

    /// A checkbox writes the stored notes, tokens and all, never the rendered
    /// copy: storing that would freeze today's names into the notes.
    fn tick(&mut self, storage: &StorageHandle, id: &str, index: usize, done: bool) {
        let Some(stored) = &mut self.notes else {
            return;
        };
        if let Some(notes_json) = notes::ticked(stored, index, done) {
            storage.send(StorageCmd::Meeting(MeetingCmd::UpdateNotes {
                id: id.to_string(),
                notes_json,
            }));
        }
    }

    fn run_share(
        &mut self,
        ctx: &egui::Context,
        action: ShareAction,
        detail: &MeetingDetail,
        id: &str,
        tz: &TimeZone,
    ) {
        let export = share::export_of(detail, self.notes.as_ref(), &self.title, tz);
        self.sharing.run(ctx, action, id, export);
    }
}

/// The final-pass button: with no transcript to replace, the job is to make
/// one, and the button says so.
struct FinalPassAction {
    label: &'static str,
    title: &'static str,
    body: &'static str,
}

impl FinalPassAction {
    fn for_meeting(detail: &MeetingDetail) -> Self {
        if detail.segments.is_empty() {
            FinalPassAction {
                label: "Transcribe recording",
                title: "Transcribe this recording with Deepgram?",
                body: "Uploads this recording to Deepgram using your key and saves the transcript it returns. Notes and audio stay. Provider usage charges apply.",
            }
        } else {
            FinalPassAction {
                label: "Re-run final pass",
                title: "Re-run the final pass with Deepgram?",
                body: "Uploads this recording to Deepgram using your key. On success, it replaces the transcript, guesses speaker names again and rewrites the notes. Audio stays. Provider usage charges apply.",
            }
        }
    }
}

/// The persisted final-pass status (`meetings.transcript_status`) as an
/// icon and a sentence. `None` when nothing is outstanding or the text is
/// from a newer Hark.
fn status_line(stored: Option<&str>, now_ms: i64, tz: &TimeZone) -> Option<(&'static str, String)> {
    use hark_pipeline::meeting::{Failure, FailureKind, TranscriptStatus};
    Some(match TranscriptStatus::parse(stored?)? {
        TranscriptStatus::Pending {
            retry_at_ms: Some(at),
        } if at > now_ms => (
            theme::icons::CLOCK,
            format!(
                "Waiting for a connection, retrying at {}.",
                format::clock_time(at, now_ms, tz)
            ),
        ),
        TranscriptStatus::Pending { .. } => (
            theme::icons::CLOCK,
            "Waiting for a connection, retrying shortly.".to_string(),
        ),
        TranscriptStatus::Failed(Failure::NoKey) => {
            (theme::icons::KEY, Failure::NoKey.message().to_string())
        }
        TranscriptStatus::Failed(Failure::Stt(FailureKind::Network)) => (
            theme::icons::WARNING,
            "Deepgram could not be reached after several tries. Choose Transcribe recording to try again."
                .to_string(),
        ),
        TranscriptStatus::Failed(failure) => {
            (theme::icons::WARNING, failure.message().to_string())
        }
    })
}

fn meta(detail: &MeetingDetail, tz: &TimeZone) -> String {
    let s = &detail.summary;
    let mut parts = vec![format::full_timestamp(s.started_ms, tz)];
    if let Some(end) = s.ended_ms {
        parts.push(format::duration(end - s.started_ms));
    }
    if let Some(app) = &s.app_hint {
        parts.push(hark_pipeline::meeting::app_display_name(app));
    }
    parts.push(if s.refined {
        "refined transcript".to_string()
    } else {
        format!("live transcript ({})", detail.stt_provider)
    });
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deletion_reply_preserves_failure_and_only_marks_confirmed_success() {
        let mut view = DetailView::new();
        let (reply, receiver) = mpsc::channel();
        view.deleting = Some(("m1".to_string(), receiver));
        view.poll();
        assert!(view.deleting.is_some());
        assert!(view.deleted.is_none());
        reply
            .send(Err("The recording is in use. Try Delete again.".to_string()))
            .unwrap();
        view.poll();
        assert!(view.deleting.is_none());
        assert!(view.deleted.is_none());
        assert_eq!(view.delete_error.as_ref().unwrap().0, "m1");
        assert!(view
            .delete_error
            .as_ref()
            .unwrap()
            .1
            .contains("Try Delete again"));
        let (reply, receiver) = mpsc::channel();
        view.deleting = Some(("m1".to_string(), receiver));
        reply.send(Ok(())).unwrap();
        view.poll();
        assert_eq!(view.deleted.as_deref(), Some("m1"));
    }

    #[test]
    fn the_persisted_status_reads_as_a_sentence() {
        let tz = TimeZone::fixed(jiff::tz::offset(-5));
        let now = jiff::civil::date(2026, 10, 14)
            .at(16, 30, 0, 0)
            .to_zoned(tz.clone())
            .unwrap()
            .timestamp()
            .as_millisecond();
        let line = |s: &str| status_line(Some(s), now, &tz).map(|(_, text)| text);
        assert_eq!(
            line(&format!("pending:{}", now + 5 * 60_000)).as_deref(),
            Some("Waiting for a connection, retrying at 4:35 PM.")
        );
        assert_eq!(
            line(&format!("pending:{}", now - 1)).as_deref(),
            Some("Waiting for a connection, retrying shortly.")
        );
        assert_eq!(
            line("failed:silent").as_deref(),
            Some("This recording is silent; Hark captured no audio from either side.")
        );
        assert!(line("failed:no-key")
            .unwrap()
            .starts_with("No Deepgram key."));
        assert!(line("failed:network")
            .unwrap()
            .contains("Transcribe recording"));
        assert!(line("failed:auth").unwrap().contains("refused the key"));
        assert_eq!(line("failed:from-the-future"), None);
        assert_eq!(status_line(None, now, &tz), None);
    }

    #[test]
    fn disconnected_deletion_reply_never_claims_success() {
        let mut view = DetailView::new();
        let (reply, receiver) = mpsc::channel();
        view.deleting = Some(("m1".to_string(), receiver));
        drop(reply);
        view.poll();
        assert!(view.deleted.is_none());
        assert!(view
            .delete_error
            .as_ref()
            .unwrap()
            .1
            .contains("could not be confirmed"));
    }
}

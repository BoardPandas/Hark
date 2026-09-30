//! One meeting: title, notes (with tickable action items), speakers, the
//! transcript, sharing, and delete. Cached on (write generation, id).

use super::share::{self, ShareAction, Sharing};
use crate::storage::meetings::MeetingCmd;
use crate::storage::{StorageCmd, StorageHandle};
use crate::theme;
use crate::ui::{format, widgets};
use egui::{RichText, TextEdit, Ui};
use hark_meeting::export::{format_timestamp, speaker_label};
use hark_meeting::Channel;
use hark_store::MeetingDetail;
use hark_voice::MeetingNotes;
use jiff::tz::TimeZone;
use std::sync::mpsc::{self, Receiver, TryRecvError};

pub(super) struct DetailView {
    /// (generation, id) the cache reflects.
    key: Option<(u64, String)>,
    detail: Option<MeetingDetail>,
    notes: Option<MeetingNotes>,
    error: Option<String>,
    title: String,
    /// Speaker being renamed and its edit buffer.
    renaming: Option<(u32, String)>,
    confirm: Option<widgets::Confirm>,
    confirm_rerun: Option<widgets::Confirm>,
    sharing: Sharing,
    deleting: Option<(String, Receiver<Result<(), String>>)>,
    delete_error: Option<(String, String)>,
    deleted: Option<String>,
}

impl DetailView {
    pub fn new() -> Self {
        DetailView {
            key: None,
            detail: None,
            notes: None,
            error: None,
            title: String::new(),
            renaming: None,
            confirm: None,
            confirm_rerun: None,
            sharing: Sharing::new(),
            deleting: None,
            delete_error: None,
            deleted: None,
        }
    }

    pub fn poll(&mut self) {
        self.sharing.poll();
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
        let mut busy = meetings.finishing().iter().any(|i| i == id)
            || detail.summary.ended_ms.is_none()
            || self.deleting.is_some();
        ui.horizontal(|ui| {
            action = share::menu(ui, !busy && detail.summary.audio_evicted_ms.is_none());
            if ui.add_enabled(!busy && meetings.is_available() && detail.summary.audio_evicted_ms.is_none() && detail.summary.audio_bytes > 0,
                egui::Button::new("Re-run final pass")).clicked() {
                self.confirm_rerun = Some(widgets::Confirm::new(
                    "Re-run the final pass with Deepgram?",
                    "Uploads this recording to Deepgram using your key. On success, it replaces the transcript and resets speaker names. Notes and audio stay. Provider usage charges apply.",
                    "Re-run final pass",
                ));
            }
            if ui
                .add_enabled(!busy, egui::Button::new(theme::icon_label_job(
                    ui.style(),
                    theme::icons::TRASH,
                    "Delete",
                )))
                .clicked()
            {
                self.confirm = Some(widgets::Confirm::new(
                    "Delete this meeting?",
                    "Its notes, transcript and audio are removed from this device.",
                    "Delete meeting",
                ));
            }
        });
        if let Some(status) = meetings.rerun_status(id) {
            ui.label(RichText::new(status).small());
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
                    self.renaming = None;
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

        if let Some(notes) = self.notes.clone() {
            self.notes_card(ui, storage, id, notes);
            ui.add_space(theme::SECTION_GAP);
        }
        if busy {
            self.renaming = None;
        }
        ui.add_enabled_ui(!busy, |ui| self.speakers_card(ui, storage, id, detail));
        transcript(ui, detail);

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
                    self.renaming = None;
                }
                self.detail = detail;
                self.error = None;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        self.key = Some(key);
    }

    fn notes_card(
        &mut self,
        ui: &mut Ui,
        storage: &StorageHandle,
        id: &str,
        mut notes: MeetingNotes,
    ) {
        let mut changed = false;
        theme::card(ui, |ui| {
            ui.label(RichText::new("Summary").text_style(theme::subheading()));
            ui.label(&notes.summary);
            list(ui, "Key points", &notes.key_points);
            list(ui, "Decisions", &notes.decisions);
            if !notes.action_items.is_empty() {
                ui.add_space(theme::GAP);
                ui.label(RichText::new("Action items").strong());
                for item in &mut notes.action_items {
                    let label = match &item.owner {
                        Some(owner) => format!("{} ({owner})", item.text),
                        None => item.text.clone(),
                    };
                    changed |= ui.checkbox(&mut item.done, label).changed();
                }
            }
        });
        if changed {
            storage.send(StorageCmd::Meeting(MeetingCmd::UpdateNotes {
                id: id.to_string(),
                notes_json: notes.to_json(),
            }));
            self.notes = Some(notes);
        }
    }

    /// Rename "Speaker 2" to "Dana". Only speakers that appear are listed.
    fn speakers_card(
        &mut self,
        ui: &mut Ui,
        storage: &StorageHandle,
        id: &str,
        detail: &MeetingDetail,
    ) {
        let mut speakers: Vec<u32> = detail
            .segments
            .iter()
            .filter(|s| s.channel == 1)
            .filter_map(|s| s.speaker)
            .collect();
        speakers.sort_unstable();
        speakers.dedup();
        if speakers.is_empty() {
            return;
        }
        let renames = detail.speakers.clone();
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Speakers").strong());
            for speaker in speakers {
                let label = speaker_label(Channel::Them, Some(speaker), &renames);
                match &mut self.renaming {
                    Some((s, buf)) if *s == speaker => {
                        let r = ui.add(TextEdit::singleline(buf).desired_width(120.0));
                        if r.lost_focus() {
                            storage.send(StorageCmd::Meeting(MeetingCmd::RenameSpeaker {
                                id: id.to_string(),
                                speaker,
                                name: buf.clone(),
                            }));
                            self.renaming = None;
                        } else {
                            r.request_focus();
                        }
                    }
                    _ => {
                        if ui.button(&label).on_hover_text("Rename").clicked() {
                            self.renaming = Some((speaker, label));
                        }
                    }
                }
            }
        });
        ui.add_space(theme::GAP);
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

fn list(ui: &mut Ui, heading: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    ui.add_space(theme::GAP);
    ui.label(RichText::new(heading).strong());
    for item in items {
        ui.label(format!("• {item}"));
    }
}

fn transcript(ui: &mut Ui, detail: &MeetingDetail) {
    ui.label(RichText::new("Transcript").text_style(theme::subheading()));
    if detail.segments.is_empty() {
        ui.label(RichText::new("No transcript.").weak());
        return;
    }
    for s in &detail.segments {
        let channel = if s.channel == 0 {
            Channel::Me
        } else {
            Channel::Them
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format_timestamp(s.start_ms.max(0) as u64))
                    .small()
                    .weak(),
            );
            ui.label(RichText::new(speaker_label(channel, s.speaker, &detail.speakers)).strong());
            ui.add(egui::Label::new(&s.text).selectable(true));
        });
    }
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

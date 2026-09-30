//! Sharing a meeting (plan §4.10): copy notes and transcript as Markdown or
//! plain text, save them to a file, save the audio as a small MP3 or a WAV
//! anyone can play, or show the audio in its folder.
//!
//! Two rules from the plan's gotchas: a native save dialog opened on the egui
//! thread freezes the event loop (the recording pill included), so dialogs and
//! file writes run on a worker thread that wakes the UI when done; and
//! "Copy" goes straight to the clipboard, never through the injection
//! stash/restore path, which would put the old clipboard back.

use hark_meeting::export::{
    self, speaker_label, ExportAction, ExportLine, ExportMeeting, ExportNotes, ExportOptions,
};
use hark_meeting::Channel;
use hark_store::MeetingDetail;
use hark_voice::MeetingNotes;
use jiff::tz::TimeZone;
use std::sync::mpsc::{self, Receiver};

mod excerpt;
mod files;
#[cfg(windows)]
mod native;
#[cfg(any(windows, target_os = "macos"))]
mod word;
use files::{meeting_dir, save_audio, save_text, show_in_folder};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ShareAction {
    CopyMarkdown,
    CopyText,
    SaveMarkdown,
    SaveText,
    SaveSrt,
    SaveVtt,
    #[cfg(any(windows, target_os = "macos"))]
    SaveDocx,
    #[cfg(target_os = "macos")]
    MacShare,
    #[cfg(windows)]
    WindowsShare,
    SaveExcerpt,
    SaveMp3,
    SaveWav,
    ShowFolder,
}

/// The Share menu. Audio items only while the meeting still has audio.
pub(super) fn menu(ui: &mut egui::Ui, has_audio: bool) -> Option<ShareAction> {
    let mut picked = None;
    ui.menu_button("Share", |ui| {
        let mut item = |ui: &mut egui::Ui, label: &str, action: ShareAction| {
            if ui.button(label).clicked() {
                picked = Some(action);
                ui.close();
            }
        };
        item(ui, "Copy as Markdown", ShareAction::CopyMarkdown);
        item(ui, "Copy as text", ShareAction::CopyText);
        ui.separator();
        item(ui, "Save as Markdown…", ShareAction::SaveMarkdown);
        item(ui, "Save as text…", ShareAction::SaveText);
        item(ui, "Save subtitles as SRT…", ShareAction::SaveSrt);
        item(ui, "Save subtitles as VTT…", ShareAction::SaveVtt);
        #[cfg(any(windows, target_os = "macos"))]
        item(ui, "Save as Word document…", ShareAction::SaveDocx);
        #[cfg(windows)]
        item(ui, "Share with Windows…", ShareAction::WindowsShare);
        #[cfg(target_os = "macos")]
        item(ui, "Share with macOS…", ShareAction::MacShare);
        if has_audio {
            ui.separator();
            item(ui, "Save an excerpt with audio…", ShareAction::SaveExcerpt);
            item(ui, "Save audio as MP3…", ShareAction::SaveMp3);
            item(ui, "Save audio as WAV…", ShareAction::SaveWav);
            item(ui, "Show audio in folder", ShareAction::ShowFolder);
        }
    });
    picked
}

/// The export model for one meeting, in the user's time zone.
pub(super) fn export_of(
    detail: &MeetingDetail,
    notes: Option<&MeetingNotes>,
    title: &str,
    tz: &TimeZone,
) -> ExportMeeting {
    let s = &detail.summary;
    let title = if title.trim().is_empty() {
        "Meeting".to_string()
    } else {
        title.trim().to_string()
    };
    ExportMeeting {
        title,
        started: crate::ui::format::full_timestamp(s.started_ms, tz),
        duration_ms: s.ended_ms.map_or_else(
            || {
                detail
                    .segments
                    .iter()
                    .map(|seg| seg.end_ms.max(0) as u64)
                    .max()
                    .unwrap_or(0)
            },
            |e| e.saturating_sub(s.started_ms).max(0) as u64,
        ),
        lines: detail
            .segments
            .iter()
            .map(|seg| ExportLine {
                at_ms: seg.start_ms.max(0) as u64,
                end_ms: seg.end_ms.max(seg.start_ms).max(0) as u64,
                speaker: speaker_label(
                    if seg.channel == 0 {
                        Channel::Me
                    } else {
                        Channel::Them
                    },
                    seg.speaker,
                    &detail.speakers,
                ),
                text: seg.text.clone(),
            })
            .collect(),
        notes: notes.map(|n| ExportNotes {
            summary: n.summary.clone(),
            key_points: n.key_points.clone(),
            decisions: n.decisions.clone(),
            action_items: n
                .action_items
                .iter()
                .map(|a| ExportAction {
                    text: a.text.clone(),
                    owner: a.owner.clone(),
                    done: a.done,
                })
                .collect(),
        }),
    }
}

/// The outcome of the last share action, and any still running.
pub(super) struct Sharing {
    rx: Option<Receiver<String>>,
    status: Option<String>,
    excerpt: Option<excerpt::ExcerptDialog>,
    #[cfg(windows)]
    native: Option<native::WindowsShare>,
}

impl Sharing {
    pub fn new() -> Self {
        Sharing {
            rx: None,
            status: None,
            excerpt: None,
            #[cfg(windows)]
            native: None,
        }
    }

    pub fn poll(&mut self) {
        if let Some(rx) = &self.rx {
            match rx.try_recv() {
                Ok(status) => {
                    self.status = Some(status);
                    self.rx = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.status = Some("The save worker stopped before completing.".into());
                    self.rx = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
    }

    pub fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        if self.rx.is_some() {
            return;
        }
        let Some(mut dialog) = self.excerpt.take() else {
            return;
        };
        match dialog.show(ctx) {
            excerpt::DialogAction::Keep => self.excerpt = Some(dialog),
            excerpt::DialogAction::Cancel => {}
            excerpt::DialogAction::Save { mp3 } => self.spawn(ctx, move || dialog.save(mp3)),
        }
    }

    pub fn run(
        &mut self,
        ctx: &egui::Context,
        action: ShareAction,
        id: &str,
        export: ExportMeeting,
    ) {
        if self.rx.is_some() {
            self.status = Some("Finish the current save before starting another.".to_string());
            return;
        }
        let opts = ExportOptions::default();
        match action {
            ShareAction::CopyMarkdown => {
                ctx.copy_text(export::to_markdown(&export, opts));
                self.status = Some("Copied as Markdown.".to_string());
            }
            ShareAction::CopyText => {
                ctx.copy_text(export::to_text(&export, opts));
                self.status = Some("Copied as text.".to_string());
            }
            ShareAction::SaveMarkdown => {
                let name = export::safe_file_name(&export.title, "md");
                let body = export::to_markdown(&export, opts);
                self.spawn(ctx, move || save_text(name, body, "Markdown", "md"));
            }
            ShareAction::SaveText => {
                let name = export::safe_file_name(&export.title, "txt");
                let body = export::to_text(&export, opts);
                self.spawn(ctx, move || save_text(name, body, "Text", "txt"));
            }
            ShareAction::SaveSrt | ShareAction::SaveVtt => {
                let vtt = action == ShareAction::SaveVtt;
                let (filter, ext) = if vtt {
                    ("WebVTT subtitles", "vtt")
                } else {
                    ("SubRip subtitles", "srt")
                };
                let name = export::safe_file_name(&export.title, ext);
                let body = if vtt {
                    export::to_vtt(&export)
                } else {
                    export::to_srt(&export)
                };
                self.spawn(ctx, move || save_text(name, body, filter, ext));
            }
            #[cfg(any(windows, target_os = "macos"))]
            ShareAction::SaveDocx => self.spawn(ctx, move || word::save(export)),
            #[cfg(windows)]
            ShareAction::WindowsShare => {
                self.native = None;
                match native::WindowsShare::show(&export.title, &export::to_text(&export, opts)) {
                    Ok(native) => {
                        self.native = Some(native);
                        self.status = Some("Choose an app in Windows Share.".into());
                    }
                    Err(error) => {
                        self.status = Some(format!("Could not open Windows Share: {error}"))
                    }
                }
            }
            #[cfg(target_os = "macos")]
            ShareAction::MacShare => {
                self.status = Some(
                    match crate::macos::share_text(&export::to_text(&export, opts)) {
                        Ok(()) => "Choose an app in the share sheet.".into(),
                        Err(error) => error.into(),
                    },
                );
            }
            ShareAction::SaveExcerpt => {
                self.excerpt = Some(excerpt::ExcerptDialog::new(id, export))
            }
            ShareAction::SaveMp3 | ShareAction::SaveWav => {
                let dir = meeting_dir(id);
                let mp3 = action == ShareAction::SaveMp3;
                let name = export::safe_file_name(&export.title, if mp3 { "mp3" } else { "wav" });
                self.spawn(ctx, move || save_audio(dir, name, mp3));
            }
            ShareAction::ShowFolder => {
                self.status = Some(show_in_folder(meeting_dir(id)));
            }
        }
    }

    fn spawn(&mut self, ctx: &egui::Context, job: impl FnOnce() -> String + Send + 'static) {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        self.status = Some("Working…".to_string());
        self.rx = Some(rx);
        let spawned = std::thread::Builder::new()
            .name("hark-meeting-share".to_string())
            .spawn(move || {
                let _ = tx.send(job());
                crate::app::wake_ui(&ctx);
            });
        if let Err(e) = spawned {
            self.rx = None;
            self.status = Some(format!("Could not start saving: {e}"));
        }
    }
}

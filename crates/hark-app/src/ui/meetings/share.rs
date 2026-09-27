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
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ShareAction {
    CopyMarkdown,
    CopyText,
    SaveMarkdown,
    SaveText,
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
        if has_audio {
            ui.separator();
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
        duration_ms: s.ended_ms.map_or(0, |e| (e - s.started_ms).max(0) as u64),
        lines: detail
            .segments
            .iter()
            .map(|seg| ExportLine {
                at_ms: seg.start_ms.max(0) as u64,
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
}

impl Sharing {
    pub fn new() -> Self {
        Sharing {
            rx: None,
            status: None,
        }
    }

    pub fn poll(&mut self) {
        if let Some(rx) = &self.rx {
            if let Ok(status) = rx.try_recv() {
                self.status = Some(status);
                self.rx = None;
            }
        }
    }

    pub fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    pub fn run(
        &mut self,
        ctx: &egui::Context,
        action: ShareAction,
        id: &str,
        export: ExportMeeting,
    ) {
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

fn meeting_dir(id: &str) -> Option<PathBuf> {
    hark_config::default_data_dir().map(|d| d.join("meetings").join(id))
}

/// Ask where to save. `None` when cancelled (or no dialog on this platform).
/// Runs on a worker thread, owned by Hark's window (plan §4.10) so it stays in
/// front of it rather than surfacing behind.
fn ask_path(file_name: &str, filter: &str, ext: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let dialog = rfd::FileDialog::new()
            .set_file_name(file_name)
            .add_filter(filter, &[ext]);
        let dialog = match parent::MainWindow::find() {
            Some(window) => dialog.set_parent(&window),
            None => dialog,
        };
        dialog.save_file()
    }
    #[cfg(not(windows))]
    {
        let _ = (file_name, filter, ext);
        None
    }
}

fn save_text(name: String, body: String, filter: &str, ext: &str) -> String {
    let Some(path) = ask_path(&name, filter, ext) else {
        return "Save cancelled.".to_string();
    };
    match std::fs::write(&path, body) {
        Ok(()) => format!("Saved to {}.", path.display()),
        Err(e) => format!("Could not save: {e}"),
    }
}

fn save_audio(dir: Option<PathBuf>, name: String, mp3: bool) -> String {
    let Some(source) = dir.as_deref().and_then(hark_audio::meeting_audio) else {
        return "This meeting's audio is no longer on this device.".to_string();
    };
    let (filter, ext) = if mp3 {
        ("MP3 audio", "mp3")
    } else {
        ("WAV audio", "wav")
    };
    let Some(path) = ask_path(&name, filter, ext) else {
        return "Save cancelled.".to_string();
    };
    let result = if mp3 {
        hark_audio::export_mono_mp3(&source, &path)
    } else {
        hark_audio::export_mono_wav(&source, &path)
    };
    match result {
        Ok(()) => format!("Saved to {}.", path.display()),
        Err(e) => format!("Could not save the audio: {e}"),
    }
}

/// Open Explorer with the meeting's audio selected.
fn show_in_folder(dir: Option<PathBuf>) -> String {
    let Some(dir) = dir else {
        return "No data folder on this device.".to_string();
    };
    let target = [
        hark_audio::ARCHIVE_FILE,
        hark_audio::spool::ME_FILE,
        hark_audio::spool::THEM_FILE,
    ]
    .iter()
    .map(|f| dir.join(f))
    .find(|p| p.exists());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Explorer is a GUI program, but the rule for a windowless app is
        // absolute: every child process gets CREATE_NO_WINDOW (LL-G).
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut cmd = std::process::Command::new("explorer.exe");
        match &target {
            Some(file) => cmd.arg(format!("/select,{}", file.display())),
            None => cmd.arg(&dir),
        };
        match cmd.creation_flags(CREATE_NO_WINDOW).spawn() {
            Ok(_) => "Opened the meeting's folder.".to_string(),
            Err(e) => format!("Could not open the folder: {e}"),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = target;
        format!("The audio is in {}.", dir.display())
    }
}

/// Hark's main window as a dialog parent, found by title on the worker
/// thread: a window handle cannot cross threads, but its value can be looked
/// up again. The main window is the only top-level window titled exactly
/// "Hark" (the pill and the prompt have their own titles), and
/// single-instance guarantees one Hark process.
#[cfg(windows)]
mod parent {
    use raw_window_handle::{
        DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
        RawWindowHandle, Win32WindowHandle, WindowHandle, WindowsDisplayHandle,
    };
    use std::num::NonZeroIsize;

    pub struct MainWindow(NonZeroIsize);

    impl MainWindow {
        pub fn find() -> Option<MainWindow> {
            use windows::core::{w, PCWSTR};
            use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
            // SAFETY: a plain lookup by exact window title.
            let hwnd = unsafe { FindWindowW(PCWSTR::null(), w!("Hark")) }.ok()?;
            NonZeroIsize::new(hwnd.0 as isize).map(MainWindow)
        }
    }

    impl HasWindowHandle for MainWindow {
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            let raw = RawWindowHandle::Win32(Win32WindowHandle::new(self.0));
            // SAFETY: the handle is a live top-level window for as long as the
            // dialog runs (Hark's main window outlives any dialog it opens).
            Ok(unsafe { WindowHandle::borrow_raw(raw) })
        }
    }

    impl HasDisplayHandle for MainWindow {
        fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
            let raw = RawDisplayHandle::Windows(WindowsDisplayHandle::new());
            // SAFETY: Windows has no display connection to outlive.
            Ok(unsafe { DisplayHandle::borrow_raw(raw) })
        }
    }
}

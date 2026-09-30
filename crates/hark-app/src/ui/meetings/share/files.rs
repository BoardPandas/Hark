//! Native dialogs and filesystem actions, called from the share worker.

use std::path::{Path, PathBuf};

pub(super) fn meeting_dir(id: &str) -> Option<PathBuf> {
    hark_config::default_data_dir().map(|d| d.join("meetings").join(id))
}

/// Ask where to save. `None` when cancelled (or no dialog on this platform).
/// Runs on a worker thread, owned by Hark's window (plan §4.10) so it stays in
/// front of it rather than surfacing behind.
pub(super) fn ask_path(file_name: &str, filter: &str, ext: &str) -> Option<PathBuf> {
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
    #[cfg(target_os = "macos")]
    {
        let _ = filter;
        crate::macos::save_file(file_name, ext)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = (file_name, filter, ext);
        None
    }
}

pub(super) fn save_text(name: String, body: String, filter: &str, ext: &str) -> String {
    let Some(path) = ask_path(&name, filter, ext) else {
        return "Save cancelled.".to_string();
    };
    match write_file(&path, body.as_bytes()) {
        Ok(()) => format!("Saved to {}.", path.display()),
        Err(e) => format!("Could not save: {e}"),
    }
}

pub(super) fn save_audio(dir: Option<PathBuf>, name: String, mp3: bool) -> String {
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
    if let Err(error) = check_export_path(&path) {
        return format!("Could not save the audio: {error}");
    }
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

/// Sharing must never replace Hark's source audio or lifecycle markers.
/// Resolve both parents so a junction/symlink cannot bypass the boundary.
pub(super) fn check_export_path(path: &Path) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("Choose an export folder."))?
        .canonicalize()?;
    if let Some(root) = hark_config::default_data_dir().map(|dir| dir.join("meetings")) {
        if let Ok(root) = root.canonicalize() {
            if parent.starts_with(root) {
                return Err(std::io::Error::other(
                    "Choose a folder outside Hark's meeting storage.",
                ));
            }
        }
    }
    Ok(())
}

/// Encode fully before this boundary; failed writes leave an existing export
/// untouched. The unique temporary file is ours alone, never a user's .tmp.
pub(super) fn write_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    check_export_path(path)?;
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(
        ".hark-{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let tmp = path.with_file_name(name);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Open Explorer with the meeting's audio selected.
pub(super) fn show_in_folder(dir: Option<PathBuf>) -> String {
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
    #[cfg(target_os = "macos")]
    {
        let mut command = std::process::Command::new("/usr/bin/open");
        if let Some(file) = target {
            command.arg("-R").arg(file);
        } else {
            command.arg(&dir);
        }
        match command.status() {
            Ok(status) if status.success() => "Opened the meeting’s folder.".into(),
            Ok(status) => format!("Could not open Finder ({status})."),
            Err(error) => format!("Could not open Finder: {error}"),
        }
    }
    #[cfg(not(any(windows, target_os = "macos")))]
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

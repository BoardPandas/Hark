//! Windows probe for [`crate::detect`]: who holds the microphone right now,
//! which processes own a meeting-titled window, and the process list for
//! resolving a loopback target. Glue only; the decisions are in `detect`.
//! Verified by hand with `examples/detect_smoke.rs`, never in `cargo test`.
//!
//! Microphone use comes from the ConsentStore
//! (`HKCU\...\CapabilityAccessManager\ConsentStore\microphone`), read in
//! process with `winreg` (never `reg.exe`: a GUI-subsystem binary would flash
//! a console). Packaged apps are direct subkeys named by package family;
//! desktop apps sit under `NonPackaged\<exe path with # for \>`. An entry is in
//! use while `LastUsedTimeStart > 0` and `LastUsedTimeStop == 0`, and it flips
//! within a second of the app opening or closing the mic, even when the
//! process is killed (CP0). No permission is needed; a read costs ~0.6 ms.
//!
//! Window titles are read in memory to test for a meeting marker and never
//! stored, returned or logged.
//!
//! The `probe` facade dispatches to this on Windows, `linux.rs` on Linux,
//! and `mac.rs` on macOS; anything else returns `Unsupported`.

use crate::detect::{Proc, Snapshot};
use std::io;

#[path = "watch_win.rs"]
mod watch;
pub use watch::ChangeWatcher;

/// Take one detector snapshot. Window titles are only scanned when a browser
/// holds the mic, since only browsers consult them.
pub fn snapshot() -> io::Result<Snapshot> {
    #[cfg(windows)]
    {
        native::snapshot()
    }
    #[cfg(not(windows))]
    {
        Err(unsupported())
    }
}

/// Every running process, for [`crate::detect::root_pid`].
pub fn processes() -> io::Result<Vec<Proc>> {
    #[cfg(windows)]
    {
        native::processes()
    }
    #[cfg(not(windows))]
    {
        Err(unsupported())
    }
}

#[cfg(not(windows))]
fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "meeting detection is not implemented on this platform",
    )
}

#[cfg(windows)]
mod native {
    use crate::detect::{title_has_meeting_marker, MicApp, MicUse, Proc, Snapshot, BROWSERS};
    use std::io;
    use windows::core::{BOOL, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
    };
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    const CONSENT_MIC: &str = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone";

    /// Closes a Win32 handle on drop.
    struct Owned(HANDLE);

    impl Drop for Owned {
        fn drop(&mut self) {
            // SAFETY: we own the handle; a failure means it is already gone.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    pub(super) fn snapshot() -> io::Result<Snapshot> {
        let users = mic_users()?;
        let browser_in_use = users
            .iter()
            .any(|u| u.in_use && BROWSERS.contains(&u.app.id().as_str()));
        let meeting_windows = if browser_in_use {
            meeting_window_exes()
        } else {
            Vec::new()
        };
        Ok(Snapshot {
            users,
            meeting_windows,
        })
    }

    fn mic_users() -> io::Result<Vec<MicUse>> {
        let root = match RegKey::predef(HKEY_CURRENT_USER).open_subkey(CONSENT_MIC) {
            Ok(root) => root,
            // No app has ever asked for the mic on this account.
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let in_use = |key: &RegKey| {
            let start: u64 = key.get_value("LastUsedTimeStart").unwrap_or(0);
            let stop: u64 = key.get_value("LastUsedTimeStop").unwrap_or(0);
            start > 0 && stop == 0
        };
        let mut users = Vec::new();
        for name in root.enum_keys() {
            let name = name?;
            // A key can vanish between enumeration and open (app uninstalled):
            // skip it rather than fail the whole poll.
            let Ok(sub) = root.open_subkey(&name) else {
                continue;
            };
            if name == "NonPackaged" {
                for exe in sub.enum_keys() {
                    let exe = exe?;
                    if let Ok(key) = sub.open_subkey(&exe) {
                        users.push(MicUse {
                            app: MicApp::from_nonpackaged_key(&exe),
                            in_use: in_use(&key),
                        });
                    }
                }
            } else {
                users.push(MicUse {
                    in_use: in_use(&sub),
                    app: MicApp::Packaged(name),
                });
            }
        }
        Ok(users)
    }

    /// Lowercase exe names of processes owning a visible top-level window
    /// with a meeting marker in its title.
    fn meeting_window_exes() -> Vec<String> {
        let mut pids: Vec<u32> = Vec::new();
        // SAFETY: the callback only runs during EnumWindows, while `pids`
        // (passed through LPARAM) is borrowed mutably and alive.
        let enumerated = unsafe {
            EnumWindows(
                Some(collect_meeting_window),
                LPARAM(&mut pids as *mut Vec<u32> as isize),
            )
        };
        if let Err(e) = enumerated {
            log::debug!("meeting-window scan failed: {e}");
        }
        let mut exes: Vec<String> = pids.into_iter().filter_map(exe_name_of).collect();
        exes.sort();
        exes.dedup();
        exes
    }

    unsafe extern "system" fn collect_meeting_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: EnumWindows hands back the LPARAM we passed: a live
        // `&mut Vec<u32>` for the duration of the enumeration.
        let pids = unsafe { &mut *(lparam.0 as *mut Vec<u32>) };
        // SAFETY: plain queries on a window handle the system just gave us.
        unsafe {
            if IsWindowVisible(hwnd).as_bool() {
                let mut title = [0u16; 512];
                let len = GetWindowTextW(hwnd, &mut title).max(0) as usize;
                if title_has_meeting_marker(&String::from_utf16_lossy(&title[..len])) {
                    let mut pid = 0u32;
                    GetWindowThreadProcessId(hwnd, Some(&raw mut pid));
                    if pid != 0 && !pids.contains(&pid) {
                        pids.push(pid);
                    }
                }
            }
        }
        true.into()
    }

    fn exe_name_of(pid: u32) -> Option<String> {
        // SAFETY: query-only access to another process; the handle is closed
        // by `Owned`, and the buffer outlives the call.
        unsafe {
            let process = Owned(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?);
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            QueryFullProcessImageNameW(
                process.0,
                PROCESS_NAME_WIN32,
                PWSTR(buf.as_mut_ptr()),
                &mut len,
            )
            .ok()?;
            let path = String::from_utf16_lossy(&buf[..len as usize]);
            Some(MicApp::Desktop(path).id())
        }
    }

    pub(super) fn processes() -> io::Result<Vec<Proc>> {
        // SAFETY: a ToolHelp snapshot this function owns and closes; the
        // entry struct is ours and carries its own size as the API requires.
        unsafe {
            let snap = Owned(CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)?);
            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            let mut procs = Vec::new();
            let mut more = Process32FirstW(snap.0, &mut entry).is_ok();
            while more {
                let name = &entry.szExeFile;
                let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
                procs.push(Proc {
                    pid: entry.th32ProcessID,
                    parent: entry.th32ParentProcessID,
                    exe: String::from_utf16_lossy(&name[..len]),
                });
                more = Process32NextW(snap.0, &mut entry).is_ok();
            }
            Ok(procs)
        }
    }
}

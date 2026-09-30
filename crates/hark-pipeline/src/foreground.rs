//! Optional, one-shot foreground app identity. No window titles are queried.
//! A single bounded worker isolates OS lookups from audio, hotkeys and STT.

use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

struct Request {
    started: Instant,
    reply: mpsc::Sender<Option<String>>,
}

// One process-wide worker, including across settings/pipeline restarts. If a
// display server stops answering, it cannot accumulate threads or requests.
static PROBE: OnceLock<Option<SyncSender<Request>>> = OnceLock::new();
const MAX_SNAPSHOT_AGE: Duration = Duration::from_millis(150);

/// Called by the pipeline worker at engagement; off means no OS query and no
/// helper thread. The caller only ever polls the returned channel after text
/// injection and discards an unfinished lookup.
pub(crate) fn request(enabled: bool) -> Option<Receiver<Option<String>>> {
    if !enabled {
        return None;
    }
    let probe = PROBE.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel(1);
        match std::thread::Builder::new()
            .name("hark-app-insights".into())
            .spawn(move || run(rx, platform_app))
        {
            Ok(_) => Some(tx),
            Err(_) => {
                log::warn!("foreground app insights worker unavailable");
                None
            }
        }
    });
    let (reply, rx) = mpsc::channel();
    probe
        .as_ref()?
        .try_send(Request {
            started: Instant::now(),
            reply,
        })
        .ok()?;
    Some(rx)
}

fn run(rx: Receiver<Request>, mut lookup: impl FnMut() -> Option<String>) {
    while let Ok(request) = rx.recv() {
        // A queued request must never look at whichever app gained focus long
        // after the dictation started. Slow/late metadata is simply unknown.
        let label = if request.started.elapsed() <= MAX_SNAPSHOT_AGE {
            let label = lookup().and_then(valid_label);
            (request.started.elapsed() <= MAX_SNAPSHOT_AGE)
                .then_some(label)
                .flatten()
        } else {
            None
        };
        let _ = request.reply.send(label);
    }
}

fn valid_label(label: String) -> Option<String> {
    let label = label.trim();
    if label.is_empty()
        || label.chars().count() > 128
        || label
            .chars()
            .any(|c| c.is_control() || c == '/' || c == '\\')
    {
        return None;
    }
    Some(label.to_string())
}

#[cfg(target_os = "linux")]
fn local_x11_session(display: &str, session_type: Option<&str>, has_wayland_display: bool) -> bool {
    !has_wayland_display
        && !session_type.is_some_and(|value| value.eq_ignore_ascii_case("wayland"))
        && (display.starts_with(':') || display.starts_with("unix:"))
}

#[cfg(target_os = "linux")]
fn platform_app() -> Option<String> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};

    // XWayland exposes only X11 clients, which would mislabel native clients.
    // Do not infer focus from the last X11 window in a Wayland session.
    // A remote/SSH-forwarded X server's PID belongs to a different machine.
    // Never interpret it as an unrelated process in this machine's /proc.
    let display = std::env::var("DISPLAY").ok()?;
    if !local_x11_session(
        &display,
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
        std::env::var_os("WAYLAND_DISPLAY").is_some_and(|value| !value.is_empty()),
    ) {
        return None;
    }
    let (conn, screen) = x11rb::connect(None).ok()?;
    let root = conn.setup().roots.get(screen)?.root;
    let active = conn
        .intern_atom(true, b"_NET_ACTIVE_WINDOW")
        .ok()?
        .reply()
        .ok()?
        .atom;
    let window = conn
        .get_property(false, root, active, AtomEnum::WINDOW, 0, 1)
        .ok()?
        .reply()
        .ok()?
        .value32()?
        .next()?;
    if window == 0 {
        return None;
    }
    let pid_atom = conn
        .intern_atom(true, b"_NET_WM_PID")
        .ok()?
        .reply()
        .ok()?
        .atom;
    let pid = conn
        .get_property(false, window, pid_atom, AtomEnum::CARDINAL, 0, 1)
        .ok()?
        .reply()
        .ok()?
        .value32()?
        .next()?;
    let executable = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?;
    Some(executable.file_name()?.to_str()?.to_string())
}

#[cfg(windows)]
fn platform_app() -> Option<String> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    struct Process(HANDLE);
    impl Drop for Process {
        fn drop(&mut self) {
            // SAFETY: owns the successful OpenProcess handle exactly once.
            let _ = unsafe { CloseHandle(self.0) };
        }
    }
    // SAFETY: query-only APIs, with caller-owned output buffers. No message
    // is sent to the target app, and no window text is read.
    unsafe {
        let window = GetForegroundWindow();
        if window.is_invalid() {
            return None;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(window, Some(&mut pid));
        let process = Process(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?);
        let mut buffer = [0_u16; 32_768];
        let mut length = buffer.len() as u32;
        QueryFullProcessImageNameW(
            process.0,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        )
        .ok()?;
        let path = String::from_utf16(&buffer[..length as usize]).ok()?;
        let name = std::path::Path::new(&path).file_stem()?.to_str()?;
        Some(name.to_string())
    }
}

#[cfg(target_os = "macos")]
fn platform_app() -> Option<String> {
    // NSRunningApplication properties are thread-safe. This reads app
    // identity only; it neither creates UI nor requests Accessibility access.
    objc2::rc::autoreleasepool(|_| {
        let app = objc2_app_kit::NSWorkspace::sharedWorkspace().frontmostApplication()?;
        Some(app.localizedName()?.to_string())
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn platform_app() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_capture_does_not_request_metadata() {
        assert!(request(false).is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn wayland_and_remote_displays_never_masquerade_as_local_x11_apps() {
        assert!(local_x11_session(":0", Some("x11"), false));
        assert!(local_x11_session("unix:1", None, false));
        assert!(!local_x11_session(":0", Some("wayland"), false));
        assert!(!local_x11_session(":0", Some("x11"), true));
        assert!(!local_x11_session("localhost:10.0", Some("x11"), false));
        assert!(!local_x11_session("remote:0", None, false));
    }

    #[test]
    fn stale_request_never_queries_current_focus() {
        let (tx, rx) = mpsc::sync_channel(1);
        let (reply, result) = mpsc::channel();
        tx.send(Request {
            started: Instant::now() - MAX_SNAPSHOT_AGE - Duration::from_secs(1),
            reply,
        })
        .unwrap();
        drop(tx);
        run(rx, || {
            panic!("expired request must not inspect current focus")
        });
        assert_eq!(result.recv().unwrap(), None);
    }

    #[test]
    fn labels_cannot_persist_paths_control_characters_or_unbounded_strings() {
        assert_eq!(
            valid_label(" Visual Studio Code ".into()).as_deref(),
            Some("Visual Studio Code")
        );
        for label in [
            "",
            "\n",
            "/usr/bin/editor",
            "C:\\Users\\name\\editor.exe",
            "editor\nprivate",
            &"a".repeat(129),
        ] {
            assert_eq!(valid_label(label.into()), None);
        }
    }
}

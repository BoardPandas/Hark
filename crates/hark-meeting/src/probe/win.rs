//! Windows probe for [`crate::detect`]: who holds the microphone right now,
//! which processes own a meeting-titled window, and the process list for
//! resolving a loopback target. Glue only; the decisions are in `detect`.
//! Verified by hand with `examples/detect_smoke.rs`, never in `cargo test`.
//!
//! Microphone use comes from Core Audio: every audio session on every active
//! capture endpoint, kept when its state is `AudioSessionStateActive` (a
//! capture stream is running). A session names its process; a packaged
//! process is reported by package family (`MSTeams_8wekyb3d8bbwe`, which its
//! WebView2 children share), anything else by exe path. Endpoints, not only
//! the default mic, because a meeting app may capture from another device or
//! from a virtual one (Krisp) that sits in front of the real mic. Each app
//! keeps the endpoints it records from: Teams has its own device setting, and
//! the communications default was the wrong (muted) mic on 2026-10-14.
//!
//! The render side lists every session on every active output endpoint with
//! the default outputs, so the coordinator can tell when a call plays to an
//! endpoint per-process loopback does not hear (`detect::them_source`).
//!
//! This replaced the undocumented ConsentStore registry
//! (`CapabilityAccessManager\ConsentStore\microphone`, `LastUsedTimeStart` /
//! `LastUsedTimeStop`). On Windows 11 build 26300 those values stopped
//! changing after the updates installed on 2026-10-03, even for Hark's own
//! open stream, so a Teams call left no trace and was never offered. Core
//! Audio sessions are documented API and report the running stream itself.
//!
//! Window titles are read in memory to test for a meeting marker and never
//! stored, returned or logged.
//!
//! The `probe` facade dispatches to this on Windows, `linux.rs` on Linux,
//! and `mac.rs` on macOS.

use crate::detect::{
    title_has_meeting_marker, Endpoint, MicApp, MicUse, Proc, RenderSession, RenderSnapshot,
    Snapshot, BROWSERS,
};
use std::io;
use windows::core::{Interface, BOOL, PWSTR};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::{
    CloseHandle, ERROR_SUCCESS, HANDLE, HWND, LPARAM, RPC_E_CHANGED_MODE,
};
use windows::Win32::Media::Audio::{
    eCapture, eCommunications, eConsole, eRender, AudioSessionStateActive, EDataFlow, ERole,
    IAudioSessionControl2, IAudioSessionManager2, IMMDevice, IMMDeviceEnumerator,
    MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::Storage::Packaging::Appx::GetPackageFamilyName;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    COINIT_MULTITHREADED, STGM_READ,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
};

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

/// COM for the calling thread, for one snapshot. Bind it before any COM
/// interface in a scope, so it drops (CoUninitialize) after all of them.
struct Apartment {
    owned: bool,
}

impl Apartment {
    fn enter() -> io::Result<Apartment> {
        // SAFETY: initializes COM for this thread only; balanced by Drop.
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if hr == RPC_E_CHANGED_MODE {
            // Already a single-threaded apartment, which Core Audio also
            // serves. Not ours to uninitialize.
            return Ok(Apartment { owned: false });
        }
        hr.ok().map_err(io::Error::other)?;
        Ok(Apartment { owned: true })
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.owned {
            // SAFETY: balances the successful CoInitializeEx in `enter`.
            unsafe { CoUninitialize() };
        }
    }
}

/// Take one detector snapshot. Window titles are only scanned when a browser
/// holds the mic, since only browsers consult them.
pub fn snapshot() -> io::Result<Snapshot> {
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

/// Every app with a running capture stream on any active input endpoint,
/// with the endpoints those streams record from.
fn mic_users() -> io::Result<Vec<MicUse>> {
    let _com = Apartment::enter()?;
    let enumerator = enumerator()?;
    let mut pids: Vec<(u32, Endpoint)> = Vec::new();
    for (endpoint, sessions) in endpoint_sessions(&enumerator, eCapture, true)? {
        pids.extend(sessions.into_iter().map(|(pid, _)| (pid, endpoint.clone())));
    }
    // Stable: one PID's endpoints keep their probe order.
    pids.sort_by_key(|(pid, _)| *pid);
    let mut users: Vec<MicUse> = Vec::new();
    for (pid, endpoint) in pids {
        let Some(app) = app_of(pid) else {
            continue;
        };
        match users.iter_mut().find(|u| u.app == app) {
            Some(user) => {
                if !user.capture_endpoints.contains(&endpoint) {
                    user.capture_endpoints.push(endpoint);
                }
            }
            None => users.push(MicUse {
                app,
                in_use: true,
                capture_endpoints: vec![endpoint],
            }),
        }
    }
    Ok(users)
}

/// Every session on every active output endpoint, and the default outputs
/// for the console and communications roles.
pub fn render_snapshot() -> io::Result<RenderSnapshot> {
    let _com = Apartment::enter()?;
    let enumerator = enumerator()?;
    let mut sessions = Vec::new();
    for (endpoint, found) in endpoint_sessions(&enumerator, eRender, false)? {
        sessions.extend(found.into_iter().map(|(pid, active)| RenderSession {
            pid,
            endpoint: endpoint.clone(),
            active,
        }));
    }
    let default = |role: ERole| {
        // SAFETY: a Core Audio query inside `_com`'s apartment. No default
        // for a role (no output device at all) is a normal state.
        unsafe { enumerator.GetDefaultAudioEndpoint(eRender, role) }
            .and_then(|device| endpoint_of(&device))
            .ok()
    };
    Ok(RenderSnapshot {
        console_default: default(eConsole),
        communications_default: default(eCommunications),
        sessions,
    })
}

fn enumerator() -> io::Result<IMMDeviceEnumerator> {
    // SAFETY: the caller holds a COM apartment that outlives the result.
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.map_err(io::Error::other)
}

/// One endpoint's sessions as `(pid, running)`.
type Sessions = Vec<(u32, bool)>;

/// Each active endpoint of `flow` that has sessions (only running ones with
/// `active_only`), named, with its sessions.
fn endpoint_sessions(
    enumerator: &IMMDeviceEnumerator,
    flow: EDataFlow,
    active_only: bool,
) -> io::Result<Vec<(Endpoint, Sessions)>> {
    // SAFETY: plain Core Audio queries inside the caller's COM apartment.
    let endpoints = unsafe { enumerator.EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE) }
        .map_err(io::Error::other)?;
    // SAFETY: as above.
    let count = unsafe { endpoints.GetCount() }.map_err(io::Error::other)?;
    let mut found = Vec::new();
    for index in 0..count {
        // A device can vanish mid-poll (a headset unplugged): skip it rather
        // than fail the poll and miss the apps on every other device.
        // SAFETY: as above.
        let listed = unsafe { endpoints.Item(index) }.and_then(|device| {
            let mut sessions = sessions_of(&device)?;
            if active_only {
                sessions.retain(|(_, active)| *active);
            }
            if sessions.is_empty() {
                return Ok(None);
            }
            Ok(Some((endpoint_of(&device)?, sessions)))
        });
        match listed {
            Ok(Some(entry)) => found.push(entry),
            Ok(None) => {}
            Err(e) => log::debug!("meeting probe: endpoint {index} skipped ({e})"),
        }
    }
    Ok(found)
}

/// One endpoint's sessions. PID 0 is the system-sounds session, never an app.
fn sessions_of(device: &IMMDevice) -> windows::core::Result<Sessions> {
    // SAFETY: plain Core Audio queries on interfaces owned by this frame,
    // inside the caller's COM apartment.
    unsafe {
        let manager: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None)?;
        let sessions = manager.GetSessionEnumerator()?;
        let mut found = Vec::new();
        for i in 0..sessions.GetCount()? {
            let Ok(session) = sessions.GetSession(i) else {
                continue;
            };
            let active =
                matches!(session.GetState(), Ok(state) if state == AudioSessionStateActive);
            match session
                .cast::<IAudioSessionControl2>()
                .and_then(|s| s.GetProcessId())
            {
                Ok(pid) if pid != 0 => found.push((pid, active)),
                _ => {}
            }
        }
        Ok(found)
    }
}

/// An endpoint's id and friendly name, the pattern
/// `hark_audio::capture_win::query_communications_default` uses.
fn endpoint_of(device: &IMMDevice) -> windows::core::Result<Endpoint> {
    // SAFETY: Core Audio queries inside the caller's COM apartment. GetId's
    // string is ours to free, and is copied out before it is.
    unsafe {
        let raw = device.GetId()?;
        let id = String::from_utf16_lossy(raw.as_wide());
        CoTaskMemFree(Some(raw.0 as *const _));
        let name = device
            .OpenPropertyStore(STGM_READ)?
            .GetValue(&PKEY_Device_FriendlyName)?
            .to_string();
        Ok(Endpoint { id, name })
    }
}

/// The app a capturing process belongs to: its package family when it has
/// package identity, else its exe path. `None` once the process is gone.
fn app_of(pid: u32) -> Option<MicApp> {
    // SAFETY: query-only access to another process; the handle is closed by
    // `Owned`.
    let process =
        Owned(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?);
    if let Some(family) = package_family(&process) {
        return Some(MicApp::Packaged(family));
    }
    image_path(&process).map(MicApp::Desktop)
}

fn package_family(process: &Owned) -> Option<String> {
    // A family name is at most 64 + 1 + 13 characters plus the terminator.
    let mut buf = [0u16; 128];
    let mut len = buf.len() as u32;
    // SAFETY: `len` carries the buffer's capacity; the call writes at most
    // that many UTF-16 units, terminator included, and stores the count.
    let status =
        unsafe { GetPackageFamilyName(process.0, &mut len, Some(PWSTR(buf.as_mut_ptr()))) };
    // APPMODEL_ERROR_NO_PACKAGE for an ordinary desktop process.
    if status != ERROR_SUCCESS {
        return None;
    }
    let len = (len as usize).saturating_sub(1).min(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

fn image_path(process: &Owned) -> Option<String> {
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    // SAFETY: the buffer outlives the call and `len` carries its capacity.
    unsafe {
        QueryFullProcessImageNameW(
            process.0,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
    }
    .ok()?;
    Some(String::from_utf16_lossy(&buf[..len as usize]))
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
    // by `Owned`.
    let process =
        Owned(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?);
    image_path(&process).map(|path| MicApp::Desktop(path).id())
}

/// Every running process, for [`crate::detect::root_pid`].
pub fn processes() -> io::Result<Vec<Proc>> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unpackaged_process_is_reported_by_the_exe_path_hark_excludes() {
        // The detector drops Hark's own pre-roll session by comparing this
        // path with `current_exe()`, so the two spellings must agree.
        let exe = std::env::current_exe().unwrap().display().to_string();
        match app_of(std::process::id()) {
            Some(MicApp::Desktop(path)) => {
                assert!(path.eq_ignore_ascii_case(&exe), "{path} vs {exe}")
            }
            other => panic!("expected a desktop app, got {other:?}"),
        }
    }

    #[test]
    fn a_process_that_is_gone_is_skipped() {
        // PIDs are multiples of four; an odd one never names a process.
        assert_eq!(app_of(u32::MAX - 2), None);
    }
}

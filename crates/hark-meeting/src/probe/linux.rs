//! Linux probe for [`crate::detect`]: who holds the microphone right now,
//! which processes own a meeting-titled window, and the process list for
//! resolving a loopback target. Glue only; the decisions are in `detect`.
//! Live facts are verified with `examples/detect_smoke.rs`; tests exercise
//! isolated transport failures and window-enumeration fixtures.
//!
//! Microphone use comes from PipeWire's graph: an application holding the mic
//! open is a `Stream/Input/Audio` node, and it exists exactly as long as the
//! app keeps the mic — the same lifetime the ConsentStore's `LastUsedTimeStop`
//! expresses on Windows. Apps reach PipeWire through the session manager
//! (directly or via `pipewire-pulse`), and their nodes carry
//! `application.process.binary` and `application.process.id` in the node's
//! info props — the registry's global props filter `application.*` (measured;
//! see `hark-audio`'s loopback), so each capture node is bound and read.
//! Hark's own streams (and its cpal/ALSA capture, which never enters the
//! graph) never count. A snapshot costs one short-lived connection and two
//! roundtrips, a few milliseconds against the 2 s poll.
//!
//! Window titles are read to test for a meeting marker and never stored,
//! returned or logged. Under X11 they come from `_NET_WM_NAME` plus
//! `_NET_WM_PID`; Wayland has no universal toplevel API, so there the probe
//! reports no titles and browser-held-mic detection stays off (native clients
//! detect everywhere) — a documented platform difference, not a silent gap.

use crate::detect::{MicApp, MicUse, Proc, Snapshot, BROWSERS};
use std::cell::RefCell;
use std::io;
use std::rc::Rc;
use std::time::Duration;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::ConnectionExt;

#[path = "watch_linux.rs"]
mod watch;
#[path = "linux_windows.rs"]
mod windows;
pub use watch::ChangeWatcher;

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

/// Bound microphone nodes with their listeners: both must outlive the
/// snapshot's roundtrips, and they die together.
type HeldNodes = Rc<
    RefCell<
        Vec<(
            Box<dyn pipewire::proxy::ProxyT>,
            Box<dyn pipewire::proxy::Listener>,
        )>,
    >,
>;

/// One app's open microphone: a live `Stream/Input/Audio` node.
fn mic_users() -> io::Result<Vec<MicUse>> {
    pipewire::init();
    let err = |what: &str, e: pipewire::Error| io::Error::other(format!("{what}: {e}"));
    let mainloop = pipewire::main_loop::MainLoopRc::new(None)
        .map_err(|e| err("cannot create the PipeWire loop", e))?;
    let context = pipewire::context::ContextRc::new(&mainloop, None)
        .map_err(|e| err("cannot create the PipeWire context", e))?;
    let core = context
        .connect_rc(None)
        .map_err(|e| err("cannot connect to PipeWire", e))?;
    let registry = core
        .get_registry_rc()
        .map_err(|e| err("cannot get the registry", e))?;

    // The bound nodes' info events carry the owning app; the users list is
    // filled as they arrive during the roundtrips below.
    let users: Rc<RefCell<Vec<MicUse>>> = Rc::new(RefCell::new(Vec::new()));
    let registry_weak = registry.downgrade();
    let held: HeldNodes = Rc::new(RefCell::new(Vec::new()));

    let sink_users = users.clone();
    let sink_held = held.clone();
    let self_pid = std::process::id().to_string();
    let self_binary = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().and_then(|n| n.to_str().map(str::to_string)));
    let _reg_l = registry
        .add_listener_local()
        .global(move |obj| {
            if obj.type_ != pipewire::types::ObjectType::Node {
                return;
            }
            let props = obj.props.as_ref();
            let get = |k: &str| props.and_then(|p| p.get(k)).unwrap_or("").to_string();
            if get("media.class") != "Stream/Input/Audio" {
                return;
            }
            let Some(reg) = registry_weak.upgrade() else {
                return;
            };
            let Ok(node) = reg.bind::<pipewire::node::Node, _>(obj) else {
                return;
            };
            let users = sink_users.clone();
            let self_pid = self_pid.clone();
            let self_binary = self_binary.clone();
            let l = node
                .add_listener_local()
                .info(move |info| {
                    let props = info.props();
                    let get = |k: &str| props.and_then(|p| p.get(k)).unwrap_or("").to_string();
                    // Hark's own loopback captures are Stream/Input nodes;
                    // they must never read as somebody's meeting.
                    if get("application.process.id") == self_pid {
                        return;
                    }
                    let binary = get("application.process.binary");
                    if !binary.is_empty() {
                        if let Some(own) = self_binary.as_deref() {
                            if binary.rsplit('/').next() == Some(own) {
                                return;
                            }
                        }
                        let app = MicApp::Desktop(binary);
                        if !users.borrow().iter().any(|u| u.app.id() == app.id()) {
                            users.borrow_mut().push(MicUse { app, in_use: true });
                        }
                    } else {
                        // No binary prop (rare, protocol-native clients):
                        // the node name is the app name the session manager
                        // derived, e.g. "firefox".
                        let name = get("node.name");
                        if !name.is_empty() {
                            let app = MicApp::Desktop(name);
                            if !users.borrow().iter().any(|u| u.app.id() == app.id()) {
                                users.borrow_mut().push(MicUse { app, in_use: true });
                            }
                        }
                    }
                })
                .register();
            sink_held.borrow_mut().push((Box::new(node), Box::new(l)));
        })
        .register();
    let _ = held;
    // One roundtrip drains the enumeration; a second flushes the bound
    // nodes' info events (measured: one is not enough).
    roundtrip(&mainloop, &core, Duration::from_secs(1))?;
    roundtrip(&mainloop, &core, Duration::from_secs(1))?;
    let users = users.borrow().clone();
    Ok(users)
}

fn roundtrip(
    mainloop: &pipewire::main_loop::MainLoopRc,
    core: &pipewire::core::CoreRc,
    timeout: Duration,
) -> io::Result<()> {
    let pending = core
        .sync(0)
        .map_err(|_| io::Error::other("cannot request PipeWire discovery"))?;
    let outcome = Rc::new(RefCell::new(None));
    let done = outcome.clone();
    let ml = mainloop.clone();
    let failed = outcome.clone();
    let error_loop = mainloop.clone();
    let _l = core
        .add_listener_local()
        .done(move |id, seq| {
            if id == pipewire::core::PW_ID_CORE && seq == pending {
                done.borrow_mut().get_or_insert(Ok(()));
                ml.quit();
            }
        })
        .error(move |id, _seq, code, _message| {
            if id == pipewire::core::PW_ID_CORE {
                *failed.borrow_mut() = Some(Err(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    format!("PipeWire discovery connection failed ({code})"),
                )));
                error_loop.quit();
            }
        })
        .register();
    // The timer must cover the first enumeration too, before capture's own
    // startup watchdog exists. Peer disconnects do not stop a PipeWire loop.
    let expired = outcome.clone();
    let timeout_loop = mainloop.clone();
    let timer = mainloop.loop_().add_timer(move |_| {
        expired.borrow_mut().get_or_insert_with(|| {
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "PipeWire discovery timed out",
            ))
        });
        timeout_loop.quit();
    });
    timer
        .update_timer(Some(timeout), None)
        .into_result()
        .map_err(|_| io::Error::other("cannot arm PipeWire discovery timeout"))?;
    mainloop.run();
    let result = outcome
        .borrow_mut()
        .take()
        .unwrap_or_else(|| Err(io::Error::other("PipeWire discovery interrupted")));
    result
}

/// Lowercase exe names of processes owning a top-level window whose title
/// has a meeting marker. X11 only; an empty list under Wayland.
fn meeting_window_exes() -> Vec<String> {
    let (conn, screen) = match x11rb::connect(None) {
        Ok(c) => c,
        Err(_) => {
            // Wayland, or no display: the browser signal is simply absent.
            return Vec::new();
        }
    };
    let conn = &conn;
    // `connect` hands back the screen's index, not its struct.
    let Some(screen) = conn.setup().roots.get(screen) else {
        return Vec::new();
    };
    let mut exes = Vec::new();
    for window in windows::clients(conn, screen.root) {
        let title = window_title(conn, window);
        if !crate::detect::title_has_meeting_marker(&title) {
            continue;
        }
        if let Some(pid) = window_pid(conn, window) {
            if let Some(exe) = exe_name_of(pid) {
                exes.push(exe);
            }
        }
    }
    exes.sort();
    exes.dedup();
    exes
}

fn window_title(conn: &x11rb::rust_connection::RustConnection, window: u32) -> String {
    // _NET_WM_NAME (UTF-8) with the Latin-1 WM_NAME as the fallback.
    for (atom, utf8) in [("_NET_WM_NAME", true), ("WM_NAME", false)] {
        let Ok(atom) = conn.intern_atom(false, atom.as_bytes()) else {
            continue;
        };
        let Ok(atom) = atom.reply() else {
            continue;
        };
        let Ok(reply) = conn
            .get_property(
                false,
                window,
                atom.atom,
                x11rb::protocol::xproto::AtomEnum::ANY,
                0,
                512,
            )
            .map_err(Into::into)
            .and_then(|c| c.reply())
        else {
            continue;
        };
        if reply.value.is_empty() {
            continue;
        }
        if utf8 {
            return String::from_utf8_lossy(&reply.value).into_owned();
        }
        return reply.value.iter().map(|&b| b as char).collect();
    }
    String::new()
}

fn window_pid(conn: &x11rb::rust_connection::RustConnection, window: u32) -> Option<u32> {
    let atom = conn.intern_atom(false, b"_NET_WM_PID").ok()?.reply().ok()?;
    let reply = conn
        .get_property(
            false,
            window,
            atom.atom,
            x11rb::protocol::xproto::AtomEnum::CARDINAL,
            0,
            1,
        )
        .ok()?
        .reply()
        .ok()?;
    // _NET_WM_PID is a 32-bit cardinal, little-endian in the reply.
    reply
        .value
        .get(..4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// The process's exe file name from `/proc`, lowercase; `comm` (15 chars,
/// no path) when the exe is unreadable.
fn exe_name_of(pid: u32) -> Option<String> {
    let exe = std::fs::read_link(format!("/proc/{pid}/exe"))
        .ok()
        .and_then(|p| p.file_name().and_then(|n| n.to_str().map(str::to_string)));
    match exe {
        Some(name) => Some(name.to_lowercase()),
        None => std::fs::read_to_string(format!("/proc/{pid}/comm"))
            .ok()
            .map(|c| c.trim().to_lowercase()),
    }
}

pub(super) fn processes() -> io::Result<Vec<Proc>> {
    let mut procs = Vec::new();
    for entry in std::fs::read_dir("/proc")? {
        let Ok(name) = entry?.file_name().into_string() else {
            continue;
        };
        let Ok(pid) = name.parse::<u32>() else {
            continue;
        };
        // stat: "pid (comm with spaces) state ppid ..."; the parenthesised
        // comm may itself contain spaces, so parse after the last ')'.
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
            continue;
        };
        let Some(after) = stat.rsplit(')').next() else {
            continue;
        };
        let mut fields = after.split_whitespace();
        let _state = fields.next();
        let Some(parent) = fields.next().and_then(|v| v.parse().ok()) else {
            continue;
        };
        let exe = std::fs::read_link(format!("/proc/{pid}/exe"))
            .ok()
            .and_then(|p| p.file_name().and_then(|n| n.to_str().map(str::to_string)))
            .or_else(|| std::fs::read_to_string(format!("/proc/{pid}/comm")).ok())
            .map(|c| c.trim().to_string())
            .unwrap_or_default();
        if exe.is_empty() {
            continue;
        }
        procs.push(Proc { pid, parent, exe });
    }
    Ok(procs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_timeout_and_disconnect_exit_the_real_pipewire_loop() {
        use std::os::unix::net::UnixStream;

        let config_dir = tempfile::tempdir().unwrap();
        let config = config_dir.path().join("client.conf");
        std::fs::write(
            &config,
            "context.modules = [ { name = libpipewire-module-protocol-native } ]",
        )
        .unwrap();
        for disconnect in [false, true] {
            pipewire::init();
            let mainloop = pipewire::main_loop::MainLoopRc::new(None).unwrap();
            let props = pipewire::properties::properties! {
                "config.name" => config.to_str().unwrap(),
            };
            let context = pipewire::context::ContextRc::new(&mainloop, Some(props)).unwrap();
            let (client, peer) = UnixStream::pair().unwrap();
            let core = context.connect_fd_rc(client.into(), None).unwrap();
            let peer = (!disconnect).then_some(peer);
            let error = roundtrip(&mainloop, &core, Duration::from_millis(50)).unwrap_err();
            assert_eq!(
                error.kind(),
                if disconnect {
                    io::ErrorKind::ConnectionAborted
                } else {
                    io::ErrorKind::TimedOut
                }
            );
            drop(peer);
        }
    }

    #[test]
    fn the_process_list_contains_us_and_the_kernel_init() {
        let procs = processes().expect("reading /proc works on Linux");
        assert!(procs.iter().any(|p| p.pid == std::process::id()));
        assert!(procs.iter().any(|p| p.pid == 1));
        assert!(
            procs.iter().all(|p| !p.exe.is_empty()),
            "every listed process has an exe name"
        );
    }

    #[test]
    fn a_snapshot_never_lists_hark_itself() {
        // No PipeWire in `cargo test`: the snapshot errors instead of lying.
        // When a session manager does run, our own streams must not count.
        if let Ok(snapshot) = snapshot() {
            let self_exe = std::env::current_exe()
                .ok()
                .and_then(|p| p.file_name().and_then(|n| n.to_str().map(str::to_string)))
                .unwrap_or_default();
            assert!(!snapshot
                .users
                .iter()
                .any(|u| u.app.id() == self_exe.to_lowercase()));
        }
    }
}

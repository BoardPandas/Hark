//! Where to capture a detected app's audio from: the process tree to loop
//! back, or the output endpoint it is playing to.

use super::Endpoint;

/// One process's audio session on an output endpoint.
#[derive(Debug, Clone)]
pub struct RenderSession {
    pub pid: u32,
    pub endpoint: Endpoint,
    /// A stream is playing (`AudioSessionStateActive`), not merely open.
    pub active: bool,
}

/// Where the machine plays audio right now. Empty off Windows.
#[derive(Debug, Clone, Default)]
pub struct RenderSnapshot {
    pub sessions: Vec<RenderSession>,
    /// The default output for the console role: the one Sound settings shows.
    pub console_default: Option<Endpoint>,
    /// The default output for the communications role, where call apps play
    /// unless told otherwise.
    pub communications_default: Option<Endpoint>,
}

/// Where to capture a meeting app's audio from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemSource {
    /// Per-process loopback of the app's tree.
    ProcessTree,
    /// Loopback of this whole output endpoint.
    Endpoint(Endpoint),
}

/// The rule for "Them": loopback of the endpoint the app is playing to, else
/// per-process loopback.
///
/// Microsoft documents process loopback as endpoint-independent, but new
/// Teams was measured invisible to it on Windows 11 26300: -96.7 dBFS playing
/// to the communications endpoint (2026-10-14) and -96.3 dBFS playing to the
/// console default (2026-10-15), while endpoint loopback of the same device
/// heard the call at -5 and -15 dBFS. So an app seen playing is recorded from
/// that endpoint, at the cost of also hearing anything else playing there.
/// Process loopback remains for an app not playing yet when the meeting
/// starts. With several endpoints the communications default wins (calls
/// play there), then the console default.
pub fn them_source(render: &RenderSnapshot, procs: &[Proc], root: u32) -> ThemSource {
    let playing: Vec<&Endpoint> = tree_sessions(render, procs, root)
        .into_iter()
        .filter(|s| s.active)
        .map(|s| &s.endpoint)
        .collect();
    let defaults = [&render.communications_default, &render.console_default];
    let chosen = defaults
        .iter()
        .find_map(|d| {
            d.as_ref()
                .and_then(|d| playing.iter().find(|e| e.id == d.id))
        })
        .or(playing.first());
    match chosen {
        Some(endpoint) => ThemSource::Endpoint((*endpoint).clone()),
        None => ThemSource::ProcessTree,
    }
}

/// The render sessions of `root` and its descendants.
pub fn tree_sessions<'a>(
    render: &'a RenderSnapshot,
    procs: &[Proc],
    root: u32,
) -> Vec<&'a RenderSession> {
    let tree = process_tree(procs, root);
    render
        .sessions
        .iter()
        .filter(|s| tree.contains(&s.pid))
        .collect()
}

/// `root` and every process descended from it. Windows reuses PIDs, so a
/// stale parent link can point anywhere, even into a cycle: each PID is
/// visited once.
pub fn process_tree(procs: &[Proc], root: u32) -> Vec<u32> {
    let mut tree = vec![root];
    let mut next = 0;
    while next < tree.len() {
        let parent = tree[next];
        next += 1;
        for p in procs {
            if p.parent == parent && p.pid != parent && !tree.contains(&p.pid) {
                tree.push(p.pid);
            }
        }
    }
    tree
}

/// A process, for resolving a meeting app's loopback target.
#[derive(Debug, Clone)]
pub struct Proc {
    pub pid: u32,
    pub parent: u32,
    /// Exe file name, any case.
    pub exe: String,
}

/// The exe whose process tree carries a detected app's audio. Packaged apps
/// are keyed by family, so they map to their exe here.
pub fn target_exe(app_id: &str) -> String {
    match app_id {
        "msteams_8wekyb3d8bbwe" => "ms-teams.exe".to_string(),
        other => other.to_string(),
    }
}

/// The root of `exe`'s process tree: a process running `exe` whose parent is
/// not also `exe` (browsers and Electron apps run many children of one
/// root). Per-process loopback in include mode on this PID covers the app's
/// audio children. With several roots (two browser profiles), the lowest PID
/// wins so the choice is stable across polls.
pub fn root_pid(procs: &[Proc], exe: &str) -> Option<u32> {
    let runs_exe = |pid: u32| {
        procs
            .iter()
            .any(|p| p.pid == pid && p.exe.eq_ignore_ascii_case(exe))
    };
    procs
        .iter()
        .filter(|p| p.exe.eq_ignore_ascii_case(exe) && !runs_exe(p.parent))
        .map(|p| p.pid)
        .min()
}

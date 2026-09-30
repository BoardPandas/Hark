//! macOS detection facts come from Core Audio process objects (14.2+).
//! A running application alone never counts as microphone use. Browser
//! titles are inspected only in memory and absent titles fail closed.
use crate::detect::{title_has_meeting_marker, MicApp, MicUse, Proc, Snapshot, BROWSERS};
use std::io;

pub fn snapshot() -> io::Result<Snapshot> {
    let mut snapshot = Snapshot::default();
    let processes = hark_audio::core_audio_mac::process_snapshot(|id, title| {
        if BROWSERS.contains(&id)
            && title_has_meeting_marker(title)
            && !snapshot
                .meeting_windows
                .iter()
                .any(|existing| existing == id)
        {
            snapshot.meeting_windows.push(id.to_owned());
        }
    })?;
    snapshot.users = processes
        .into_iter()
        .filter(|p| p.input_running)
        .map(|p| MicUse {
            // A macOS bundle identifier has the same stable matching semantics as
            // a Windows package family; Hark's PID was excluded by the native probe.
            app: MicApp::Packaged(p.app_id),
            in_use: true,
        })
        .collect();
    Ok(snapshot)
}

pub fn processes() -> io::Result<Vec<Proc>> {
    Ok(hark_audio::core_audio_mac::process_snapshot(|_, _| {})?
        .into_iter()
        .map(|p| Proc {
            pid: p.pid,
            parent: p.parent,
            exe: p.app_id,
        })
        .collect())
}

//! The detection probe for [`crate::detect`]: who holds the microphone right
//! now, which processes own a meeting-titled window, and the process list for
//! resolving a loopback target. Glue only; the decisions are in `detect`.
//!
//! `win.rs` reads Core Audio capture sessions and `EnumWindows`; `linux.rs`
//! reads PipeWire's graph and `/proc`; `mac.rs` reads Core Audio process
//! objects. All are verified by hand with `examples/detect_smoke.rs`, never
//! in `cargo test`.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod mac;
#[cfg(windows)]
mod win;

#[cfg(target_os = "linux")]
pub use linux::ChangeWatcher;

use crate::detect::{Proc, Snapshot};
use std::io;

/// Take one detector snapshot. Window titles are only scanned when a browser
/// holds the mic, since only browsers consult them.
pub fn snapshot() -> io::Result<Snapshot> {
    #[cfg(windows)]
    {
        win::snapshot()
    }
    #[cfg(target_os = "linux")]
    {
        linux::snapshot()
    }
    #[cfg(target_os = "macos")]
    {
        mac::snapshot()
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        Err(unsupported())
    }
}

/// Every running process, for [`crate::detect::root_pid`].
pub fn processes() -> io::Result<Vec<Proc>> {
    #[cfg(windows)]
    {
        win::processes()
    }
    #[cfg(target_os = "linux")]
    {
        linux::processes()
    }
    #[cfg(target_os = "macos")]
    {
        mac::processes()
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        Err(unsupported())
    }
}

/// Change notification exists only on Linux (PipeWire graph events). Windows
/// and macOS are polled: both report capture per audio session or per
/// process, so a watcher would have to subscribe to each one as it appears,
/// while one snapshot costs a few milliseconds. `start` returns `Unsupported`
/// and the coordinator keeps its two-second polling fallback. Owns no worker.
#[cfg(not(target_os = "linux"))]
pub struct ChangeWatcher {
    _private: (),
}

#[cfg(not(target_os = "linux"))]
impl ChangeWatcher {
    pub fn start(on_change: impl FnMut() + Send + 'static) -> io::Result<Self> {
        let _ = on_change;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "microphone change notifications are not available on this platform",
        ))
    }

    pub fn is_alive(&self) -> bool {
        false
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "meeting detection is not implemented on this platform",
    )
}

#[cfg(all(test, not(target_os = "linux")))]
mod tests {
    use super::*;

    #[test]
    fn change_notifications_are_explicitly_unsupported_off_linux() {
        match ChangeWatcher::start(|| panic!("unsupported watcher must not invoke its callback")) {
            Err(error) => assert_eq!(error.kind(), io::ErrorKind::Unsupported),
            Ok(_) => panic!("change watcher unexpectedly started"),
        }
    }
}

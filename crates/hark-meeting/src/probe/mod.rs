//! The detection probe for [`crate::detect`]: who holds the microphone right
//! now, which processes own a meeting-titled window, and the process list for
//! resolving a loopback target. Glue only; the decisions are in `detect`.
//!
//! `win.rs` reads the ConsentStore registry and `EnumWindows`; `linux.rs`
//! reads PipeWire's graph and `/proc`; `mac.rs` reads Core Audio process
//! objects. All are verified by hand with `examples/detect_smoke.rs`, never
//! in `cargo test`.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod mac;
// `win.rs` compiles on macOS too: its watcher (registry-free off Windows)
// gives the coordinator the `Unsupported` start that keeps its polling
// backstop alive there, exactly as before the module split.
#[cfg(any(windows, target_os = "macos"))]
mod win;

#[cfg(target_os = "linux")]
pub use linux::ChangeWatcher;
#[cfg(any(windows, target_os = "macos"))]
pub use win::ChangeWatcher;

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

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "meeting detection is not implemented on this platform",
    )
}

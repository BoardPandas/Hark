//! Single-instance guard for Hark.
//!
//! Launch-at-login plus a manual launch (or a double-click on the installed
//! shortcut while Hark sits in the tray) otherwise starts a second process.
//! Two Hark processes means two low-level keyboard hooks racing for the same
//! push-to-talk chord, two tray icons, and two writers on one SQLite file.
//!
//! The lock is held by an OS object that dies with the process, never by a
//! PID file: a hard-killed Hark must not lock its own next launch out.
//!
//! - **Windows:** a named mutex in the `Local\` namespace. Session-scoped on
//!   purpose — under fast user switching or RDP each logged-in user gets their
//!   own Hark, with their own tray, config, and keychain entries.
//! - **Unix (macOS):** `flock(LOCK_EX | LOCK_NB)` on a file in the per-user
//!   data dir. The kernel drops the lock when the fd closes, including on
//!   abnormal termination, so a stale file on disk is inert.
//!
//! Callers **fail open**: if the check itself errors, start anyway. A guard
//! that can refuse to launch the app is a worse bug than the double instance
//! it prevents.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[cfg(windows)]
    #[error("cannot create the instance mutex: {0}")]
    Mutex(#[source] windows::core::Error),
    #[cfg(unix)]
    #[error("no per-user data directory to hold the lock file")]
    NoDataDir,
    #[cfg(unix)]
    #[error("cannot open the lock file at {path}: {source}")]
    LockFile {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[cfg(unix)]
    #[error("cannot lock {path}: {source}")]
    Flock {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Proof that this process owns the single-instance lock.
///
/// Dropping it releases the lock, so it must be bound to a live variable that
/// outlives the app — `let _guard = ...`, never `let _ = ...`, which drops at
/// the end of the statement and re-opens the door a second instance came in
/// through.
#[must_use = "binding to `_` drops the guard immediately and releases the lock"]
#[derive(Debug)]
pub struct InstanceGuard(
    /// Held purely for its `Drop`; nothing ever reads it.
    #[allow(dead_code)]
    imp::Guard,
);

/// Claim the single-instance lock for this process.
///
/// `Ok(Some(guard))` — this is the only Hark; hold the guard for the process
/// lifetime. `Ok(None)` — another Hark already holds it; exit quietly.
/// `Err(_)` — the check itself failed and decided nothing; start anyway.
pub fn acquire() -> Result<Option<InstanceGuard>, Error> {
    Ok(imp::acquire()?.map(InstanceGuard))
}

#[cfg(windows)]
mod imp {
    use super::Error;
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
    use windows::Win32::System::Threading::CreateMutexW;

    /// `Local\` scopes the object to the logon session (see module docs). The
    /// GUID makes the name collision-proof against unrelated software and is
    /// **permanent**: changing it silently disables the guard for anyone
    /// running a mixed pair of versions during an upgrade.
    const MUTEX_NAME: &str = r"Local\Hark-SingleInstance-9F2A7C41-6B3E-4D58-B0A9-2E7C5D1F84B6";

    /// Owns the mutex handle; the named object lives as long as any handle to
    /// it is open, so closing this is what frees the name for the next launch.
    #[derive(Debug)]
    pub(super) struct Guard(HANDLE);

    impl Drop for Guard {
        fn drop(&mut self) {
            // Nothing actionable if this fails, and the process is on its way
            // out: the kernel closes the handle regardless.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    pub(super) fn acquire() -> Result<Option<Guard>, Error> {
        // `binitialowner: false` — we never take ownership. Existence of the
        // named object is the signal; not owning it means there is no
        // abandoned-mutex state to reason about if a Hark process is killed.
        let handle = unsafe { CreateMutexW(None, false, &HSTRING::from(MUTEX_NAME)) }
            .map_err(Error::Mutex)?;

        // CreateMutexW succeeds either way when the name is taken, handing
        // back a second handle to the *existing* object and setting the last
        // error. The success path of the windows-rs wrapper does not touch
        // the thread's last-error value, so this read is the real one.
        let already_running = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;

        let guard = Guard(handle);
        if already_running {
            // Dropping `guard` closes our redundant handle; the first
            // instance's handle keeps the object alive.
            return Ok(None);
        }
        Ok(Some(guard))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn second_acquire_sees_the_first() {
            let first = acquire().expect("first acquire").expect("lock is free");
            assert!(
                acquire().expect("second acquire").is_none(),
                "a second claim must report the instance already running"
            );
            drop(first);
            assert!(
                acquire().expect("third acquire").is_some(),
                "releasing the guard must free the name for the next launch"
            );
        }
    }
}

#[cfg(unix)]
mod imp {
    use super::Error;
    use std::fs::{File, OpenOptions};
    use std::os::fd::AsRawFd;
    use std::path::Path;

    const LOCK_FILE: &str = "instance.lock";

    /// Owns the open fd. `flock` locks belong to the open file description,
    /// so the lock lives exactly as long as this `File` — including through a
    /// crash, where the kernel closes it for us.
    #[derive(Debug)]
    pub(super) struct Guard(#[allow(dead_code)] File);

    pub(super) fn acquire() -> Result<Option<Guard>, Error> {
        let dir = hark_config::default_data_dir().ok_or(Error::NoDataDir)?;
        acquire_at(&dir.join(LOCK_FILE))
    }

    /// The lock file is never deleted. Unlinking it on release would let a
    /// launch racing that release lock a file that is already unreachable by
    /// name, and both instances would then think they were alone. An empty
    /// file left behind costs nothing.
    fn acquire_at(path: &Path) -> Result<Option<Guard>, Error> {
        let open_err = |source| Error::LockFile {
            path: path.to_path_buf(),
            source,
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(open_err)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path)
            .map_err(open_err)?;

        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(Some(Guard(file)));
        }
        let e = std::io::Error::last_os_error();
        // EWOULDBLOCK (== EAGAIN) is the "someone else holds it" answer, not a
        // failure; anything else means the check itself broke.
        if e.raw_os_error() == Some(libc::EWOULDBLOCK) {
            Ok(None)
        } else {
            Err(Error::Flock {
                path: path.to_path_buf(),
                source: e,
            })
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn second_acquire_sees_the_first() {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.path().join(LOCK_FILE);

            let first = acquire_at(&path).expect("first acquire").expect("free");
            // flock is per open file description, not per process, so a second
            // open in this same test process contends exactly like a second
            // Hark process would.
            assert!(
                acquire_at(&path).expect("second acquire").is_none(),
                "a second claim must report the instance already running"
            );
            drop(first);
            assert!(
                acquire_at(&path).expect("third acquire").is_some(),
                "releasing the guard must free the lock for the next launch"
            );
        }

        #[test]
        fn creates_the_lock_file_and_its_parent() {
            let dir = tempfile::tempdir().expect("tempdir");
            // The data dir does not exist yet on a first-ever launch.
            let path = dir.path().join("hark").join(LOCK_FILE);

            let _guard = acquire_at(&path).expect("acquire").expect("free");
            assert!(path.exists(), "the lock file must be created on demand");
        }
    }
}

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
//!
//! # Activation
//!
//! Refusing to start is only half the job. Launching Hark from the Start menu
//! (or its shortcut, or the taskbar) while it already runs in the tray must
//! surface the *running* window, not exit silently — silence is
//! indistinguishable from a broken app. So the losing process signals the
//! winner over a second named OS object before it exits, and the winner shows
//! its window:
//!
//! - **Windows:** an auto-reset named event, `SetEvent` from the loser and a
//!   blocking `WaitForSingleObject` thread in the winner. Same `Local\`
//!   session scoping as the mutex.
//! - **Unix (macOS):** a Unix-domain socket next to the lock file; one
//!   accepted connection is one activation.
//!
//! Activation is best-effort in both directions: a failed signal only costs
//! the user a second click, so it is logged and never fatal.

use std::time::{Duration, Instant};
use thiserror::Error;

/// How long [`signal_existing`] retries when the activation object does not
/// exist yet. The winner claims the mutex before it starts listening, so a
/// launch that lands in that window would otherwise find nothing to signal.
const SIGNAL_WAIT: Duration = Duration::from_secs(2);
const SIGNAL_POLL: Duration = Duration::from_millis(100);

#[derive(Debug, Error)]
pub enum Error {
    #[cfg(windows)]
    #[error("cannot create the instance mutex: {0}")]
    Mutex(#[source] windows::core::Error),
    #[cfg(windows)]
    #[error("cannot create the activation event: {0}")]
    Event(#[source] windows::core::Error),
    #[cfg(unix)]
    #[error("cannot listen for activation on {path}: {source}")]
    ActivationSocket {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("no running instance answered the activation signal")]
    NoListener,
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

/// Proof that this process is listening for activation requests.
///
/// Like [`InstanceGuard`], it must be bound to a live variable: dropping it
/// stops the listener (and, on Unix, removes the socket), after which a second
/// launch has nothing to signal and exits silently again.
#[must_use = "binding to `_` drops the listener immediately and stops activation"]
pub struct ActivationListener(
    #[allow(dead_code)] // Held purely for its `Drop`.
    imp::Listener,
);

/// Start listening for activation requests from later launches.
///
/// `on_activate` runs on a background thread, once per request, for the life
/// of the listener — so it must do only what is safe off the main thread. The
/// UI's callback sends on a channel and wakes the event loop; it never touches
/// the window directly (the macOS main-thread rule).
///
/// Only the process holding the [`InstanceGuard`] may call this: two listeners
/// on one name would split activations between them.
pub fn listen(on_activate: impl FnMut() + Send + 'static) -> Result<ActivationListener, Error> {
    Ok(ActivationListener(imp::listen(Box::new(on_activate))?))
}

/// Ask the already-running instance to show its window, then exit.
///
/// Called by the losing process after [`acquire`] returns `Ok(None)`. Retries
/// for [`SIGNAL_WAIT`] because the winner claims the mutex before it starts
/// listening; `Err(Error::NoListener)` means it never answered, which is worth
/// a log line but is not otherwise actionable.
pub fn signal_existing() -> Result<(), Error> {
    let deadline = Instant::now() + SIGNAL_WAIT;
    loop {
        if imp::signal()? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(Error::NoListener);
        }
        std::thread::sleep(SIGNAL_POLL);
    }
}

#[cfg(windows)]
mod imp {
    use super::Error;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread::JoinHandle;
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, WAIT_OBJECT_0,
    };
    use windows::Win32::System::Threading::{
        CreateEventW, CreateMutexW, OpenEventW, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE,
        INFINITE,
    };

    /// `Local\` scopes the object to the logon session (see module docs). The
    /// GUID makes the name collision-proof against unrelated software and is
    /// **permanent**: changing it silently disables the guard for anyone
    /// running a mixed pair of versions during an upgrade.
    const MUTEX_NAME: &str = r"Local\Hark-SingleInstance-9F2A7C41-6B3E-4D58-B0A9-2E7C5D1F84B6";

    /// The activation event, scoped and permanent for the same reasons as
    /// [`MUTEX_NAME`]: a version that renames it cannot be activated by (or
    /// activate) the version it is replacing.
    const EVENT_NAME: &str = r"Local\Hark-Activate-1D4B8E07-3A6F-42C9-9E51-7B0C86D2A3F4";

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

    /// Owns the event handle and the waiting thread. `Drop` signals the event
    /// one last time with `stopping` set, so the thread wakes, sees the flag,
    /// and returns instead of parking in `INFINITE` forever.
    pub(super) struct Listener {
        event: HANDLE,
        stopping: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }

    // HANDLE is a raw pointer, so it is not Send by default. A kernel handle is
    // process-wide and every API used on it here is thread-safe, which is
    // exactly what the auto-derive cannot know.
    unsafe impl Send for Listener {}

    impl Drop for Listener {
        fn drop(&mut self) {
            self.stopping.store(true, Ordering::SeqCst);
            unsafe {
                let _ = SetEvent(self.event);
            }
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
            unsafe {
                let _ = CloseHandle(self.event);
            }
        }
    }

    pub(super) fn listen(mut on_activate: Box<dyn FnMut() + Send>) -> Result<Listener, Error> {
        // Auto-reset (`bmanualreset: false`): each wait consumes exactly one
        // signal, so two launches in quick succession are two activations. A
        // signal raised while nobody is waiting stays latched, so an activation
        // sent between two waits is never lost.
        let event = unsafe { CreateEventW(None, false, false, &HSTRING::from(EVENT_NAME)) }
            .map_err(Error::Event)?;

        let stopping = Arc::new(AtomicBool::new(false));
        let thread_stopping = Arc::clone(&stopping);
        // The handle is copied into the thread deliberately: `Drop` closes it
        // only after joining, so it cannot be used after close.
        let thread_event = SendHandle(event);
        let thread = std::thread::Builder::new()
            .name("hark-activation".into())
            .spawn(move || {
                // Read through a method, never by destructuring: edition 2021
                // captures the *fields* a closure touches, so `let
                // SendHandle(event) = thread_event` would capture the bare
                // (non-Send) HANDLE and fail to compile.
                let event = thread_event.get();
                loop {
                    if unsafe { WaitForSingleObject(event, INFINITE) } != WAIT_OBJECT_0 {
                        // The wait itself failed, which cannot be retried into
                        // working; leaving the loop parks activation rather
                        // than spinning a core on the same error.
                        return;
                    }
                    if thread_stopping.load(Ordering::SeqCst) {
                        return;
                    }
                    on_activate();
                }
            })
            .map_err(|_| Error::NoListener)?;

        Ok(Listener {
            event,
            stopping,
            thread: Some(thread),
        })
    }

    /// Moves a handle into the listener thread. Same reasoning as the `Send`
    /// impl on `Listener`; a named wrapper keeps the unsafety at one line.
    struct SendHandle(HANDLE);
    unsafe impl Send for SendHandle {}

    impl SendHandle {
        fn get(&self) -> HANDLE {
            self.0
        }
    }

    /// `Ok(false)` when no instance is listening yet, which the caller retries.
    pub(super) fn signal() -> Result<bool, Error> {
        // A failure here is overwhelmingly ERROR_FILE_NOT_FOUND ("not listening
        // yet"), which is a retry, not an error; a genuinely broken open would
        // fail the same way on every poll and end as NoListener.
        let Ok(event) =
            (unsafe { OpenEventW(EVENT_MODIFY_STATE, false, &HSTRING::from(EVENT_NAME)) })
        else {
            return Ok(false);
        };
        let set = unsafe { SetEvent(event) };
        unsafe {
            let _ = CloseHandle(event);
        }
        set.map_err(Error::Event)?;
        Ok(true)
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
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread::JoinHandle;

    const LOCK_FILE: &str = "instance.lock";
    const ACTIVATION_SOCKET: &str = "activate.sock";

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

    /// Owns the accept thread and the socket path. `Drop` unlinks the socket
    /// and connects to itself once, so the blocking `accept` wakes and sees
    /// `stopping`.
    pub(super) struct Listener {
        path: PathBuf,
        stopping: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }

    impl Drop for Listener {
        fn drop(&mut self) {
            self.stopping.store(true, Ordering::SeqCst);
            let _ = UnixStream::connect(&self.path);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
            // Unlike the lock file, the socket must go: a leftover path makes
            // `bind` fail on the next launch, and unlinking is safe because
            // only the instance holding the flock ever gets here.
            let _ = std::fs::remove_file(&self.path);
        }
    }

    pub(super) fn listen(on_activate: Box<dyn FnMut() + Send>) -> Result<Listener, Error> {
        let dir = hark_config::default_data_dir().ok_or(Error::NoDataDir)?;
        listen_at(&dir.join(ACTIVATION_SOCKET), on_activate)
    }

    fn listen_at(path: &Path, mut on_activate: Box<dyn FnMut() + Send>) -> Result<Listener, Error> {
        let err = |source| Error::ActivationSocket {
            path: path.to_path_buf(),
            source,
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(err)?;
        }
        // A socket left by a crashed instance would fail `bind` with EADDRINUSE
        // forever. Removing it is safe here and only here: the caller holds the
        // single-instance lock, so no live Hark is listening on it.
        let _ = std::fs::remove_file(path);
        let listener = UnixListener::bind(path).map_err(err)?;

        let stopping = Arc::new(AtomicBool::new(false));
        let thread_stopping = Arc::clone(&stopping);
        let thread = std::thread::Builder::new()
            .name("hark-activation".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    if thread_stopping.load(Ordering::SeqCst) {
                        return;
                    }
                    // One connection is one activation; the peer sends nothing,
                    // so there is no payload to read.
                    if stream.is_ok() {
                        on_activate();
                    }
                }
            })
            .map_err(|_| Error::NoListener)?;

        Ok(Listener {
            path: path.to_path_buf(),
            stopping,
            thread: Some(thread),
        })
    }

    pub(super) fn signal() -> Result<bool, Error> {
        let dir = hark_config::default_data_dir().ok_or(Error::NoDataDir)?;
        Ok(signal_at(&dir.join(ACTIVATION_SOCKET)))
    }

    /// False when nothing is listening (no socket, or a stale one with no
    /// accepting peer), which the caller retries.
    fn signal_at(path: &Path) -> bool {
        UnixStream::connect(path).is_ok()
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

        /// Blocks until `count` activations arrive, so the test never sleeps on
        /// a fixed duration (the accept thread's timing is not ours to assume).
        fn wait_for(rx: &std::sync::mpsc::Receiver<()>, count: usize) {
            for i in 0..count {
                rx.recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap_or_else(|_| panic!("activation {} never arrived", i + 1));
            }
        }

        #[test]
        fn each_signal_delivers_one_activation() {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.path().join(ACTIVATION_SOCKET);
            let (tx, rx) = std::sync::mpsc::channel();

            let listener =
                listen_at(&path, Box::new(move || tx.send(()).unwrap())).expect("listen");
            assert!(signal_at(&path), "first signal must reach the listener");
            assert!(signal_at(&path), "a second launch signals again");
            wait_for(&rx, 2);
            drop(listener);
        }

        #[test]
        fn signalling_with_no_listener_reports_false() {
            let dir = tempfile::tempdir().expect("tempdir");
            assert!(!signal_at(&dir.path().join(ACTIVATION_SOCKET)));
        }

        #[test]
        fn a_stale_socket_does_not_block_the_next_listener() {
            // What a hard-killed Hark leaves behind: the path exists, nothing
            // is accepting on it. The next launch must still be able to bind.
            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.path().join(ACTIVATION_SOCKET);
            drop(UnixListener::bind(&path).expect("stale socket"));
            assert!(path.exists(), "the stale socket must outlive its listener");

            let (tx, rx) = std::sync::mpsc::channel();
            let listener = listen_at(&path, Box::new(move || tx.send(()).unwrap()))
                .expect("a stale socket must not block bind");
            assert!(signal_at(&path));
            wait_for(&rx, 1);
            drop(listener);
        }

        #[test]
        fn dropping_the_listener_removes_the_socket_and_stops_activation() {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.path().join(ACTIVATION_SOCKET);
            let (tx, _rx) = std::sync::mpsc::channel();

            let listener =
                listen_at(&path, Box::new(move || tx.send(()).unwrap())).expect("listen");
            drop(listener);
            assert!(!path.exists(), "Drop must unlink the socket");
            assert!(!signal_at(&path), "a dropped listener answers nothing");
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

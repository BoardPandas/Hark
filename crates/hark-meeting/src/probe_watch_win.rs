//! Change notification for the ConsentStore probe. The worker only wakes the
//! coordinator; snapshots and detector decisions remain with their owner.

use std::io;

/// A recursive ConsentStore watch, with explicit shutdown independent of the
/// callback's channel. Drop this before waiting for that channel to disconnect.
pub struct ChangeWatcher {
    #[cfg(windows)]
    running: win::Running,
}

impl ChangeWatcher {
    /// Start observing microphone-use registry changes. `on_change` must only
    /// enqueue a wakeup and return; registry contents never leave this worker.
    /// A missing/inaccessible ConsentStore is an error so the caller can retain
    /// its polling backstop. This does not create registry keys.
    pub fn start(on_change: impl FnMut() + Send + 'static) -> io::Result<Self> {
        #[cfg(windows)]
        {
            Ok(Self {
                running: win::Running::start(on_change)?,
            })
        }
        #[cfg(not(windows))]
        {
            let _ = on_change;
            Err(super::unsupported())
        }
    }

    /// False after notification setup, rearming, or waiting fails. The caller
    /// should keep polling and may recreate the watch on a later backstop.
    pub fn is_alive(&self) -> bool {
        #[cfg(windows)]
        {
            self.running.is_alive()
        }
        #[cfg(not(windows))]
        {
            false
        }
    }
}

#[cfg(windows)]
mod win {
    use std::io;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc};
    use std::thread::JoinHandle;
    use std::time::Duration;
    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_FAILED, WAIT_OBJECT_0};
    use windows::Win32::System::Registry::{
        RegNotifyChangeKeyValue, HKEY, REG_NOTIFY_CHANGE_LAST_SET, REG_NOTIFY_CHANGE_NAME,
    };
    use windows::Win32::System::Threading::{
        CreateEventW, SetEvent, WaitForMultipleObjects, INFINITE,
    };
    use winreg::enums::{HKEY_CURRENT_USER, KEY_NOTIFY};
    use winreg::RegKey;

    // Watching the parent catches a microphone key created for the first time,
    // as well as nested NonPackaged values and key deletion/recreation.
    const CONSENT_STORE: &str =
        r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore";

    struct Event(HANDLE);

    // SAFETY: Windows event handles support concurrent SetEvent/wait operations.
    // Arc keeps the handle open until the owner and waiting worker are both done.
    unsafe impl Send for Event {}
    unsafe impl Sync for Event {}

    impl Event {
        fn new(manual_reset: bool) -> io::Result<Self> {
            // SAFETY: unnamed event, no borrowed security descriptor or name.
            unsafe { CreateEventW(None, manual_reset, false, None) }
                .map(Self)
                .map_err(io::Error::other)
        }

        fn handle(&self) -> HANDLE {
            self.0
        }

        fn signal(&self) -> io::Result<()> {
            // SAFETY: owned handle remains alive throughout the call.
            unsafe { SetEvent(self.0) }.map_err(io::Error::other)
        }
    }

    impl Drop for Event {
        fn drop(&mut self) {
            // SAFETY: this is the sole owning wrapper for this handle.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    struct Subscription {
        // Close the key first to cancel any pending registration while its
        // event still exists. Field order is deliberate.
        key: RegKey,
        changed: Event,
    }

    impl Subscription {
        fn open(path: &str) -> io::Result<Self> {
            let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(path, KEY_NOTIFY)?;
            let subscription = Self {
                key,
                changed: Event::new(false)?,
            };
            subscription.arm()?;
            Ok(subscription)
        }

        fn arm(&self) -> io::Result<()> {
            // SAFETY: the RegKey owns the HKEY; the auto-reset event and key
            // outlive the registration. Every arm is on the persistent worker,
            // and only after the previous registration has completed.
            let status = unsafe {
                RegNotifyChangeKeyValue(
                    HKEY(self.key.raw_handle()),
                    true,
                    REG_NOTIFY_CHANGE_NAME | REG_NOTIFY_CHANGE_LAST_SET,
                    Some(self.changed.handle()),
                    true,
                )
            };
            if status.0 == 0 {
                Ok(())
            } else {
                Err(io::Error::from_raw_os_error(status.0 as i32))
            }
        }
    }

    pub(super) struct Running {
        stop: Arc<Event>,
        alive: Arc<AtomicBool>,
        done: mpsc::Receiver<()>,
        thread: Option<JoinHandle<()>>,
    }

    impl Running {
        pub(super) fn start(on_change: impl FnMut() + Send + 'static) -> io::Result<Self> {
            Self::start_at(CONSENT_STORE.into(), on_change)
        }

        fn start_at(
            path: String,
            mut on_change: impl FnMut() + Send + 'static,
        ) -> io::Result<Self> {
            let stop = Arc::new(Event::new(true)?);
            let alive = Arc::new(AtomicBool::new(true));
            let worker_stop = stop.clone();
            let worker_alive = alive.clone();
            let (ready_tx, ready_rx) = mpsc::sync_channel(1);
            let (done_tx, done) = mpsc::channel::<()>();
            let thread = std::thread::Builder::new()
                .name("hark-meeting-registry".into())
                .spawn(move || {
                    let _done = done_tx;
                    let _alive = AliveGuard(worker_alive);
                    let subscription = match Subscription::open(&path) {
                        Ok(subscription) => subscription,
                        Err(error) => {
                            let _ = ready_tx.send(Err(error));
                            return;
                        }
                    };
                    if ready_tx.send(Ok(())).is_err() {
                        return;
                    }
                    if let Err(error) = watch(&subscription, &worker_stop, &mut on_change) {
                        log::warn!("meeting registry notifications stopped ({error}); polling remains available");
                    }
                })?;
            let running = Self {
                stop,
                alive,
                done,
                thread: Some(thread),
            };
            ready_rx
                .recv_timeout(Duration::from_secs(2))
                .map_err(io::Error::other)??;
            Ok(running)
        }

        pub(super) fn is_alive(&self) -> bool {
            self.alive.load(Ordering::Acquire)
        }
    }

    impl Drop for Running {
        fn drop(&mut self) {
            if let Err(error) = self.stop.signal() {
                log::warn!("meeting registry watcher could not be signalled to stop ({error})");
            }
            if self.done.recv_timeout(Duration::from_millis(750))
                == Err(mpsc::RecvTimeoutError::Timeout)
            {
                // The worker owns its handles and callback until it exits, so
                // a slow callback cannot make bounded app shutdown unsafe.
                log::warn!("meeting registry watcher still busy at shutdown; not waiting");
                return;
            }
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    struct AliveGuard(Arc<AtomicBool>);

    impl Drop for AliveGuard {
        fn drop(&mut self) {
            self.0.store(false, Ordering::Release);
        }
    }

    fn watch(
        subscription: &Subscription,
        stop: &Event,
        on_change: &mut impl FnMut(),
    ) -> io::Result<()> {
        // Shutdown is first: if both events are signalled, teardown wins.
        let handles = [stop.handle(), subscription.changed.handle()];
        loop {
            // SAFETY: both owned event handles remain open for this wait.
            let result = unsafe { WaitForMultipleObjects(&handles, false, INFINITE) };
            if result == WAIT_OBJECT_0 {
                return Ok(());
            }
            if result == WAIT_FAILED {
                return Err(io::Error::last_os_error());
            }
            if result.0 != WAIT_OBJECT_0.0 + 1 {
                return Err(io::Error::other(
                    "unexpected registry notification wait result",
                ));
            }
            // Auto-reset consumed the event. Re-arm before asking for a new
            // snapshot so registry writes during that snapshot wake us again.
            subscription.arm()?;
            on_change();
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::time::{Instant, SystemTime, UNIX_EPOCH};

        struct TestKey(String);

        impl TestKey {
            fn unique() -> Self {
                Self(format!(
                    r"Software\HarkMeetingProbeTests-{}-{}",
                    std::process::id(),
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                ))
            }
        }

        impl Drop for TestKey {
            fn drop(&mut self) {
                let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(&self.0);
            }
        }

        #[test]
        fn recursive_registry_changes_rearm_and_shutdown_drops_callback_sender() {
            let path = TestKey::unique();
            let (root, _) = RegKey::predef(HKEY_CURRENT_USER)
                .create_subkey(&path.0)
                .unwrap();
            let (nested, _) = root
                .create_subkey(r"microphone\NonPackaged\test-app")
                .unwrap();
            let (tx, rx) = mpsc::channel();
            let watcher = Running::start_at(path.0.clone(), move || {
                let _ = tx.send(());
            })
            .unwrap();

            // Each callback is sent after rearming, so a subsequent nested
            // write must produce another wakeup without a polling timer.
            nested.set_value("LastUsedTimeStop", &0_u64).unwrap();
            rx.recv_timeout(Duration::from_secs(2)).unwrap();
            nested.set_value("LastUsedTimeStop", &1_u64).unwrap();
            rx.recv_timeout(Duration::from_secs(2)).unwrap();
            drop(nested);
            root.delete_subkey_all("microphone").unwrap();
            rx.recv_timeout(Duration::from_secs(2)).unwrap();

            let started = Instant::now();
            drop(watcher);
            assert!(started.elapsed() < Duration::from_secs(2));
            // Shutdown must retire the worker's sender, even with no more
            // registry events. This is the coordinator shutdown invariant.
            while rx.try_recv().is_ok() {}
            assert_eq!(
                rx.recv_timeout(Duration::from_secs(1)),
                Err(mpsc::RecvTimeoutError::Disconnected)
            );
        }

        #[test]
        fn missing_watch_root_fails_without_creating_a_registry_key() {
            let path = TestKey::unique();
            assert!(Running::start_at(path.0.clone(), || {}).is_err());
            assert!(RegKey::predef(HKEY_CURRENT_USER)
                .open_subkey(&path.0)
                .is_err());
        }
    }
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;

    #[test]
    fn registry_notifications_are_explicitly_unsupported_off_windows() {
        match ChangeWatcher::start(|| panic!("unsupported watcher must not invoke its callback")) {
            Err(error) => assert_eq!(error.kind(), io::ErrorKind::Unsupported),
            Ok(_) => panic!("registry watcher unexpectedly started"),
        }
    }
}

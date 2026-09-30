//! Cancellation and injection admission shared by one pipeline run.
//! Stopping never takes a lock; only workers serialize clipboard transactions.
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Mutex;

const ACTIVE: u8 = 0;
const INJECTING: u8 = 1;
const CANCELLED: u8 = 2;
static INJECTION: Mutex<()> = Mutex::new(());

#[derive(Default)]
pub(crate) struct RunControl {
    state: AtomicU8,
    occupied: AtomicBool,
}

impl RunControl {
    pub fn cancel(&self) {
        self.state.store(CANCELLED, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.state.load(Ordering::Acquire) == CANCELLED
    }

    /// Reserve the entire hold/completion cycle at the input boundary.
    pub fn start_cycle(&self) -> bool {
        !self.is_cancelled()
            && self
                .occupied
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
    }

    pub fn finish_cycle(&self) {
        self.occupied.store(false, Ordering::Release);
    }

    /// The CAS is the start of injection: cancellation that wins first
    /// prevents any clipboard mutation. An admitted transaction must finish
    /// restoration, even if stop happens while it is pasting.
    pub fn inject<T>(&self, inject: impl FnOnce() -> T) -> Option<T> {
        let _serial = INJECTION
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        self.state
            .compare_exchange(ACTIVE, INJECTING, Ordering::AcqRel, Ordering::Acquire)
            .ok()?;
        let _reset = InjectionGuard(self);
        Some(inject())
    }
}

struct InjectionGuard<'a>(&'a RunControl);

impl Drop for InjectionGuard<'_> {
    fn drop(&mut self) {
        // Never revive a run cancelled while its clipboard was borrowed.
        let _ =
            self.0
                .state
                .compare_exchange(INJECTING, ACTIVE, Ordering::AcqRel, Ordering::Acquire);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retired_runs_cannot_begin_injection_or_another_cycle() {
        let control = RunControl::default();
        control.cancel();
        assert!(control
            .inject(|| panic!("retired injection executed"))
            .is_none());
        assert!(!control.start_cycle());
    }

    #[test]
    fn stop_during_injection_allows_restoration_without_reviving_the_run() {
        let control = RunControl::default();
        let result = control.inject(|| {
            control.cancel();
            "clipboard restored"
        });
        assert_eq!(result, Some("clipboard restored"));
        assert!(control.is_cancelled());
        assert!(control.inject(|| ()).is_none());
    }

    #[test]
    fn cancellation_prevents_injection_after_another_runs_transaction() {
        let previous = RunControl::default();
        let waiting = std::sync::Arc::new(RunControl::default());
        let worker = previous
            .inject(|| {
                let (ready, received) = std::sync::mpsc::channel();
                let run = waiting.clone();
                let worker = std::thread::spawn(move || {
                    ready.send(()).unwrap();
                    run.inject(|| panic!("cancelled waiter injected"))
                });
                received.recv().unwrap();
                waiting.cancel();
                worker
            })
            .unwrap();
        assert!(worker.join().unwrap().is_none());
    }
}

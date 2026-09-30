//! Change notification for the PipeWire probe. The worker only wakes the
//! coordinator; snapshots and detector decisions remain with their owner.
//!
//! One dedicated thread holds a PipeWire connection and listens for node
//! globals appearing or disappearing: an app opening the mic creates a
//! `Stream/Input/Audio` node, closing it removes one, so node churn is
//! exactly the mic-use change signal the Windows registry watch provides.
//! A slow backstop poll remains the coordinator's regardless (browser window
//! titles change under no PipeWire event).

use std::cell::RefCell;
use std::io;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::Duration;

/// The loop timer's period: bounds shutdown, and nothing else needs waking.
const TICK: Duration = Duration::from_millis(200);

/// A PipeWire graph watch, with explicit shutdown independent of the
/// callback's channel. Drop this before waiting for that channel to
/// disconnect.
pub struct ChangeWatcher {
    running: Running,
}

impl ChangeWatcher {
    /// Start observing graph changes. `on_change` must only enqueue a wakeup
    /// and return; graph contents never leave this worker. An unreachable
    /// session manager is an error so the caller keeps its polling backstop.
    pub fn start(on_change: impl FnMut() + Send + 'static) -> io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let alive = Arc::new(AtomicBool::new(true));
        let (ready_tx, ready_rx) = mpsc::sync_channel::<io::Result<()>>(1);
        let (done_tx, done) = mpsc::channel::<()>();

        let worker_stop = stop.clone();
        let worker_alive = alive.clone();
        let thread = std::thread::Builder::new()
            .name("hark-meeting-graph".into())
            .spawn(move || {
                let _done = done_tx;
                let _alive = AliveGuard(worker_alive);
                // The whole pipewire-rs stack is created here, inside the
                // thread that owns it: its Rc types are not Send, by design.
                let err = |what: &str, e: pipewire::Error| io::Error::other(format!("{what}: {e}"));
                let ready = (|| -> Result<
                    (
                        pipewire::main_loop::MainLoopRc,
                        pipewire::core::CoreRc,
                        pipewire::registry::RegistryRc,
                    ),
                    io::Error,
                > {
                    pipewire::init();
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
                    Ok((mainloop, core, registry))
                })();
                let (mainloop, core, registry) = match ready {
                    Ok(made) => made,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                // Both registry callbacks share one FnMut: they run on this
                // thread only, so an Rc<RefCell> is exact.
                let notify: Rc<RefCell<Box<dyn FnMut()>>> =
                    Rc::new(RefCell::new(Box::new(on_change)));
                let notify_add = notify.clone();
                let notify_remove = notify.clone();

                let _node_l = registry
                    .add_listener_local()
                    .global(move |obj| {
                        if obj.type_ == pipewire::types::ObjectType::Node {
                            (notify_add.borrow_mut())();
                        }
                    })
                    .global_remove(move |_id| (notify_remove.borrow_mut())())
                    .register();

                // A dead connection must retire the watcher: without this it
                // would report alive while never firing again.
                let error_quit = mainloop.clone();
                let _core_l = core
                    .add_listener_local()
                    .error(move |id, _seq, res, message| {
                        if id == 0 {
                            log::warn!(
                                "meeting graph watcher lost the connection: {res} {message}"
                            );
                            error_quit.quit();
                        }
                    })
                    .register();
                let _ = ready_tx.send(Ok(()));

                let quit = mainloop.clone();
                let timer = mainloop.loop_().add_timer(move |_| {
                    if worker_stop.load(Ordering::SeqCst) {
                        quit.quit();
                    }
                });
                timer.update_timer(Some(TICK), Some(TICK));
                mainloop.run();
                drop(_node_l);
                drop(_core_l);
            })
            .map_err(|e| io::Error::other(format!("cannot spawn the watcher thread: {e}")))?;
        ready_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(io::Error::other)??;
        Ok(Self {
            running: Running {
                stop,
                done,
                thread: Some(thread),
            },
        })
    }

    /// False after the connection failed or the worker exited. The caller
    /// should keep polling and may recreate the watch on a later backstop.
    pub fn is_alive(&self) -> bool {
        self.running.is_alive()
    }
}

struct Running {
    stop: Arc<AtomicBool>,
    done: mpsc::Receiver<()>,
    thread: Option<JoinHandle<()>>,
}

impl Running {
    fn is_alive(&self) -> bool {
        // The done channel disconnects exactly when the worker returns.
        self.done.try_recv() == Err(mpsc::TryRecvError::Empty)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if self.done.recv_timeout(TICK * 4) == Err(mpsc::RecvTimeoutError::Timeout) {
            // The worker owns its connection and callback until it exits, so
            // a wedged callback cannot make app shutdown hang.
            log::warn!("meeting graph watcher still busy at shutdown; not waiting");
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

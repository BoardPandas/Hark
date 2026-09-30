//! PipeWire system-audio loopback for meeting mode, the Linux "Them" channel.
//! Glue verifiable only against a live session manager; exercised by hand
//! with `examples/loopback_smoke.rs` (a private `PIPEWIRE_RUNTIME_DIR` stack
//! with a null sink is enough — see `packaging/LINUX.md`).
//!
//! One dedicated thread owns the entire `pipewire-rs` stack (loop, context,
//! core, registry, streams); every callback runs on it, mirroring the Windows
//! thread that owns its MTA apartment. The only per-packet work is a stack
//! copy and `Producer::push`. `pw::init` is refcounted process-wide and never
//! balanced with `deinit`: the detection probe and watcher in `hark-meeting`
//! keep the library initialized for the process lifetime anyway.
//!
//! Targeting, measured on PipeWire 1.6 + WirePlumber (2026-09-29):
//! - **ExcludeTree** captures the *monitor of the default sink*. Hark renders
//!   no audio, so "everything but Hark" is simply everything. A targetless
//!   `stream.capture.sink` capture errors out ("no target node available"),
//!   so the default sink's node name is resolved from the
//!   `default.audio.sink` metadata and passed as `target.object`; when that
//!   metadata changes mid-meeting the substream is rebuilt, counting one
//!   discontinuity for the gap.
//! - **IncludeTree** captures the meeting app's *output stream nodes*
//!   directly: a plain capture (no `stream.capture.sink`) with
//!   `target.object` set to the stream node's `object.serial` links to
//!   exactly that node — verified by tapping a stream routed to a
//!   non-default sink while another played into the default one. Nodes are
//!   matched by `/proc` ancestry to the root PID, and nodes that appear
//!   mid-meeting are attached as they show up. If the tree has no output
//!   node when enumeration completes, the capture falls back to the default
//!   sink, the same shape as the coordinator's Windows fallback, and says so
//!   in the log.
//!
//! The capture format is fixed in the EnumFormat POD at F32/16 kHz/mono;
//! PipeWire inserts an audioconvert adapter (the counterpart of Windows'
//! `AUTOCONVERTPCM`) and keeps delivering silence through gaps in the app's
//! playback, so the sample count stays the timeline.

use super::{LoopbackError, LoopbackHandle, LoopbackTarget, LOOPBACK_RATE};
use crate::ring::{ring, Consumer, Producer};
use pipewire as pw;
use pw::properties::properties;
use pw::spa::pod::Pod;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

/// The loop timer's period: bounds shutdown latency and paces substream
/// bookkeeping. Same shape as the Windows thread's 100 ms wait timeout.
const TICK: Duration = Duration::from_millis(100);

/// Samples pushed per `Producer::push` batch: small enough for the stack.
const PUSH_BATCH: usize = 480;

/// State shared with the handle, all atomics: the capture thread never holds
/// a lock the app side could contend on.
struct Shared {
    shutdown: Arc<AtomicBool>,
    stream_error: Arc<AtomicBool>,
    discontinuities: Arc<AtomicU64>,
    start_ns: Arc<AtomicU64>,
}

/// One `target.object` a substream captures.
#[derive(Clone)]
enum Source {
    /// `object.serial` of a `Stream/Output/Audio` node: that app stream.
    StreamNode(String),
    /// `node.name` of a sink, monitored via `stream.capture.sink`.
    SinkMonitor(String),
}

/// Everything the capture thread owns. Every pw callback receives the whole
/// thing behind `Rc<RefCell<..>>`; they all run on this thread, so the
/// borrows never overlap.
struct Thread {
    core: pw::core::CoreRc,
    producer: Rc<Producer>,
    shared: Shared,
    /// Live substreams by key (`stream:<serial>` / `monitor:default`), with
    /// their registered listeners: a dropped listener unregisters its
    /// callbacks, so the two live and die together.
    substreams: HashMap<String, (pw::stream::StreamRc, pw::stream::StreamListener<()>)>,
    /// Substreams to open, and keys gone from the graph. Callbacks queue;
    /// the timer tick applies: a pw callback must not mutate the very maps
    /// its own registration iterates.
    add: Vec<(String, Source)>,
    dead: Vec<String>,
    /// Sink node names seen in the registry, for the no-metadata fallback.
    sinks: Vec<(u32, String)>,
    /// The default sink's node name, from metadata.
    default_sink: Option<String>,
    /// Exclude mode: a monitor substream is wanted.
    monitor_wanted: bool,
    /// Include mode: the root PID whose tree's audio is "Them".
    include_root: Option<u32>,
    /// The initial registry enumeration has completed.
    enumerated: bool,
    /// Any substream reached Streaming at least once.
    ever_streamed: bool,
    /// The Include fallback to the whole sink has been applied.
    fell_back: bool,
    /// True once start() has been satisfied: the quit conditions then shift
    /// from "live or failed" to "shutdown or failed".
    running: bool,
    /// Set when capture can no longer continue.
    fatal: Option<String>,
    /// When this capture started, bounding the start phase: a stream that
    /// never links must not hang the meeting (the recorder waits on us).
    started: std::time::Instant,
    /// Registry node id -> object.serial, for retiring substreams when an
    /// app's output node disappears.
    node_ids: HashMap<u32, String>,
}

/// How long the start phase may take before giving up, mirroring the Windows
/// activation timeout's role.
const START_TIMEOUT: Duration = Duration::from_secs(5);

pub(super) fn start(
    target: LoopbackTarget,
    ring_seconds: u32,
) -> Result<(LoopbackHandle, Consumer), LoopbackError> {
    let shared = Shared {
        shutdown: Arc::new(AtomicBool::new(false)),
        stream_error: Arc::new(AtomicBool::new(false)),
        discontinuities: Arc::new(AtomicU64::new(0)),
        start_ns: Arc::new(AtomicU64::new(0)),
    };
    let handle_parts = (
        shared.shutdown.clone(),
        shared.stream_error.clone(),
        shared.discontinuities.clone(),
        shared.start_ns.clone(),
    );
    let (producer, consumer) = ring(ring_seconds as usize * LOOPBACK_RATE as usize);
    let (result_tx, result_rx) = mpsc::sync_channel::<Result<(), LoopbackError>>(1);
    let thread = std::thread::Builder::new()
        .name("hark-audio-loopback".to_string())
        .spawn(move || capture_thread(target, Rc::new(producer), shared, result_tx))
        .map_err(|e| LoopbackError::Start(format!("spawning the capture thread: {e}")))?;

    let (shutdown, stream_error, discontinuities, start_ns) = handle_parts;
    match result_rx.recv() {
        Ok(Ok(())) => Ok((
            LoopbackHandle {
                shutdown,
                stream_error,
                discontinuities,
                start_qpc_ns: start_ns,
                thread: Some(thread),
            },
            consumer,
        )),
        Ok(Err(e)) => {
            let _ = thread.join();
            Err(e)
        }
        Err(_) => Err(LoopbackError::ThreadDied),
    }
}

/// Context properties identifying every Hark stream to the rest of the graph
/// (pavucontrol shows them; the detection probe skips them by pid).
fn context_props() -> pw::properties::PropertiesBox {
    let mut props = properties! {
        *pw::keys::APP_NAME => "Hark",
        "application.process.id" => std::process::id().to_string(),
    };
    if let Some(name) = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().and_then(|n| n.to_str().map(str::to_string)))
    {
        props.insert(*pw::keys::APP_PROCESS_BINARY, name);
    }
    props
}

fn capture_thread(
    target: LoopbackTarget,
    producer: Rc<Producer>,
    shared: Shared,
    result_tx: mpsc::SyncSender<Result<(), LoopbackError>>,
) {
    pw::init();
    let failed = |what: &str, e: pw::Error| LoopbackError::Start(format!("{what}: {e}"));
    let mainloop = match pw::main_loop::MainLoopRc::new(None) {
        Ok(m) => m,
        Err(e) => {
            let _ = result_tx.send(Err(failed("cannot create the PipeWire loop", e)));
            return;
        }
    };
    let context = match pw::context::ContextRc::new(&mainloop, Some(context_props())) {
        Ok(c) => c,
        Err(e) => {
            let _ = result_tx.send(Err(failed("cannot create the PipeWire context", e)));
            return;
        }
    };
    let core = match context.connect_rc(None) {
        Ok(c) => c,
        Err(e) => {
            // The common failure: no session manager (a bare `pipewire`
            // daemon, an SSH session without a desktop). The recorder keeps
            // the microphone and the caller shows its notice.
            let _ = result_tx.send(Err(failed("cannot connect to PipeWire", e)));
            return;
        }
    };

    let state = Rc::new(RefCell::new(Thread {
        core: core.clone(),
        producer,
        shared,
        substreams: HashMap::new(),
        add: Vec::new(),
        dead: Vec::new(),
        sinks: Vec::new(),
        default_sink: None,
        monitor_wanted: matches!(target, LoopbackTarget::ExcludeTree(_)),
        include_root: match target {
            LoopbackTarget::IncludeTree(pid) => Some(pid),
            LoopbackTarget::ExcludeTree(_) => None,
        },
        enumerated: false,
        ever_streamed: false,
        fell_back: false,
        running: false,
        fatal: None,
        started: std::time::Instant::now(),
        node_ids: HashMap::new(),
    }));

    // Registry (nodes + sinks) and metadata (the default sink) listeners.
    // Their registrations outlive the enumeration below: they stay in this
    // scope until the thread ends.
    let registry = match core.get_registry_rc() {
        Ok(r) => r,
        Err(e) => {
            let _ = result_tx.send(Err(failed("cannot get the PipeWire registry", e)));
            return;
        }
    };
    let registry_weak = registry.downgrade();
    let held: HeldObjects = Rc::new(RefCell::new(Vec::new()));

    let reg_state = state.clone();
    let held_meta = held.clone();
    let removed_state = state.clone();
    let _reg_l = registry
        .add_listener_local()
        .global(move |obj| {
            let Some(reg) = registry_weak.upgrade() else {
                return;
            };
            let props = obj.props.as_ref();
            let get = |k: &str| props.and_then(|p| p.get(k)).unwrap_or("").to_string();
            match obj.type_ {
                pw::types::ObjectType::Node => {
                    let class = get("media.class");
                    let serial = get("object.serial");
                    if class == "Audio/Sink" && !get("node.name").is_empty() {
                        let name = get("node.name");
                        let mut s = reg_state.borrow_mut();
                        if !s.sinks.iter().any(|(_, n)| *n == name) {
                            s.sinks.push((obj.id, name));
                        }
                        return;
                    }
                    if class == "Stream/Output/Audio" && !serial.is_empty() {
                        let (root, wanted) = {
                            let s = reg_state.borrow();
                            (s.include_root, s.include_root.is_some())
                        };
                        // Registry globals filter `application.*`, so the
                        // owning pid only shows up in the bound node's info
                        // props — bind and decide there (Include mode only;
                        // Exclude never targets app nodes).
                        if wanted {
                            reg_state
                                .borrow_mut()
                                .node_ids
                                .insert(obj.id, serial.clone());
                            if let Ok(node) = reg.bind::<pw::node::Node, _>(obj) {
                                let node_state = reg_state.clone();
                                let node_serial = serial;
                                let l = node
                                    .add_listener_local()
                                    .info(move |info| {
                                        let props = info.props();
                                        let pid = props
                                            .and_then(|p| p.get("application.process.id"))
                                            .and_then(|v| v.parse::<u32>().ok());
                                        let Some((root, pid)) = root.zip(pid) else {
                                            return;
                                        };
                                        if !is_descendant(pid, root) {
                                            return;
                                        }
                                        let mut s = node_state.borrow_mut();
                                        let key = format!("stream:{node_serial}");
                                        let queued = s.add.iter().any(|(k, _)| *k == key)
                                            || s.substreams.contains_key(&key);
                                        if !queued {
                                            s.add.push((
                                                key,
                                                Source::StreamNode(node_serial.clone()),
                                            ));
                                        }
                                    })
                                    .register();
                                held_meta.borrow_mut().push((Box::new(node), Box::new(l)));
                            }
                        }
                    }
                }
                pw::types::ObjectType::Metadata => {
                    let Ok(m) = reg.bind::<pw::metadata::Metadata, _>(obj) else {
                        return;
                    };
                    let meta_state = reg_state.clone();
                    let l = m
                        .add_listener_local()
                        .property(move |_subject, key, _t, value| {
                            if key != Some("default.audio.sink") {
                                return 0;
                            }
                            let Some(name) = value.and_then(default_sink_name) else {
                                return 0;
                            };
                            let mut s = meta_state.borrow_mut();
                            let changed = s.default_sink.as_deref() != Some(name);
                            s.default_sink = Some(name.to_string());
                            if changed && s.monitor_wanted {
                                // Rebuild a LIVE monitor on the next tick; the
                                // swap's gap is one discontinuity, like a
                                // device change. On the first arrival there is
                                // nothing to drop — and a dead entry would
                                // otherwise retire the substream this very
                                // tick opens.
                                if s.substreams.contains_key("monitor:default") {
                                    s.shared.discontinuities.fetch_add(1, Ordering::Relaxed);
                                    s.dead.push("monitor:default".into());
                                }
                                s.add.push((
                                    "monitor:default".into(),
                                    Source::SinkMonitor(name.to_string()),
                                ));
                            }
                            0
                        })
                        .register();
                    held_meta.borrow_mut().push((Box::new(m), Box::new(l)));
                }
                _ => {}
            }
        })
        .global_remove(move |id| {
            // An app's output node is gone: retire its substream by the
            // serial remembered at bind time.
            let serial = removed_state.borrow_mut().node_ids.remove(&id);
            if let Some(serial) = serial {
                removed_state
                    .borrow_mut()
                    .dead
                    .push(format!("stream:{serial}"));
            }
        })
        .register();

    // Two roundtrips: the first drains the registry enumeration, the second
    // flushes the metadata property events the binds above queued (measured:
    // one roundtrip is not enough for those).
    if let Err(e) = roundtrip(&mainloop, &core) {
        let _ = result_tx.send(Err(failed("PipeWire roundtrip", e)));
        return;
    }
    if let Err(e) = roundtrip(&mainloop, &core) {
        let _ = result_tx.send(Err(failed("PipeWire roundtrip", e)));
        return;
    }
    state.borrow_mut().enumerated = true;

    let quit = mainloop.clone();
    let tick_state = state.clone();
    let timer = mainloop.loop_().add_timer(move |_| {
        if tick(&tick_state) {
            quit.quit();
        }
    });
    timer.update_timer(Some(TICK), Some(TICK));

    // Phase 1: run until a substream streams, or start fails.
    mainloop.run();
    {
        let mut s = state.borrow_mut();
        if let Some(detail) = s.fatal.take() {
            let _ = result_tx.send(Err(LoopbackError::Start(detail)));
            return;
        }
        if !s.ever_streamed {
            let _ = result_tx.send(Err(LoopbackError::Start(
                "shut down before any audio streamed".into(),
            )));
            return;
        }
        s.running = true;
    }
    log::info!("loopback open: {target:?}, {LOOPBACK_RATE} Hz mono f32 (PipeWire)");
    let _ = result_tx.send(Ok(()));

    // Phase 2: run until shutdown or a fatal stream error.
    mainloop.run();
    let mut s = state.borrow_mut();
    if let Some(detail) = s.fatal.take() {
        s.shared.stream_error.store(true, Ordering::Relaxed);
        log::error!("loopback stream error: {detail}");
    }
}

fn roundtrip(
    mainloop: &pw::main_loop::MainLoopRc,
    core: &pw::core::CoreRc,
) -> Result<(), pw::Error> {
    let pending = core.sync(0)?;
    let ml = mainloop.clone();
    let _l = core
        .add_listener_local()
        .done(move |id, seq| {
            if id == pw::core::PW_ID_CORE && seq == pending {
                ml.quit();
            }
        })
        .register();
    mainloop.run();
    Ok(())
}

/// The periodic bookkeeping pass. Returns true when the loop should stop.
/// Every access is a short borrow: `open_substream` re-enters the state from
/// its own callbacks' `Rc`, and a held borrow here would panic there.
fn tick(state: &Rc<RefCell<Thread>>) -> bool {
    if state.borrow().shared.shutdown.load(Ordering::Relaxed) {
        return true;
    }
    // Hoisted out of the loop head: a `borrow_mut()` temporary in a `for`
    // scrutinee lives for the whole loop and every borrow inside would panic.
    let adds = std::mem::take(&mut state.borrow_mut().add);
    for (key, source) in adds {
        if state.borrow().substreams.contains_key(&key) {
            continue;
        }
        match open_substream(state, &key, source) {
            Ok(opened) => {
                state.borrow_mut().substreams.insert(key, opened);
            }
            Err(e) => log::warn!("loopback: cannot open {key}: {e}"),
        }
    }
    let dead = std::mem::take(&mut state.borrow_mut().dead);
    {
        let mut s = state.borrow_mut();
        for key in dead {
            s.substreams.remove(&key);
        }
    }

    // No metadata (very old session managers): monitor any sink we saw.
    {
        let mut s = state.borrow_mut();
        if s.monitor_wanted
            && s.default_sink.is_none()
            && s.enumerated
            && !s.substreams.contains_key("monitor:default")
            && !s.sinks.is_empty()
            && !s.add.iter().any(|(k, _)| k == "monitor:default")
        {
            let name = s
                .sinks
                .iter()
                .min_by_key(|(id, _)| *id)
                .map(|(_, n)| n.clone())
                .expect("checked non-empty");
            s.add
                .push(("monitor:default".into(), Source::SinkMonitor(name)));
        }
        // The Include fallback: the app's tree had no audio, capture the sink.
        if s.include_root.is_some()
            && s.enumerated
            && !s.fell_back
            && !s.ever_streamed
            && s.substreams.is_empty()
            && s.add.is_empty()
        {
            log::info!(
                "loopback: no audio stream for the meeting app's process tree; \
                 capturing the default sink instead"
            );
            s.fell_back = true;
            s.monitor_wanted = true;
        }
        // The monitor died mid-meeting (sink unplugged): metadata has not
        // changed, so rebuild it ourselves, counting the gap.
        if s.monitor_wanted
            && s.running
            && s.ever_streamed
            && !s.substreams.contains_key("monitor:default")
            && !s.add.iter().any(|(k, _)| k == "monitor:default")
        {
            if let Some(name) = s.default_sink.clone() {
                s.shared.discontinuities.fetch_add(1, Ordering::Relaxed);
                s.add
                    .push(("monitor:default".into(), Source::SinkMonitor(name)));
            }
        }
    }

    // The start phase completes with the first streaming substream: the
    // phase-1 loop quits so `start` can unblock and phase 2 begins.
    {
        let s = state.borrow();
        if !s.running && s.ever_streamed {
            return true;
        }
    }
    // The start phase has run out of time: a stream that never links must
    // not hang the meeting.
    if !state.borrow().running
        && state.borrow().started.elapsed() > START_TIMEOUT
        && !state.borrow().ever_streamed
    {
        state.borrow_mut().fatal = Some("no system audio could be opened in time".to_string());
        return true;
    }
    // Start phase over, nothing streaming and nothing left to try; or the
    // Include tree's every stream is gone.
    let (starting_dead, tree_dead) = {
        let s = state.borrow();
        (
            !s.running
                && s.enumerated
                && s.substreams.is_empty()
                && s.add.is_empty()
                && !s.ever_streamed,
            s.running && s.include_root.is_some() && s.substreams.is_empty(),
        )
    };
    if starting_dead || tree_dead {
        state.borrow_mut().fatal = Some(if state.borrow().include_root.is_some() {
            "the meeting app's audio streams are gone".into()
        } else {
            "no audio output to capture".into()
        });
        return true;
    }
    false
}

/// Create and connect one capture substream for `source`. Marks the thread
/// state streaming on its first Streaming transition and queues the key as
/// dead on an error, both from the callback (same thread, short borrows).
fn open_substream(
    state: &Rc<RefCell<Thread>>,
    key: &str,
    source: Source,
) -> Result<(pw::stream::StreamRc, pw::stream::StreamListener<()>), pw::Error> {
    let (core, producer, start_ns) = {
        let s = state.borrow();
        (
            s.core.clone(),
            s.producer.clone(),
            s.shared.start_ns.clone(),
        )
    };
    let mut props = properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "Music",
    };
    match &source {
        Source::StreamNode(serial) => {
            // Plain capture: `target.object` on a stream node taps exactly
            // that node's audio (measured; see the module docs).
            props.insert(*pw::keys::TARGET_OBJECT, serial.clone());
        }
        Source::SinkMonitor(sink) => {
            props.insert(*pw::keys::STREAM_CAPTURE_SINK, "true".to_string());
            props.insert(*pw::keys::TARGET_OBJECT, sink.clone());
        }
    }
    let stream = pw::stream::StreamRc::new(core, "hark-loopback", props)?;

    let on_state = state.clone();
    let on_error = state.clone();
    let error_key = key.to_string();
    let producer = producer.clone();
    let start_ns = start_ns.clone();
    let bytes = format_pod();
    let mut params = [Pod::from_bytes(&bytes).expect("fixed POD serializes")];
    let listener = stream
        .add_local_listener_with_user_data(())
        .state_changed(move |_, _, old, new| {
            if old != new {
                log::info!("loopback substream {error_key}: {old:?} -> {new:?}");
            }
            match new {
                pw::stream::StreamState::Streaming => {
                    on_state.borrow_mut().ever_streamed = true;
                }
                pw::stream::StreamState::Error(ref e) => {
                    log::warn!("loopback substream {error_key} failed: {e}");
                    on_error.borrow_mut().dead.push(error_key.clone());
                }
                _ => {}
            }
        })
        .process(move |stream, _| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let datas = buffer.datas_mut();
            let Some(data) = datas.first_mut() else {
                return;
            };
            let size = data.chunk().size() as usize;
            let Some(bytes) = data.data() else {
                return;
            };
            // F32LE mono at LOOPBACK_RATE is the negotiated format, so the
            // byte slice is the sample slice.
            let samples =
                unsafe { std::slice::from_raw_parts(bytes.as_ptr() as *const f32, size / 4) };
            if start_ns.load(Ordering::Relaxed) == 0 {
                start_ns.store(monotonic_ns().max(1), Ordering::Release);
            }
            let mut batch = [0f32; PUSH_BATCH];
            for part in samples.chunks(PUSH_BATCH) {
                batch[..part.len()].copy_from_slice(part);
                producer.push(&batch[..part.len()]);
            }
        })
        .register()?;
    stream.connect(
        pw::spa::utils::Direction::Input,
        None,
        pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
        &mut params,
    )?;
    Ok((stream, listener))
}

/// Nanoseconds on `CLOCK_MONOTONIC`, the clock `Instant` is built from.
fn monotonic_ns() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: writes into a local timespec owned by this call.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &raw mut ts) };
    (ts.tv_sec as u64) * 1_000_000_000 + ts.tv_nsec as u64
}

/// The EnumFormat POD: F32LE, 16 kHz, mono.
fn format_pod() -> Vec<u8> {
    let mut info = pw::spa::param::audio::AudioInfoRaw::new();
    info.set_format(pw::spa::param::audio::AudioFormat::F32LE);
    info.set_rate(LOOPBACK_RATE);
    info.set_channels(1);
    let mut position = [0; pw::spa::param::audio::MAX_CHANNELS];
    position[0] = pw::spa::sys::SPA_AUDIO_CHANNEL_MONO;
    info.set_position(position);
    let obj = pw::spa::pod::Object {
        type_: pw::spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: pw::spa::param::ParamType::EnumFormat.as_raw(),
        properties: info.into(),
    };
    pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(obj),
    )
    .expect("serializing a fixed POD cannot fail")
    .0
    .into_inner()
}

/// `ppid` from `/proc/<pid>/stat`, or `None` if the process vanished. Field
/// 4, after the parenthesised comm that may itself contain spaces.
fn parent_of(pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after = stat.rsplit(')').next()?;
    after.split_whitespace().nth(1)?.parse().ok()
}

/// Does `pid`'s ancestry reach `root`? A dead intermediate parent breaks the
/// chain (the orphan was reparented), which is the same limitation the
/// Windows process tree has.
fn is_descendant(pid: u32, root: u32) -> bool {
    if pid == root {
        return true;
    }
    let mut cursor = pid;
    // Bounded by the pid space, not by trust in /proc consistency.
    for _ in 0..4_194_304 {
        match parent_of(cursor) {
            Some(p) if p == root => return true,
            Some(p) if p != cursor => cursor = p,
            _ => return false,
        }
    }
    false
}

/// Bound registry objects (metadata, nodes) with their listeners: both must
/// outlive the capture, and they die together.
type HeldObjects = Rc<RefCell<Vec<(Box<dyn pw::proxy::ProxyT>, Box<dyn pw::proxy::Listener>)>>>;

/// The `default.audio.sink` metadata value: PipeWire < 0.3.77 stores the
/// bare node name, newer stores `{"name":"alsa_output...","device":...}`.
pub(super) fn default_sink_name(value: &str) -> Option<&str> {
    let value = value.trim();
    if !value.starts_with('{') {
        return (!value.is_empty()).then_some(value);
    }
    // The one field needed from JSON PipeWire itself wrote; node names
    // cannot contain quotes or escapes, so a targeted scan beats a parser.
    let key = value.find("\"name\"")?;
    let rest = &value[key + 6..];
    let colon = rest.find(':')?;
    let rest = rest[colon + 1..].trim_start();
    let quoted = rest.strip_prefix('"')?;
    let end = quoted.find('"')?;
    let name = &quoted[..end];
    (!name.is_empty()).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_sink_name_survives_both_metadata_shapes() {
        assert_eq!(
            default_sink_name(r#"{"name":"alsa_output.pci-0000_00_1f.3","device":42}"#),
            Some("alsa_output.pci-0000_00_1f.3")
        );
        assert_eq!(
            default_sink_name("alsa_output.pci-0000_00_1f.3"),
            Some("alsa_output.pci-0000_00_1f.3")
        );
        assert_eq!(default_sink_name(""), None);
        assert_eq!(default_sink_name(r#"{"other":1}"#), None);
        assert_eq!(default_sink_name(r#"{"name":""}"#), None);
    }

    #[test]
    fn a_process_is_its_own_tree_root_but_init_is_not_our_child() {
        let me = std::process::id();
        assert!(is_descendant(me, me));
        if me != 1 {
            assert!(!is_descendant(1, me));
        }
    }

    #[test]
    fn the_monotonic_clock_reads_sanity() {
        let a = monotonic_ns();
        std::thread::sleep(Duration::from_millis(2));
        let b = monotonic_ns();
        assert!(b > a, "monotonic time must advance ({a} -> {b})");
    }
}

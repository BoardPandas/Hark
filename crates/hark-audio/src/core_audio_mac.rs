//! Core Audio tap bridge. The Objective-C owner negotiates a mono f32 stream;
//! its real-time callback only copies samples into the preallocated ring.
//! Conversion to 16 kHz stays in the meeting drain, off the audio thread.
use crate::loopback::{LoopbackError, LoopbackHandle, LoopbackTarget};
use crate::ring::{ring, Consumer, Producer};
use std::ffi::{c_char, c_void, CStr};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

type SampleCallback = unsafe extern "C" fn(*mut c_void, *const f32, u32, f64, u64);
type ProcessCallback = unsafe extern "C" fn(*mut c_void, u32, u32, *const c_char, bool);
type WindowCallback = unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char);
extern "C" {
    fn hark_mac_audio_supported() -> bool;
    fn hark_mac_tap_open(root: u32, exclude: bool, rate: *mut u32, error: *mut i32) -> *mut c_void;
    fn hark_mac_tap_start(tap: *mut c_void, callback: SampleCallback, context: *mut c_void) -> i32;
    fn hark_mac_tap_refresh(tap: *mut c_void) -> i32;
    fn hark_mac_tap_close(tap: *mut c_void);
    fn hark_mac_audio_snapshot(
        context: *mut c_void,
        process: ProcessCallback,
        window: Option<WindowCallback>,
    ) -> i32;
}

pub fn supported() -> bool {
    // SAFETY: availability query has no arguments or side effects.
    unsafe { hark_mac_audio_supported() }
}

struct Tap(*mut c_void);
impl Drop for Tap {
    fn drop(&mut self) {
        // SAFETY: this owner closes once, on the capture thread; native stop
        // waits for all IO callbacks before returning.
        unsafe { hark_mac_tap_close(self.0) };
    }
}
struct Callback {
    producer: Producer,
    error: Arc<AtomicBool>,
    gaps: Arc<AtomicU64>,
    timestamp: Arc<AtomicU64>,
    packets: Arc<AtomicU64>,
    next_sample: Option<f64>,
    frames: u64,
    rate: u32,
}

unsafe extern "C" fn samples(
    context: *mut c_void,
    data: *const f32,
    count: u32,
    sample: f64,
    ns: u64,
) {
    // SAFETY: pinned Box outlives Tap; Core Audio invokes this callback serially.
    let context = unsafe { &mut *context.cast::<Callback>() };
    if count == 0 || context.error.load(Ordering::Relaxed) {
        return;
    }
    if sample >= 0.0 {
        if let Some(next) = context.next_sample {
            let missing = (sample - next).round();
            if missing != 0.0 {
                context.gaps.fetch_add(1, Ordering::Relaxed);
                // A clock reset or very large jump cannot be repaired in an
                // IO callback. Stop cleanly rather than misaligning channels.
                if missing < 0.0 || missing > f64::from(context.rate) * 2.0 {
                    context.error.store(true, Ordering::Relaxed);
                    return;
                }
                push_silence(&context.producer, missing as usize);
                context.frames += missing as u64;
            }
        }
        context.next_sample = Some(sample + f64::from(count));
    }
    if ns != 0 && context.timestamp.load(Ordering::Relaxed) == 0 {
        // The first valid timestamp may arrive after earlier packets. It
        // still describes ring sample zero, including any repaired gaps.
        let elapsed = u128::from(context.frames) * 1_000_000_000 / u128::from(context.rate);
        let origin = ns.saturating_sub(elapsed as u64);
        let _ = context
            .timestamp
            .compare_exchange(0, origin, Ordering::Release, Ordering::Relaxed);
    }
    if data.is_null() {
        push_silence(&context.producer, count as usize);
    } else {
        // SAFETY: Core Audio buffer is count mono f32 samples, valid for this callback.
        context
            .producer
            .push(unsafe { std::slice::from_raw_parts(data, count as usize) });
    }
    context.frames += u64::from(count);
    context.packets.fetch_add(1, Ordering::Relaxed);
}
fn push_silence(producer: &Producer, mut count: usize) {
    const ZERO: [f32; 512] = [0.0; 512];
    while count > 0 {
        let batch = count.min(ZERO.len());
        producer.push(&ZERO[..batch]);
        count -= batch;
    }
}
fn capture_error(status: i32) -> LoopbackError {
    LoopbackError::Start(format!(
        "Core Audio returned {status}. Allow Hark in System Settings > Privacy & Security > Screen & System Audio Recording, then retry"
    ))
}

pub(crate) fn start(
    target: LoopbackTarget,
    ring_seconds: u32,
) -> Result<(LoopbackHandle, Consumer), LoopbackError> {
    if !supported() {
        return Err(LoopbackError::Activate(
            "Meeting audio requires macOS 14.2 or later".into(),
        ));
    }
    if ring_seconds == 0 || ring_seconds > 600 {
        return Err(LoopbackError::Start(
            "invalid loopback ring duration".into(),
        ));
    }
    let shutdown = Arc::new(AtomicBool::new(false));
    let stream_error = Arc::new(AtomicBool::new(false));
    let discontinuities = Arc::new(AtomicU64::new(0));
    let start_qpc_ns = Arc::new(AtomicU64::new(0));
    let (stop, error, gaps, timestamp) = (
        shutdown.clone(),
        stream_error.clone(),
        discontinuities.clone(),
        start_qpc_ns.clone(),
    );
    let (tx, rx) = mpsc::sync_channel(1);
    let thread = std::thread::Builder::new().name("hark-audio-loopback".into()).spawn(move || {
        let (root, exclude) = match target { LoopbackTarget::IncludeTree(p) => (p, false), LoopbackTarget::ExcludeTree(p) => (p, true) };
        let mut rate = 0;
        let mut status = 0;
        // SAFETY: native code writes only to the provided out parameters.
        let raw = unsafe { hark_mac_tap_open(root, exclude, &mut rate, &mut status) };
        if raw.is_null() { let _ = tx.send(Err(capture_error(status))); return; }
        // Declare the callback before the native owner: stop callbacks before
        // releasing their context, including every early-return path.
        let (producer, consumer) = ring(rate as usize * ring_seconds as usize);
        let packets = Arc::new(AtomicU64::new(0));
        let mut callback = Box::new(Callback { producer, error: error.clone(), gaps, timestamp, packets: packets.clone(), next_sample: None, frames: 0, rate });
        let tap = Tap(raw);
        // SAFETY: callback remains pinned until after tap has stopped.
        status = unsafe { hark_mac_tap_start(tap.0, samples, (&mut *callback as *mut Callback).cast()) };
        if status != 0 { let _ = tx.send(Err(capture_error(status))); return; }
        if tx.send(Ok((rate, consumer))).is_err() { return; }
        let mut last_packets = 0;
        let mut last_delivery = Instant::now();
        let mut refresh = Instant::now();
        while !stop.load(Ordering::Relaxed) && !error.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(100));
            let delivered = packets.load(Ordering::Relaxed);
            if delivered != last_packets { last_delivery = Instant::now(); last_packets = delivered; }
            if last_delivery.elapsed() > Duration::from_secs(5) {
                error.store(true, Ordering::Relaxed);
                log::warn!("Core Audio meeting capture stalled; check system audio recording permission");
                break;
            }
            if refresh.elapsed() >= Duration::from_secs(1) {
                // SAFETY: called by the native owner's sole thread.
                let status = unsafe { hark_mac_tap_refresh(tap.0) };
                if status != 0 { error.store(true, Ordering::Relaxed); log::warn!("Core Audio meeting capture changed or failed ({status})"); }
                refresh = Instant::now();
            }
        }
        drop(tap);
    }).map_err(|e| LoopbackError::Start(e.to_string()))?;
    match rx.recv() {
        Ok(Ok((sample_rate, consumer))) => Ok((
            LoopbackHandle {
                sample_rate,
                shutdown,
                stream_error,
                discontinuities,
                start_qpc_ns,
                thread: Some(thread),
            },
            consumer,
        )),
        Ok(Err(e)) => {
            let _ = thread.join();
            Err(e)
        }
        Err(_) => {
            let _ = thread.join();
            Err(LoopbackError::ThreadDied)
        }
    }
}

/// Native process facts for meeting detection. `input_running` is Core Audio's
/// active-input flag, not a guess based on an app being open.
pub struct AudioProcess {
    pub pid: u32,
    pub parent: u32,
    pub app_id: String,
    pub input_running: bool,
}
pub fn process_snapshot(
    mut meeting_title: impl FnMut(&str, &str),
) -> std::io::Result<Vec<AudioProcess>> {
    struct Context<'a> {
        processes: Vec<AudioProcess>,
        title: &'a mut dyn FnMut(&str, &str),
    }
    unsafe extern "C" fn process(
        context: *mut c_void,
        pid: u32,
        parent: u32,
        id: *const c_char,
        input_running: bool,
    ) {
        // SAFETY: synchronous callback; both pointers are valid throughout it.
        let context = unsafe { &mut *context.cast::<Context<'_>>() };
        let app_id = unsafe { CStr::from_ptr(id) }.to_string_lossy().into_owned();
        context.processes.push(AudioProcess {
            pid,
            parent,
            app_id,
            input_running,
        });
    }
    unsafe extern "C" fn window(context: *mut c_void, id: *const c_char, title: *const c_char) {
        // SAFETY: borrowed C strings are valid for this synchronous callback.
        let context = unsafe { &mut *context.cast::<Context<'_>>() };
        let id = unsafe { CStr::from_ptr(id) }.to_string_lossy();
        let title = unsafe { CStr::from_ptr(title) }.to_string_lossy();
        (context.title)(&id, &title);
    }
    if !supported() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "meeting detection requires macOS 14.2 or later",
        ));
    }
    let mut context = Context {
        processes: Vec::new(),
        title: &mut meeting_title,
    };
    // SAFETY: native callback visits synchronously and retains no references.
    let status = unsafe {
        hark_mac_audio_snapshot(
            (&mut context as *mut Context<'_>).cast(),
            process,
            Some(window),
        )
    };
    if status != 0 {
        return Err(std::io::Error::other(format!(
            "Core Audio process snapshot failed ({status})"
        )));
    }
    Ok(context.processes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn callback_preserves_silence_and_sample_time_gaps() {
        let (producer, consumer) = ring(32);
        let mut callback = Callback {
            producer,
            error: Arc::new(AtomicBool::new(false)),
            gaps: Arc::new(AtomicU64::new(0)),
            timestamp: Arc::new(AtomicU64::new(0)),
            packets: Arc::new(AtomicU64::new(0)),
            next_sample: None,
            frames: 0,
            rate: 48000,
        };
        let data = [0.25, 0.5];
        let pointer = (&mut callback as *mut Callback).cast();
        unsafe {
            samples(pointer, data.as_ptr(), 2, 100.0, 1234);
            samples(pointer, std::ptr::null(), 2, 105.0, 4567);
        }
        assert_eq!(
            consumer.read_range(0, 7).unwrap(),
            vec![0.25, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0]
        );
        assert_eq!(callback.gaps.load(Ordering::Relaxed), 1);
        assert_eq!(callback.timestamp.load(Ordering::Relaxed), 1234);
        unsafe {
            samples(pointer, data.as_ptr(), 2, 50.0, 7890);
        }
        assert!(callback.error.load(Ordering::Relaxed));
        assert_eq!(consumer.total_written(), 7);
        // A latched error cannot resume writing before the owner stops IO.
        unsafe {
            samples(pointer, data.as_ptr(), 2, 107.0, 9999);
        }
        assert_eq!(consumer.total_written(), 7);
    }

    #[test]
    fn delayed_host_timestamp_still_refers_to_ring_sample_zero() {
        let (producer, consumer) = ring(32);
        let mut callback = Callback {
            producer,
            error: Arc::new(AtomicBool::new(false)),
            gaps: Arc::new(AtomicU64::new(0)),
            timestamp: Arc::new(AtomicU64::new(0)),
            packets: Arc::new(AtomicU64::new(0)),
            next_sample: None,
            frames: 0,
            rate: 48000,
        };
        let pointer = (&mut callback as *mut Callback).cast();
        unsafe {
            samples(pointer, std::ptr::null(), 2, 100.0, 0);
            samples(pointer, std::ptr::null(), 2, 102.0, 1_000_000);
        }
        assert_eq!(consumer.total_written(), 4);
        assert_eq!(callback.timestamp.load(Ordering::Relaxed), 958_334);
    }
}

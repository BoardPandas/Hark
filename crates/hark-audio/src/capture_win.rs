//! Live cpal capture. I/O glue: verifiable only on real hardware with a real
//! microphone (run-on-real-HW). All decision logic lives in the pure modules.
//!
//! The stream is built and owned by a dedicated thread so WASAPI's COM
//! apartment is initialized by exactly one owner: sharing a thread with
//! egui/winit or the keyboard hook risks `RPC_E_CHANGED_MODE` when the other
//! occupant initializes COM in a different mode first. The cpal input
//! callback only calls `Producer::push_interleaved` (relaxed atomic stores;
//! no allocation, no locks, no syscalls) per the cpal #970 gotcha.

use crate::level::LevelMeter;
use crate::ring::{ring, Consumer, Producer};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CaptureError {
    #[error("no usable microphone: no default input device")]
    NoDevice,
    #[error("no f32 input config available on the default device (formats offered: {offered})")]
    NoF32Config { offered: String },
    #[error("cannot query input device configs: {0}")]
    QueryConfig(String),
    #[error("cannot build the input stream: {0}")]
    BuildStream(String),
    #[error("cannot start the input stream: {0}")]
    Play(String),
    #[error("capture thread exited before reporting a stream")]
    ThreadDied,
}

/// A running capture. The ring `Consumer` is handed out by value at start
/// (it moves to the pipeline worker); this handle keeps the stream alive.
/// Dropping it stops the stream (its owning thread exits and the stream
/// drops with it).
pub struct CaptureHandle {
    sample_rate: u32,
    stream_error: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    level: Arc<LevelMeter>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl CaptureHandle {
    /// The device rate samples arrive at (the ring's rate). Downstream
    /// resamples to 16 kHz per clip.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// The live input-level meter, updated from the capture callback. Cheap
    /// to clone (an `Arc`); the UI reads it every frame to drive the
    /// recording overlay's audio-reactive pulse.
    pub fn level_meter(&self) -> Arc<LevelMeter> {
        self.level.clone()
    }

    /// True once the stream has reported an error (device unplugged, etc.).
    /// Phase 5 adds live recovery; Phase 1 surfaces it.
    pub fn stream_errored(&self) -> bool {
        self.stream_error.load(Ordering::Relaxed)
    }
}

impl Drop for CaptureHandle {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            t.thread().unpark();
            let _ = t.join();
        }
    }
}

/// Enumerate the names of the available input devices. Runs the WASAPI query
/// on a dedicated thread so COM is initialized on a thread we own: the UI
/// thread inits COM as STA (winit) and cpal wants MTA, which would collide
/// with `RPC_E_CHANGED_MODE` if we queried inline. The thread inits its own
/// apartment, does the query, and exits clean. Errors (or no host) yield an
/// empty list; the caller treats an empty list as "system default only".
pub fn list_input_devices() -> Vec<String> {
    std::thread::Builder::new()
        .name("hark-audio-enumerate".to_string())
        .spawn(enumerate_input_devices)
        .ok()
        .and_then(|t| t.join().ok())
        .unwrap_or_default()
}

/// The WASAPI query itself, run only on the dedicated enumeration thread.
/// The device name is its `Display` form (cpal `DeviceTrait: Display`).
fn enumerate_input_devices() -> Vec<String> {
    let host = cpal::default_host();
    let Ok(devices) = host.input_devices() else {
        return Vec::new();
    };
    devices.map(|d| d.to_string()).collect()
}

/// The name of the Windows **Default Communications Device** for capture, if
/// there is one. `None` on other platforms and on any failure.
///
/// Windows keeps two capture defaults, and cpal only ever asks for one of
/// them: `default_input_device()` resolves the `eConsole` role, while
/// communications apps (Teams, Zoom, Discord) ask for `eCommunications`. A
/// user who followed any headset setup guide — they all say "set as Default
/// Communication Device" — has these pointing at different microphones, and
/// experiences that as Hark ignoring the microphone that demonstrably works
/// everywhere else. Surfacing the name lets the picker say which is which
/// instead of leaving the user to guess.
///
/// Runs on its own thread so this COM apartment is one we own, exactly as
/// [`list_input_devices`] does: the UI thread is STA (winit) and this needs
/// MTA, and mixing them yields `RPC_E_CHANGED_MODE`.
pub fn communications_default_device() -> Option<String> {
    std::thread::Builder::new()
        .name("hark-audio-comms-role".to_string())
        .spawn(query_communications_default)
        .ok()
        .and_then(|t| t.join().ok())
        .flatten()
}

#[cfg(windows)]
fn query_communications_default() -> Option<String> {
    use windows::core::Result as WinResult;
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
    use windows::Win32::Media::Audio::{
        eCapture, eCommunications, IMMDeviceEnumerator, MMDeviceEnumerator,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
        STGM_READ,
    };

    // SAFETY: every call below is a COM call on a thread that owns its own
    // apartment for the duration; nothing here outlives CoUninitialize.
    unsafe {
        if CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {
            return None;
        }
        let queried: WinResult<String> = (|| {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let device = enumerator.GetDefaultAudioEndpoint(eCapture, eCommunications)?;
            let store = device.OpenPropertyStore(STGM_READ)?;
            Ok(store.GetValue(&PKEY_Device_FriendlyName)?.to_string())
        })();
        CoUninitialize();
        match queried {
            Ok(name) => Some(name),
            Err(e) => {
                // No communications default set is a normal state, not a fault.
                log::debug!("no communications-role capture device: {e}");
                None
            }
        }
    }
}

#[cfg(not(windows))]
fn query_communications_default() -> Option<String> {
    None
}

/// Start continuous capture into a ring sized `ring_seconds * live device
/// rate` (the rate is only known once the stream config resolves, so sizing
/// is by duration, not sample count). `input_device` names a specific
/// microphone (a name from [`list_input_devices`]); `None`, or a name that no
/// longer matches any device, falls back to the OS default. Blocks until the
/// stream is live (or failed to build). Returns the handle plus the ring
/// `Consumer`, which moves to the pipeline worker.
pub fn start(
    ring_seconds: u32,
    input_device: Option<String>,
) -> Result<(CaptureHandle, Consumer), CaptureError> {
    let shutdown = Arc::new(AtomicBool::new(false));
    let stream_error = Arc::new(AtomicBool::new(false));
    let level = LevelMeter::new();
    let (result_tx, result_rx) = mpsc::sync_channel::<Result<(Consumer, u32), CaptureError>>(1);

    let thread_shutdown = shutdown.clone();
    let thread_error = stream_error.clone();
    let thread_level = level.clone();
    let thread = std::thread::Builder::new()
        .name("hark-audio-capture".to_string())
        .spawn(move || {
            capture_thread(
                ring_seconds,
                input_device,
                thread_error,
                thread_shutdown,
                thread_level,
                result_tx,
            );
        })
        .expect("spawning the capture thread cannot fail");

    match result_rx.recv() {
        Ok(Ok((consumer, sample_rate))) => Ok((
            CaptureHandle {
                sample_rate,
                stream_error,
                shutdown,
                level,
                thread: Some(thread),
            },
            consumer,
        )),
        Ok(Err(e)) => {
            let _ = thread.join();
            Err(e)
        }
        Err(_) => Err(CaptureError::ThreadDied),
    }
}

/// Body of the dedicated capture thread: build the stream, report the result,
/// then keep the stream alive until shutdown.
fn capture_thread(
    ring_seconds: u32,
    input_device: Option<String>,
    stream_error: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    level: Arc<LevelMeter>,
    result_tx: mpsc::SyncSender<Result<(Consumer, u32), CaptureError>>,
) {
    let built = build_stream(ring_seconds, input_device.as_deref(), stream_error, level);
    match built {
        Ok((stream, consumer, rate)) => {
            if let Err(e) = stream.play() {
                let _ = result_tx.send(Err(CaptureError::Play(e.to_string())));
                return;
            }
            let _ = result_tx.send(Ok((consumer, rate)));
            // The stream lives exactly as long as this loop: park until told
            // to shut down, then drop the stream by returning.
            while !shutdown.load(Ordering::Relaxed) {
                std::thread::park_timeout(Duration::from_millis(200));
            }
            drop(stream);
        }
        Err(e) => {
            let _ = result_tx.send(Err(e));
        }
    }
}

/// Resolve the input device: the named one when it is still present, else the
/// OS default. A configured name that no longer matches (mic unplugged, an
/// audio interface powered off) degrades to the default with a warning rather
/// than failing the whole pipeline: keeping dictation working beats a hard
/// stop over a device change.
fn select_device(
    host: &cpal::Host,
    input_device: Option<&str>,
) -> Result<cpal::Device, CaptureError> {
    if let Some(name) = input_device {
        match host.input_devices() {
            Ok(mut devices) => {
                // Match on the `Display` name, the same string the picker
                // stored (cpal `DeviceTrait: Display`).
                if let Some(device) = devices.find(|d| d.to_string() == name) {
                    return Ok(device);
                }
                log::warn!(
                    "configured input device {name:?} not found; falling back to the default"
                );
            }
            Err(e) => log::warn!(
                "cannot enumerate input devices ({e}); falling back to the default for {name:?}"
            ),
        }
    }
    host.default_input_device().ok_or(CaptureError::NoDevice)
}

/// Pick the input device's f32 config at its default rate and build the
/// stream. SampleFormat::F32 is required explicitly (spec §2.4): we do not
/// trust default heuristics, and Phase 1 does not add integer-format
/// conversion paths.
fn build_stream(
    ring_seconds: u32,
    input_device: Option<&str>,
    stream_error: Arc<AtomicBool>,
    level: Arc<LevelMeter>,
) -> Result<(cpal::Stream, Consumer, u32), CaptureError> {
    let host = cpal::default_host();
    let device = select_device(&host, input_device)?;

    let default = device
        .default_input_config()
        .map_err(|e| CaptureError::QueryConfig(e.to_string()))?;

    let supported = if default.sample_format() == cpal::SampleFormat::F32 {
        default
    } else {
        // The default is an integer format: look for an f32 config that can
        // run at the device's default rate.
        let rate = default.sample_rate();
        let mut offered = vec![default.sample_format().to_string()];
        let candidate = device
            .supported_input_configs()
            .map_err(|e| CaptureError::QueryConfig(e.to_string()))?
            .inspect(|r| offered.push(r.sample_format().to_string()))
            .filter(|r| r.sample_format() == cpal::SampleFormat::F32)
            .find(|r| r.min_sample_rate() <= rate && rate <= r.max_sample_rate())
            .map(|r| r.with_sample_rate(rate));
        candidate.ok_or_else(|| CaptureError::NoF32Config {
            offered: offered.join(", "),
        })?
    };

    let sample_rate = supported.sample_rate();
    let channels = supported.channels() as usize;

    // Log what we actually opened. "Hark can't hear me" reports are almost
    // always one of: the wrong device, an unexpected channel layout, or a rate
    // we then resample from -- and none of those are visible to the user or
    // recoverable from a transcript. One line at startup makes every later
    // report answerable.
    log::info!(
        "capture open: device {:?}, {} Hz, {} channel(s), f32",
        device.to_string(),
        sample_rate,
        channels,
    );
    if channels > 1 {
        log::info!(
            "multi-channel input: taking channel 0 of {channels} (array-mic \
             reference channels are commonly silent, and averaging them costs 6 dB)"
        );
    }

    let config: cpal::StreamConfig = supported.into();

    let (producer, consumer): (Producer, Consumer) =
        ring(ring_seconds as usize * sample_rate as usize);

    let error_flag = stream_error.clone();
    let stream = device
        .build_input_stream(
            config,
            move |data: &[f32], _info: &cpal::InputCallbackInfo| {
                // Hot path: relaxed atomic stores only (cpal #970). The level
                // meter is the same discipline (a bounded scan + one relaxed
                // store) and is advisory-only, so it never affects the ring.
                producer.push_interleaved(data, channels);
                level.observe(data);
            },
            move |err| {
                // Called on stream failure (device lost). Not the data path;
                // a store is all we do.
                error_flag.store(true, Ordering::Relaxed);
                log::error!("input stream error: {err}");
            },
            None,
        )
        .map_err(|e| CaptureError::BuildStream(e.to_string()))?;

    Ok((stream, consumer, sample_rate))
}

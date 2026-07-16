//! Live cpal capture. I/O glue: verifiable only on real hardware with a real
//! microphone (run-on-real-HW). All decision logic lives in the pure modules.
//!
//! The stream is built and owned by a dedicated thread so WASAPI's COM
//! apartment is initialized by exactly one owner: sharing a thread with
//! egui/winit or the keyboard hook risks `RPC_E_CHANGED_MODE` when the other
//! occupant initializes COM in a different mode first. The cpal input
//! callback only calls `Producer::push_interleaved` (relaxed atomic stores;
//! no allocation, no locks, no syscalls) per the cpal #970 gotcha.

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

/// A running capture: the ring consumer plus the live device rate. Dropping
/// the handle stops the stream (its owning thread exits and the stream drops
/// with it).
pub struct CaptureHandle {
    consumer: Consumer,
    sample_rate: u32,
    stream_error: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl CaptureHandle {
    pub fn consumer(&self) -> &Consumer {
        &self.consumer
    }

    /// The device rate samples arrive at (the ring's rate). Downstream
    /// resamples to 16 kHz per clip.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
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

/// Start continuous capture into a ring of `ring_capacity` samples.
/// Blocks until the stream is live (or failed to build).
pub fn start(ring_capacity: usize) -> Result<CaptureHandle, CaptureError> {
    let shutdown = Arc::new(AtomicBool::new(false));
    let stream_error = Arc::new(AtomicBool::new(false));
    let (result_tx, result_rx) = mpsc::sync_channel::<Result<(Consumer, u32), CaptureError>>(1);

    let thread_shutdown = shutdown.clone();
    let thread_error = stream_error.clone();
    let thread = std::thread::Builder::new()
        .name("hark-audio-capture".to_string())
        .spawn(move || {
            capture_thread(ring_capacity, thread_error, thread_shutdown, result_tx);
        })
        .expect("spawning the capture thread cannot fail");

    match result_rx.recv() {
        Ok(Ok((consumer, sample_rate))) => Ok(CaptureHandle {
            consumer,
            sample_rate,
            stream_error,
            shutdown,
            thread: Some(thread),
        }),
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
    ring_capacity: usize,
    stream_error: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    result_tx: mpsc::SyncSender<Result<(Consumer, u32), CaptureError>>,
) {
    let built = build_stream(ring_capacity, stream_error);
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

/// Pick the default input device's f32 config at its default rate and build
/// the stream. SampleFormat::F32 is required explicitly (spec §2.4): we do
/// not trust default heuristics, and Phase 1 does not add integer-format
/// conversion paths.
fn build_stream(
    ring_capacity: usize,
    stream_error: Arc<AtomicBool>,
) -> Result<(cpal::Stream, Consumer, u32), CaptureError> {
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or(CaptureError::NoDevice)?;

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
    let config: cpal::StreamConfig = supported.into();

    let (producer, consumer): (Producer, Consumer) = ring(ring_capacity);

    let error_flag = stream_error.clone();
    let stream = device
        .build_input_stream(
            config,
            move |data: &[f32], _info: &cpal::InputCallbackInfo| {
                // Hot path: relaxed atomic stores only (cpal #970).
                producer.push_interleaved(data, channels);
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

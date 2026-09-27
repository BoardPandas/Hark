//! Per-process system-audio loopback for meeting mode: the "Them" channel.
//! WASAPI glue, verifiable only on real hardware (`examples/loopback_smoke.rs`).
//!
//! `AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK` (Windows 10 2004, build
//! 19041+) captures one process tree's audio whichever endpoint it renders to,
//! which endpoint loopback cannot: on real machines the default render device
//! and the communications render device differ, and a meeting app on the
//! latter is invisible to loopback of the former (CP0). It also delivers
//! packets continuously through silence, so the channel's timeline is simply
//! its sample count, and it converts to 16 kHz mono i16 itself.
//!
//! Like `capture_win`, the stream is built and owned by one dedicated thread
//! that owns its MTA apartment; the thread's only per-packet work is
//! converting into a stack buffer and `Producer::push` (no allocation, no
//! locks). Samples land in the same f32 [`ring`](crate::ring) the microphone
//! uses, as `i16 / 32768`, which [`crate::spool::f32_to_i16`] inverts exactly.

use crate::ring::Consumer;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use thiserror::Error;

/// The rate the loopback delivers: Windows resamples to it (`AUTOCONVERTPCM`).
pub const LOOPBACK_RATE: u32 = crate::TARGET_RATE;

/// Whose audio to capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopbackTarget {
    /// Only this process and its descendants: a detected meeting app, by root
    /// PID (the tree covers WebView2 and browser audio child processes).
    IncludeTree(u32),
    /// Everything except this process and its descendants: a manual start,
    /// excluding Hark's own PID so its sounds are never transcribed.
    ExcludeTree(u32),
}

#[derive(Debug, Error)]
pub enum LoopbackError {
    #[error("system-audio capture is not implemented on this platform")]
    UnsupportedPlatform,
    /// Includes Windows builds before 19041, which have no process loopback:
    /// the caller's cue to fall back to endpoint loopback.
    #[error("cannot activate process loopback: {0}")]
    Activate(String),
    #[error("cannot start process loopback: {0}")]
    Start(String),
    #[error("loopback thread exited before reporting a stream")]
    ThreadDied,
}

/// A running loopback capture. The ring `Consumer` is handed out at start and
/// moves to the meeting drain; this handle keeps the stream alive, and
/// dropping it stops the stream and joins its thread.
pub struct LoopbackHandle {
    shutdown: Arc<AtomicBool>,
    stream_error: Arc<AtomicBool>,
    discontinuities: Arc<AtomicU64>,
    start_qpc_ns: Arc<AtomicU64>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl LoopbackHandle {
    /// Always [`LOOPBACK_RATE`]: no resampling needed downstream.
    pub fn sample_rate(&self) -> u32 {
        LOOPBACK_RATE
    }

    /// True once capture has stopped for good (the stream reported an error
    /// and its thread exited). Latching.
    pub fn stream_errored(&self) -> bool {
        self.stream_error.load(Ordering::Relaxed)
    }

    /// Packets WASAPI flagged as following a gap (audio lost before Hark saw
    /// it). One at startup is normal. Monotonic.
    pub fn discontinuities(&self) -> Arc<AtomicU64> {
        self.discontinuities.clone()
    }

    /// When ring sample 0 was captured, on the QPC clock in nanoseconds (the
    /// clock cpal stamps microphone packets with), or `None` before the first
    /// packet with a valid timestamp. The drain uses it to place this channel
    /// on the session timeline beside the microphone.
    pub fn start_qpc_ns(&self) -> Option<u64> {
        match self.start_qpc_ns.load(Ordering::Acquire) {
            0 => None,
            ns => Some(ns),
        }
    }
}

impl Drop for LoopbackHandle {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            // The thread wakes at least every 100 ms to check `shutdown`.
            let _ = t.join();
        }
    }
}

/// Start capturing `target` into a ring of `ring_seconds` at
/// [`LOOPBACK_RATE`]. Blocks until the stream is live or has failed.
pub fn start_process_loopback(
    target: LoopbackTarget,
    ring_seconds: u32,
) -> Result<(LoopbackHandle, Consumer), LoopbackError> {
    #[cfg(windows)]
    {
        win::start(target, ring_seconds)
    }
    #[cfg(not(windows))]
    {
        let _ = (target, ring_seconds);
        Err(LoopbackError::UnsupportedPlatform)
    }
}

#[cfg(windows)]
mod win {
    use super::*;
    use crate::ring::{ring, Producer};
    use std::mem::ManuallyDrop;
    use std::sync::mpsc;
    use windows::core::{implement, Interface, Ref, Result as WinResult, HRESULT};
    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
    use windows::Win32::Media::Audio::{
        ActivateAudioInterfaceAsync, IActivateAudioInterfaceAsyncOperation,
        IActivateAudioInterfaceCompletionHandler, IActivateAudioInterfaceCompletionHandler_Impl,
        IAudioCaptureClient, IAudioClient, AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY,
        AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR, AUDCLNT_SHAREMODE_SHARED,
        AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
        AUDCLNT_STREAMFLAGS_LOOPBACK, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
        AUDIOCLIENT_ACTIVATION_PARAMS, AUDIOCLIENT_ACTIVATION_PARAMS_0,
        AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK, AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS,
        PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE,
        PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE, VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
        WAVEFORMATEX, WAVE_FORMAT_PCM,
    };
    use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, BLOB, COINIT_MULTITHREADED};
    use windows::Win32::System::Threading::{CreateEventW, SetEvent, WaitForSingleObject};
    use windows::Win32::System::Variant::VT_BLOB;

    /// Activation normally completes in ~2 ms (CP0); this only bounds a hang.
    const ACTIVATION_TIMEOUT_MS: u32 = 5_000;
    /// The capture thread re-checks `shutdown` at least this often.
    const WAIT_MS: u32 = 100;
    /// Shared-mode buffer, in 100 ns units: 200 ms. Packets still arrive every
    /// ~10 ms; the size is only slack for a descheduled thread, so a larger
    /// buffer costs no latency and makes an overrun far less likely.
    const BUFFER_HNS: i64 = 2_000_000;
    /// Samples converted per `push`: small enough for the stack.
    const PUSH_BATCH: usize = 480;

    /// State shared between the handle and the capture thread.
    struct Shared {
        shutdown: Arc<AtomicBool>,
        stream_error: Arc<AtomicBool>,
        discontinuities: Arc<AtomicU64>,
        start_qpc_ns: Arc<AtomicU64>,
    }

    pub(super) fn start(
        target: LoopbackTarget,
        ring_seconds: u32,
    ) -> Result<(LoopbackHandle, Consumer), LoopbackError> {
        let shared = Shared {
            shutdown: Arc::new(AtomicBool::new(false)),
            stream_error: Arc::new(AtomicBool::new(false)),
            discontinuities: Arc::new(AtomicU64::new(0)),
            start_qpc_ns: Arc::new(AtomicU64::new(0)),
        };
        let handle_parts = (
            shared.shutdown.clone(),
            shared.stream_error.clone(),
            shared.discontinuities.clone(),
            shared.start_qpc_ns.clone(),
        );
        let (producer, consumer) = ring(ring_seconds as usize * LOOPBACK_RATE as usize);
        let (result_tx, result_rx) = mpsc::sync_channel::<Result<(), LoopbackError>>(1);
        let thread = std::thread::Builder::new()
            .name("hark-audio-loopback".to_string())
            .spawn(move || capture_thread(target, producer, shared, result_tx))
            .map_err(|e| LoopbackError::Start(format!("spawning the capture thread: {e}")))?;

        let (shutdown, stream_error, discontinuities, start_qpc_ns) = handle_parts;
        match result_rx.recv() {
            Ok(Ok(())) => Ok((
                LoopbackHandle {
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
            Err(_) => Err(LoopbackError::ThreadDied),
        }
    }

    /// Owns this thread's MTA apartment. Declared before any COM interface in
    /// a scope, so it drops (CoUninitialize) after all of them.
    struct Apartment;

    impl Apartment {
        fn enter() -> WinResult<Apartment> {
            // SAFETY: initializes COM for this thread only; balanced by Drop.
            unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
            Ok(Apartment)
        }
    }

    impl Drop for Apartment {
        fn drop(&mut self) {
            // SAFETY: balances the successful CoInitializeEx in `enter`.
            unsafe { CoUninitialize() };
        }
    }

    /// An auto-reset Win32 event, closed on drop.
    struct Event(HANDLE);

    impl Event {
        fn new() -> WinResult<Event> {
            // SAFETY: plain event creation; the handle is owned by the result.
            unsafe { CreateEventW(None, false, false, None) }.map(Event)
        }
    }

    impl Drop for Event {
        fn drop(&mut self) {
            // SAFETY: we own the handle and nothing uses it after this. A
            // failure means it was already invalid; there is nothing to do.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    /// Signals its event when activation completes. The handler owns the
    /// event: the async operation holds a reference to the handler until it
    /// fires, so after a timeout the event stays valid for the late callback
    /// and closes only when the last reference goes.
    #[implement(IActivateAudioInterfaceCompletionHandler)]
    struct Activated(Event);

    impl IActivateAudioInterfaceCompletionHandler_Impl for Activated_Impl {
        fn ActivateCompleted(
            &self,
            _operation: Ref<IActivateAudioInterfaceAsyncOperation>,
        ) -> WinResult<()> {
            // SAFETY: the event is alive for as long as this handler is.
            unsafe { SetEvent(self.0 .0) }
        }
    }

    fn capture_thread(
        target: LoopbackTarget,
        producer: Producer,
        shared: Shared,
        result_tx: mpsc::SyncSender<Result<(), LoopbackError>>,
    ) {
        let _apartment = match Apartment::enter() {
            Ok(a) => a,
            Err(e) => {
                let _ = result_tx.send(Err(LoopbackError::Start(format!("COM init: {e}"))));
                return;
            }
        };
        let stream = match open(target) {
            Ok(s) => s,
            Err(e) => {
                let _ = result_tx.send(Err(e));
                return;
            }
        };
        log::info!("loopback open: {target:?}, {LOOPBACK_RATE} Hz mono i16 (process loopback)");
        let _ = result_tx.send(Ok(()));
        if let Err(e) = pump(&stream, &producer, &shared) {
            shared.stream_error.store(true, Ordering::Relaxed);
            log::error!("loopback stream error: {e}");
        }
        // SAFETY: stopping a client this thread started; errors only mean the
        // stream is already gone.
        if let Err(e) = unsafe { stream.client.Stop() } {
            log::debug!("loopback stop: {e}");
        }
        // `stream` drops here, before `_apartment`.
    }

    struct Stream {
        client: IAudioClient,
        capture: IAudioCaptureClient,
        ready: Event,
    }

    fn open(target: LoopbackTarget) -> Result<Stream, LoopbackError> {
        let client = activate(target)?;
        let start = |e: windows::core::Error| LoopbackError::Start(e.to_string());
        // GetMixFormat is unsupported on the process-loopback virtual device:
        // ask for 16 kHz mono i16 and let AUTOCONVERTPCM convert.
        let format = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_PCM as u16,
            nChannels: 1,
            nSamplesPerSec: LOOPBACK_RATE,
            wBitsPerSample: 16,
            nBlockAlign: 2,
            nAvgBytesPerSec: LOOPBACK_RATE * 2,
            cbSize: 0,
        };
        // SAFETY: COM calls on this thread's apartment; `format` outlives the
        // Initialize call, which copies it.
        unsafe {
            client
                .Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_LOOPBACK
                        | AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                        | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
                        | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                    BUFFER_HNS,
                    0,
                    &format,
                    None,
                )
                .map_err(start)?;
            let ready = Event::new().map_err(start)?;
            client.SetEventHandle(ready.0).map_err(start)?;
            let capture: IAudioCaptureClient = client.GetService().map_err(start)?;
            client.Start().map_err(start)?;
            Ok(Stream {
                client,
                capture,
                ready,
            })
        }
    }

    fn activate(target: LoopbackTarget) -> Result<IAudioClient, LoopbackError> {
        let failed = |e: windows::core::Error| LoopbackError::Activate(e.to_string());
        let (pid, mode) = match target {
            LoopbackTarget::IncludeTree(pid) => {
                (pid, PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE)
            }
            LoopbackTarget::ExcludeTree(pid) => {
                (pid, PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE)
            }
        };
        // Lives on this stack until activation has completed below.
        let params = AUDIOCLIENT_ACTIVATION_PARAMS {
            ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
            Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
                ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                    TargetProcessId: pid,
                    ProcessLoopbackMode: mode,
                },
            },
        };
        // windows 0.62's PROPVARIANT has a Drop that calls PropVariantClear,
        // which would CoTaskMemFree this blob: it points at `params` on the
        // stack, and the process dies with no message (LL-G
        // `propvariant-drop-frees-blob`). It must never drop.
        let mut blob = ManuallyDrop::new(PROPVARIANT::default());
        // SAFETY: writing the VT_BLOB arm of a default (VT_EMPTY) PROPVARIANT.
        unsafe {
            let inner = &mut *blob.Anonymous.Anonymous;
            inner.vt = VT_BLOB;
            inner.Anonymous.blob = BLOB {
                cbSize: std::mem::size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
                pBlobData: &params as *const _ as *mut u8,
            };
        }

        let event = Event::new().map_err(failed)?;
        let done = event.0;
        let handler: IActivateAudioInterfaceCompletionHandler = Activated(event).into();
        // SAFETY: `blob` and `params` outlive the wait for completion; `done`
        // is valid while `handler` (which owns it) is alive, through the wait.
        unsafe {
            let operation = ActivateAudioInterfaceAsync(
                VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
                &IAudioClient::IID,
                Some(&*blob),
                &handler,
            )
            .map_err(failed)?;
            if WaitForSingleObject(done, ACTIVATION_TIMEOUT_MS) != WAIT_OBJECT_0 {
                return Err(LoopbackError::Activate(format!(
                    "no completion within {ACTIVATION_TIMEOUT_MS} ms"
                )));
            }
            let mut result = HRESULT(0);
            let mut activated = None;
            operation
                .GetActivateResult(&mut result, &mut activated)
                .map_err(failed)?;
            result.ok().map_err(failed)?;
            activated
                .ok_or_else(|| LoopbackError::Activate("no interface returned".into()))?
                .cast()
                .map_err(failed)
        }
    }

    /// Deliver packets to the ring until shutdown or a stream error.
    fn pump(stream: &Stream, producer: &Producer, shared: &Shared) -> WinResult<()> {
        let mut pushed: u64 = 0;
        while !shared.shutdown.load(Ordering::Relaxed) {
            // SAFETY: waiting on an event this thread owns.
            let woke = unsafe { WaitForSingleObject(stream.ready.0, WAIT_MS) };
            if woke == WAIT_TIMEOUT {
                continue;
            }
            if woke != WAIT_OBJECT_0 {
                return Err(windows::core::Error::from_thread());
            }
            drain_packets(stream, producer, shared, &mut pushed)?;
        }
        Ok(())
    }

    fn drain_packets(
        stream: &Stream,
        producer: &Producer,
        shared: &Shared,
        pushed: &mut u64,
    ) -> WinResult<()> {
        let mut batch = [0f32; PUSH_BATCH];
        loop {
            // SAFETY: capture-client calls on the owning thread. The buffer
            // from GetBuffer is valid for `frames` i16 samples (mono, the
            // format we initialized) until ReleaseBuffer.
            unsafe {
                if stream.capture.GetNextPacketSize()? == 0 {
                    return Ok(());
                }
                let (mut data, mut frames, mut flags, mut qpc) =
                    (std::ptr::null_mut(), 0u32, 0u32, 0u64);
                stream.capture.GetBuffer(
                    &mut data,
                    &mut frames,
                    &mut flags,
                    None,
                    Some(&raw mut qpc),
                )?;
                if flags & AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY.0 as u32 != 0 {
                    shared.discontinuities.fetch_add(1, Ordering::Relaxed);
                }
                if flags & AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR.0 as u32 == 0
                    && shared.start_qpc_ns.load(Ordering::Relaxed) == 0
                {
                    // QPC is in 100 ns units; back-date to ring sample 0.
                    let ns =
                        (qpc * 100).saturating_sub(*pushed * 1_000_000_000 / LOOPBACK_RATE as u64);
                    shared.start_qpc_ns.store(ns.max(1), Ordering::Release);
                }
                // A SILENT packet's buffer contents are undefined: push zeros.
                if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null() {
                    let zeros = [0f32; PUSH_BATCH];
                    let mut left = frames as usize;
                    while left > 0 {
                        let n = left.min(PUSH_BATCH);
                        producer.push(&zeros[..n]);
                        left -= n;
                    }
                } else {
                    let pcm = std::slice::from_raw_parts(data as *const i16, frames as usize);
                    for part in pcm.chunks(PUSH_BATCH) {
                        for (out, &v) in batch.iter_mut().zip(part) {
                            *out = v as f32 / 32768.0;
                        }
                        producer.push(&batch[..part.len()]);
                    }
                }
                *pushed += frames as u64;
                stream.capture.ReleaseBuffer(frames)?;
            }
        }
    }
}

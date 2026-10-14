//! The Windows half of the loopback seam: WASAPI process loopback, and
//! endpoint loopback for a call playing where process loopback is deaf. The
//! shared contract and both platforms' docs live in `super`.

use super::gaps::{GapFiller, TICKS_PER_SECOND};
use super::*;
use crate::ring::{ring, Producer};
use std::mem::ManuallyDrop;
use std::sync::mpsc;
use windows::core::{implement, Interface, Ref, Result as WinResult, HRESULT, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::Media::Audio::{
    ActivateAudioInterfaceAsync, IActivateAudioInterfaceAsyncOperation,
    IActivateAudioInterfaceCompletionHandler, IActivateAudioInterfaceCompletionHandler_Impl,
    IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator,
    AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY, AUDCLNT_BUFFERFLAGS_SILENT,
    AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR, AUDCLNT_SHAREMODE_SHARED,
    AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
    AUDCLNT_STREAMFLAGS_LOOPBACK, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
    AUDIOCLIENT_ACTIVATION_PARAMS, AUDIOCLIENT_ACTIVATION_PARAMS_0,
    AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK, AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS,
    PROCESS_LOOPBACK_MODE, PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE,
    PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE, VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
    WAVEFORMATEX, WAVE_FORMAT_PCM,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, BLOB, CLSCTX_ALL, COINIT_MULTITHREADED,
};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
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
    let endpoint = matches!(target, LoopbackTarget::Endpoint { .. });
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
                timeline_from_clock: endpoint,
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
    let stream = match open(&target) {
        Ok(s) => s,
        Err(e) => {
            let _ = result_tx.send(Err(e));
            return;
        }
    };
    // Endpoint loopback goes quiet in silence, so its timeline runs on the
    // clock its packets are stamped with, from now. Stored before the start
    // is reported: the recorder aligns by it on its first read.
    let gaps = match &target {
        LoopbackTarget::Endpoint { .. } => {
            let origin = qpc_ticks();
            shared
                .start_qpc_ns
                .store((origin * 100).max(1), Ordering::Release);
            log::info!(
                "loopback open: {target:?}, {LOOPBACK_RATE} Hz mono i16 (endpoint loopback)"
            );
            Some(GapFiller::new(origin, LOOPBACK_RATE))
        }
        _ => {
            log::info!("loopback open: {target:?}, {LOOPBACK_RATE} Hz mono i16 (process loopback)");
            None
        }
    };
    let _ = result_tx.send(Ok(()));
    if let Err(e) = pump(&stream, &producer, &shared, gaps) {
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

fn open(target: &LoopbackTarget) -> Result<Stream, LoopbackError> {
    let client = match target {
        LoopbackTarget::IncludeTree(pid) => {
            activate(*pid, PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE)?
        }
        LoopbackTarget::ExcludeTree(pid) => {
            activate(*pid, PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE)?
        }
        LoopbackTarget::Endpoint { id } => activate_endpoint(id)?,
    };
    let start = |e: windows::core::Error| LoopbackError::Start(e.to_string());
    // GetMixFormat is unsupported on the process-loopback virtual device:
    // ask for 16 kHz mono i16 and let AUTOCONVERTPCM convert. A real
    // endpoint has a mix format, but the same request keeps one path.
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

/// A render endpoint's own client, by Core Audio endpoint id. No
/// activation PROPVARIANT is involved, unlike process loopback.
fn activate_endpoint(id: &str) -> Result<IAudioClient, LoopbackError> {
    let failed = |e: windows::core::Error| LoopbackError::Activate(format!("endpoint {id}: {e}"));
    let wide: Vec<u16> = id.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: COM calls in this thread's apartment; `wide` is NUL-terminated
    // and outlives GetDevice, which copies it.
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(failed)?;
        enumerator
            .GetDevice(PCWSTR(wide.as_ptr()))
            .map_err(failed)?
            .Activate::<IAudioClient>(CLSCTX_ALL, None)
            .map_err(failed)
    }
}

fn activate(pid: u32, mode: PROCESS_LOOPBACK_MODE) -> Result<IAudioClient, LoopbackError> {
    let failed = |e: windows::core::Error| LoopbackError::Activate(e.to_string());
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

/// Deliver packets to the ring until shutdown or a stream error. Endpoint
/// loopback (`gaps`) also places the silence while no packets come.
fn pump(
    stream: &Stream,
    producer: &Producer,
    shared: &Shared,
    mut gaps: Option<GapFiller>,
) -> WinResult<()> {
    let mut pushed: u64 = 0;
    while !shared.shutdown.load(Ordering::Relaxed) {
        // SAFETY: waiting on an event this thread owns.
        let woke = unsafe { WaitForSingleObject(stream.ready.0, WAIT_MS) };
        if woke != WAIT_OBJECT_0 && woke != WAIT_TIMEOUT {
            return Err(windows::core::Error::from_thread());
        }
        // Drained after a timeout too, so packets never wait on an event
        // that loopback might not signal; without packets it is one call.
        drain_packets(stream, producer, shared, &mut pushed, gaps.as_mut())?;
        if let Some(gaps) = gaps.as_mut() {
            let silence = gaps.idle(qpc_ticks());
            push_zeros(producer, silence);
            pushed += silence;
        }
    }
    Ok(())
}

/// The QPC clock in the 100 ns ticks WASAPI stamps its packets with.
fn qpc_ticks() -> u64 {
    let (mut count, mut frequency) = (0i64, 0i64);
    // SAFETY: plain reads into locals; both calls always succeed on any
    // Windows Hark runs on.
    unsafe {
        let _ = QueryPerformanceCounter(&mut count);
        let _ = QueryPerformanceFrequency(&mut frequency);
    }
    (count.max(0) as u128 * u128::from(TICKS_PER_SECOND) / frequency.max(1) as u128) as u64
}

/// Frames of endpoint loopback's timeline due by now, from its start in
/// nanoseconds: what it has captured, including silence still inside
/// [`GapFiller`]'s margin.
pub(super) fn frames_due(start_ns: u64) -> u64 {
    let elapsed = u128::from((qpc_ticks() * 100).saturating_sub(start_ns));
    (elapsed * u128::from(LOOPBACK_RATE) / 1_000_000_000) as u64
}

fn push_zeros(producer: &Producer, mut frames: u64) {
    let zeros = [0f32; PUSH_BATCH];
    while frames > 0 {
        let n = frames.min(PUSH_BATCH as u64) as usize;
        producer.push(&zeros[..n]);
        frames -= n as u64;
    }
}

fn drain_packets(
    stream: &Stream,
    producer: &Producer,
    shared: &Shared,
    pushed: &mut u64,
    mut gaps: Option<&mut GapFiller>,
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
            let timestamped = flags & AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR.0 as u32 == 0;
            // Endpoint loopback: the silence since the last packet first.
            // The packet that resumes after it is flagged discontinuous, but
            // nothing was lost: the silence is on the timeline.
            let mut resumed = false;
            if let Some(gaps) = gaps.as_deref_mut() {
                let silence = if timestamped {
                    gaps.before_packet(qpc, u64::from(frames))
                } else {
                    gaps.contiguous(u64::from(frames));
                    0
                };
                push_zeros(producer, silence);
                *pushed += silence;
                resumed = silence > 0;
            }
            if flags & AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY.0 as u32 != 0 && !resumed {
                shared.discontinuities.fetch_add(1, Ordering::Relaxed);
            }
            if timestamped && shared.start_qpc_ns.load(Ordering::Relaxed) == 0 {
                // QPC is in 100 ns units; back-date to ring sample 0.
                let ns = (qpc * 100).saturating_sub(*pushed * 1_000_000_000 / LOOPBACK_RATE as u64);
                shared.start_qpc_ns.store(ns.max(1), Ordering::Release);
            }
            // A SILENT packet's buffer contents are undefined: push zeros.
            if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null() {
                push_zeros(producer, u64::from(frames));
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

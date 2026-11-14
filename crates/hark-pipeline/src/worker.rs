//! The pipeline worker loop: receive chord edges, assemble the window,
//! encode, transcribe (one retry max), inject. Runs on its own thread; all
//! logging is lengths/counts/millis, never key material, audio, or text.

use crate::events::{FailStage, PipelineEvent};
use crate::input::WorkerInput;
use crate::local::{LocalPlan, Source};
use crate::state::{advance, Action, Event, PipelineState};
use crate::stream::LiveTurn;
use hark_audio::ring::Consumer;
use hark_audio::WindowParams;
use hark_hotkey::PttEvent;
use hark_inject::InjectSettings;
use hark_spellbook::{Corrector, Expander};
use hark_stt::{LiveStt, SttProvider};
use hark_voice::{CleanupProvider, Voice};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

mod engines;
mod held;
mod record;
mod shape;

#[cfg(test)]
mod held_tests;
#[cfg(test)]
mod recovery_tests;
#[cfg(test)]
mod tests;

use engines::transcribe_clip;
use held::{retry_requested, HeldClip};
use record::{build_record, RecordParts};
use shape::shape_transcript;

// Named only by a doc comment on `Worker`.
#[cfg(doc)]
use shape::strip_single_word_period;

/// The resolved cleanup step: adapter, effective voice, and gate threshold.
/// `None` on the worker means Verbatim (or degraded-to-Verbatim): no cleanup
/// call is ever made.
pub(crate) struct CleanupPlan {
    pub cleaner: Box<dyn CleanupProvider>,
    /// The effective voice, for log lines only.
    pub voice: Voice,
    /// The chat model, for log lines only (config label, not user content).
    pub model: String,
    /// Fewer words than this skip the cleanup call; 0 disables the gate.
    pub skip_below_words: u32,
    /// Output longer than this multiple of the input word count is discarded
    /// and the uncleaned transcript injected; 0 disables the check.
    pub max_expansion_ratio: f32,
    /// Set only when the cleanup endpoint differs from the STT one, so the
    /// first cleaned dictation also skips a cold TLS handshake.
    pub prewarm_url: Option<String>,
}

pub(crate) struct Worker {
    pub consumer: Consumer,
    pub sample_rate: u32,
    pub window: WindowParams,
    pub inject: InjectSettings,
    /// The cloud adapter. `None` when `[local_stt] mode = "primary"`: that
    /// mode never contacts a provider and does not even require an API key.
    pub provider: Option<Box<dyn SttProvider>>,
    /// The streaming adapter, when the provider has one, so audio can go up
    /// while the key is still held. `None` for every batch-only provider, and
    /// for a build without the live feature. For Gemini `provider` replays
    /// over the same socket protocol; for OpenAI Live it is batch REST, so the
    /// fallback does not share whatever just broke the stream.
    pub live: Option<Box<dyn LiveStt>>,
    /// Provider label for the dictation record. Kept separately because
    /// `provider` may be absent while the record still needs a label.
    pub cloud_label: String,
    /// On-device engine plan; `None` when local STT is off or unavailable.
    pub local: Option<LocalPlan>,
    /// Spellbook post-correction, built once from the configured terms.
    pub corrector: Corrector,
    /// Invocation expansion, built once from the configured triggers. A
    /// derived artifact: rebuilt from TOML on every pipeline start, never
    /// cached (LL-G `architecture/transient-cache-without-drift`).
    pub expander: Expander,
    /// Optional voice cleanup between spellbook pass 1 and injection.
    pub cleanup: Option<CleanupPlan>,
    /// Base URL to pre-warm (DNS + TCP + TLS) before the first dictation.
    pub prewarm_url: String,
    /// The shared HTTP client (same instance the provider adapter holds),
    /// used only for the pre-warm requests.
    pub client: reqwest::blocking::Client,
    /// The model `live` streams to, for the record of a dictation the stream
    /// delivered.
    pub stream_model: String,
    /// The model `provider` runs, for the record of a dictation it delivered.
    /// Differs from `stream_model` only for OpenAI Live, whose fallback is
    /// batch REST rather than another realtime session.
    pub batch_model: String,
    /// Drop the trailing period on a single-word dictation before injecting
    /// (see [`strip_single_word_period`]). Off leaves the text exactly as the
    /// provider/cleanup produced it.
    pub strip_single_word_period: bool,
    /// Explicit opt-in: app identity only, sampled at engagement.
    pub track_apps: bool,
    /// Advisory events toward the UI; every send is `let _ =` best-effort.
    pub events: Sender<PipelineEvent>,
    /// The same "is a dictation capturing" fact as [`PipelineEvent::Recording`],
    /// published as state so the recording overlay can read it without waiting
    /// on a UI pass (`PipelineHandle::recording_flag`).
    pub recording: Arc<AtomicBool>,
    pub control: Arc<crate::lifecycle::RunControl>,
    /// Capture discontinuities since the stream opened, from
    /// `CaptureHandle::discontinuities`. Monotonic: the worker samples it at
    /// both ends of a hold and reports the difference.
    pub discontinuities: Arc<AtomicU64>,
    /// The one clip whose transcription failed for good, kept so the user can
    /// retry from History instead of re-speaking. Memory only: never written
    /// to disk, never logged. At most one; see [`HeldClip`].
    pub held: Option<HeldClip>,
}

/// The one long-lived worker loop. Exits when the hotkey listener drops its
/// sender (pipeline shutdown).
pub(crate) fn run(mut worker: Worker, rx: Receiver<WorkerInput>) {
    // Read the on-device model in while the connection pre-warms below, so the
    // first dictation (primary) or first rescue (backup) skips the cold load.
    // A helper thread does the reading; nothing here waits for it.
    if let Some(plan) = worker.local.as_mut() {
        plan.start_preload(worker.control.clone());
    }
    // Primary-mode never issues a cloud request, so warming a connection to a
    // provider we will not call would just be a pointless network round trip.
    if worker.provider.is_some() && !worker.control.is_cancelled() {
        prewarm(&worker.client, &worker.prewarm_url);
    }
    if let Some(url) = worker
        .cleanup
        .as_ref()
        .filter(|_| !worker.control.is_cancelled())
        .and_then(|p| p.prewarm_url.as_deref())
    {
        prewarm(&worker.client, url);
    }

    let mut state = PipelineState::Idle;
    // Capture discontinuities counted as this dictation's hold began, so the
    // ones that land inside it can be reported with it.
    let mut discontinuities_at_down = 0u64;
    // The open live session, if this dictation is streaming. Dropped on every
    // path out of Recording, so a session can never outlive its dictation.
    let mut live = LiveTurn::default();
    let mut foreground = None;
    loop {
        if worker.control.is_cancelled() {
            break;
        }
        // While recording, wake regularly to push whatever the ring has
        // gained; otherwise block, so an idle Hark costs nothing.
        let input = if matches!(state, PipelineState::Recording { .. }) && live.pump.is_some() {
            match rx.recv_timeout(PUMP_INTERVAL) {
                Ok(event) => event,
                Err(RecvTimeoutError::Timeout) => {
                    if !worker.control.is_cancelled() {
                        if let Err(e) = live.push_available() {
                            log::warn!("live stream failed mid-hold: {e}");
                        }
                    }
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match rx.recv() {
                Ok(event) => event,
                Err(_) => break,
            }
        };
        if worker.control.is_cancelled() {
            break;
        }
        let edge = match input {
            WorkerInput::Edge(edge) => edge,
            WorkerInput::RetryHeld => {
                retry_requested(&mut worker, state);
                continue;
            }
            WorkerInput::DismissHeld => {
                worker.held = None;
                continue;
            }
        };
        // The input observer timestamps and admits cycles even while this
        // worker is blocked in a provider. Queue latency cannot crop speech.
        let at_abs = edge.at_abs;
        let ev = match edge.event {
            PttEvent::Down => Event::PttDown { at_abs },
            PttEvent::Up => Event::PttUp { at_abs },
            // A release the hook never saw (hark-hotkey's watchdog found the
            // keys up). Within the maximum hold it is an ordinary release: the
            // watchdog polls fast enough that only a moment of trailing audio
            // is added. Past it, the release could be minutes old — the user
            // walked away, or the machine slept mid-hold — and "transcribe the
            // last two minutes of the room, then paste it wherever the cursor
            // now is" is the one outcome worth refusing outright.
            PttEvent::UpMissed if abandoned_hold(&worker, state, at_abs) => {
                log::warn!("missing release past the maximum hold; recording discarded");
                let _ = worker.events.send(PipelineEvent::Failed {
                    stage: FailStage::Abandoned,
                    detail: "the chord's release was never delivered".to_string(),
                });
                Event::Aborted
            }
            PttEvent::UpMissed => Event::PttUp { at_abs },
            // Advisory, not an edge: the state machine never sees it.
            PttEvent::Intercepted(key) => {
                log::warn!(
                    "push-to-talk: {key} is held but the system reports it up; \
                     another program (a key remapper?) is intercepting it"
                );
                let _ = worker.events.send(PipelineEvent::ShortcutIntercepted {
                    key: key.to_string(),
                });
                continue;
            }
        };
        let (next, action) = advance(state, ev);
        // Republish the edge as state for the overlay, and do it BEFORE
        // announcing it below. The event wakes the UI thread, which paints the
        // pill, and the pill reads this flag directly to decide whether to be
        // on screen — so publishing the state after the announcement lets a
        // fast UI pass observe "not recording" for the dictation it was just
        // told about, and put the pill away for the whole hold. Derived from
        // the state machine rather than tracked alongside it, so it cannot
        // drift from what `PipelineEvent::Recording` says: every path out of
        // Recording — release, abandoned hold, abort — passes through here.
        worker.recording.store(
            matches!(next, PipelineState::Recording { .. }),
            Ordering::Relaxed,
        );
        // Surface the two UI-visible edges: capture started, request in
        // flight. Everything after that is reported from dictate itself.
        if matches!(state, PipelineState::Idle) {
            if let PipelineState::Recording { down_abs } = next {
                foreground = crate::foreground::request(worker.track_apps);
                let _ = worker.events.send(PipelineEvent::Recording);
                discontinuities_at_down = worker.discontinuities.load(Ordering::Relaxed);
                // Open the session now so the handshake overlaps the hold
                // rather than the release. `None` simply means no streaming
                // for this dictation.
                live = LiveTurn::start(
                    worker.live.as_deref(),
                    &worker.consumer,
                    down_abs,
                    worker.sample_rate,
                    &worker.window,
                );
            }
        }
        state = next;
        if let Action::Dictate { down_abs, up_abs } = action {
            let _ = worker.events.send(PipelineEvent::Processing);
            report_discontinuities(&worker, discontinuities_at_down);
            state = dictate_guarded(
                &mut worker,
                down_abs,
                up_abs,
                state,
                std::mem::take(&mut live),
                foreground.take(),
            );
        }
        // Any other way out of Recording (abort, abandoned hold) ends the
        // session too: an orphaned socket would keep billing and never inject.
        if !matches!(state, PipelineState::Recording { .. }) {
            live = LiveTurn::default();
            foreground = None;
            worker.control.finish_cycle();
        }
    }
    // The hook is gone; nothing is being captured. Belt and braces for an
    // overlay that happens to be up as the pipeline is torn down.
    worker.recording.store(false, Ordering::Relaxed);
    log::debug!("ptt channel closed; pipeline worker exiting");
}

/// How often the worker wakes during a hold to push newly captured audio.
/// Short enough that the socket stays close to real time, long enough that a
/// two-minute hold is not thousands of wake-ups.
const PUMP_INTERVAL: Duration = Duration::from_millis(20);

/// Has this recording already outlived the longest hold the window math will
/// keep (`max_hold_s`)? Only asked of a release the hook never delivered: past
/// that point the assembled clip would be a truncated tail of room noise, not
/// the user's dictation, so there is nothing worth transcribing or injecting.
fn abandoned_hold(worker: &Worker, state: PipelineState, at_abs: u64) -> bool {
    let PipelineState::Recording { down_abs } = state else {
        return false;
    };
    let max_hold = worker.window.max_hold_s as u64 * worker.sample_rate as u64;
    at_abs.saturating_sub(down_abs) > max_hold
}

/// Warm the connection pool so the first dictation skips the 0.4-0.9 s
/// cold-start the spike measured (DNS + TCP + TLS handshake). The response
/// itself is irrelevant; failures only cost the warmth.
fn prewarm(client: &reqwest::blocking::Client, url: &str) {
    let started = Instant::now();
    match client.get(url).send() {
        Ok(_) => log::info!(
            "http client pre-warmed in {} ms",
            started.elapsed().as_millis()
        ),
        Err(e) => log::warn!(
            "http pre-warm failed after {} ms (timeout={}, connect={}); first dictation will pay the cold cost",
            started.elapsed().as_millis(),
            e.is_timeout(),
            e.is_connect()
        ),
    }
}

/// One full dictation: assemble -> gate -> encode -> transcribe -> inject.
/// Report how many capture discontinuities landed inside the hold just ended.
///
/// Per dictation rather than per occurrence, which is the form this is
/// actually diagnostic in: a bare line each time says only that the machine
/// glitches, while a count attached to a dictation answers the question a
/// user's report asks — did audio go missing out of *this* one. Silent when
/// there were none, which is the overwhelming majority.
///
/// Each discontinuity is audio the driver discarded before Hark saw it, so
/// the clip is spliced across the gap with nothing marking it. Measured at
/// about one device period, far too little to cost a word, but this is the
/// only record that it happened at all.
fn report_discontinuities(worker: &Worker, at_down: u64) {
    let during = worker
        .discontinuities
        .load(Ordering::Relaxed)
        .saturating_sub(at_down);
    if during > 0 {
        let plural = if during == 1 { "y" } else { "ies" };
        log::info!("capture: {during} discontinuit{plural} during this hold (audio the driver discarded; the clip is spliced across each gap)");
    }
}

/// Run one dictation, surviving a panic inside it.
///
/// This thread is the only consumer of push-to-talk events, and [`dictate`] is
/// a long reach into audio assembly, a provider, the spellbook, cleanup and
/// injection. A panic anywhere in there used to take the thread with it — and
/// because nothing supervises the thread, the app did not crash where the user
/// could see it. It sat on "Processing…" with every later keypress ignored,
/// until it was force-quit and relaunched. One malformed transcript is worth
/// one lost dictation, never the session.
///
/// `AssertUnwindSafe` is the honest claim here rather than a shrug: everything
/// reachable is either immutable after construction (corrector, expander,
/// provider) or an owned handle whose invariants do not span a dictation
/// (the resident on-device engine, the ring reader). The state machine is
/// driven back to Idle explicitly below rather than left wherever it stopped.
fn dictate_guarded(
    worker: &mut Worker,
    down_abs: u64,
    up_abs: u64,
    state: PipelineState,
    live: LiveTurn,
    foreground: Option<Receiver<Option<String>>>,
) -> PipelineState {
    let events = worker.events.clone();
    let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        dictate(
            worker,
            down_abs,
            up_abs,
            state,
            live,
            foreground,
            hark_inject::inject,
        )
    }));
    match attempt {
        Ok(next) => next,
        Err(_) => {
            // The process-wide panic hook has already logged the payload and
            // the location; this line says what Hark did about it.
            log::error!("dictation panicked; returning the pipeline to idle");
            let _ = events.send(PipelineEvent::Failed {
                stage: FailStage::Internal,
                detail: "the dictation failed unexpectedly".to_string(),
            });
            advance(state, Event::Aborted).0
        }
    }
}

/// Always returns the post-dictation state (Idle via Injected or Aborted).
/// Every exit reports its outcome on the events channel (best-effort).
fn dictate(
    worker: &mut Worker,
    down_abs: u64,
    up_abs: u64,
    state: PipelineState,
    live: LiveTurn,
    foreground: Option<Receiver<Option<String>>>,
    inject: impl FnOnce(&str, &InjectSettings) -> Result<(), hark_inject::InjectError>,
) -> PipelineState {
    if worker.control.is_cancelled() {
        return PipelineState::Idle;
    }
    let released = Instant::now();
    // Cloned up front so reporting a failure does not hold a borrow of
    // `worker` across the transcription step, which needs `&mut` for the
    // on-device engine's slot. `Sender` clones are cheap.
    let events = worker.events.clone();
    let fail = |stage: FailStage, detail: String| {
        let _ = events.send(PipelineEvent::Failed { stage, detail });
    };

    let clip = match hark_audio::assemble_window(
        &worker.consumer,
        worker.sample_rate,
        down_abs,
        up_abs,
        &worker.window,
    ) {
        Ok(hark_audio::Assembled::Clip(clip)) => clip,
        Ok(hark_audio::Assembled::Gated(verdict)) => {
            // Report WHICH gate fired: a tap is the user's own doing and needs
            // no response, while "we heard nothing" may mean their microphone
            // is misconfigured and is worth offering help for.
            let (stage, detail) = match verdict {
                hark_audio::GateVerdict::TooShort => (FailStage::GatedTooShort, "hold too short"),
                // Speech never reaches here; treat it as too-quiet defensively
                // rather than inventing a third outcome.
                _ => (FailStage::GatedTooQuiet, "no speech detected"),
            };
            log::info!("dictation gated ({detail}); no request sent");
            fail(stage, detail.to_string());
            return advance(state, Event::Aborted).0;
        }
        Err(e) => {
            log::error!("window assembly failed: {e}");
            fail(FailStage::Audio, e.to_string());
            return advance(state, Event::Aborted).0;
        }
    };
    let audio_ms = (clip.samples_16k.len() as u64) * 1_000 / 16_000;

    // Only the cloud adapters need a WAV; the on-device engine consumes the
    // f32 samples directly. In primary mode there is no cloud adapter, so
    // encoding one would be pure waste on the hot path.

    // Finish the live session first, when there is one: most of this window is
    // already uploaded, so all that remains is the tail plus the provider's
    // finalise. Only if that fails does the clip need a WAV at all.
    if worker.control.is_cancelled() {
        return PipelineState::Idle;
    }
    let streamed_attempt = live.attempted;
    let streamed = live.pump.and_then(|p| {
        let (_, end_abs) =
            hark_audio::window::window_bounds(down_abs, up_abs, worker.sample_rate, &worker.window);
        let window_len = end_abs.saturating_sub(
            hark_audio::window::window_bounds(down_abs, up_abs, worker.sample_rate, &worker.window).0,
        );
        let before_finish = p.sent();
        match p.finish(end_abs, clip.applied_gain) {
            Ok(transcript) => {
                log::info!(
                    "live stream: {:.0}% of the window was already uploaded at release; finalise took {} ms",
                    crate::stream::streamed_fraction(before_finish, window_len) * 100.0,
                    transcript.request_ms
                );
                Some(transcript)
            }
            Err(e) => {
                log::warn!("live stream failed at finalise: {e}");
                None
            }
        }
    });

    if worker.control.is_cancelled() {
        return PipelineState::Idle;
    }

    let encode_started = Instant::now();
    let wav = match (&streamed, &worker.provider) {
        // The stream produced the transcript; nothing will read a WAV.
        (Some(_), _) => Vec::new(),
        (None, Some(_)) => hark_stt::wav::encode_wav_16k_mono(&clip.samples_16k),
        (None, None) => Vec::new(),
    };
    log::debug!(
        "clip: {} samples at 16 kHz ({} bytes WAV, encoded in {} ms)",
        clip.samples_16k.len(),
        wav.len(),
        encode_started.elapsed().as_millis()
    );

    let delivered_by_stream = streamed.is_some();
    let (source, transcript) = match streamed {
        Some(transcript) => (Source::Cloud, transcript),
        None => {
            // In this arm the stream produced nothing, so an attempted stream
            // is a failed one, and it spent this dictation's first attempt:
            // the replay is the one retry. Retrying the replay too made three
            // attempts, and against a stranded Gemini turn that was 26 s with
            // every keypress queued behind it.
            let outcome = match transcribe_clip(
                worker,
                &clip.samples_16k,
                &wav,
                streamed_attempt,
                !streamed_attempt,
            ) {
                Ok(o) => o,
                Err(e) => {
                    log::error!("transcription failed: {e}");
                    fail(FailStage::Transcribe, e.to_string());
                    // Keep the audio so the user can retry from History rather
                    // than speak it all again. A retired pipeline has nobody
                    // left to ask.
                    if !worker.control.is_cancelled() {
                        worker.held = Some(HeldClip {
                            samples: clip.samples_16k,
                            audio_ms,
                            foreground_app: foreground.and_then(|rx| rx.try_recv().ok().flatten()),
                        });
                        let _ = events.send(PipelineEvent::ClipHeld { held: true });
                    }
                    return advance(state, Event::Aborted).0;
                }
            };
            (outcome.source, outcome.transcript)
        }
    };
    if worker.control.is_cancelled() {
        return PipelineState::Idle;
    }
    let state = advance(state, Event::TranscriptReady).0;

    if transcript.text.trim().is_empty() {
        log::info!("provider returned an empty transcript; nothing to inject");
        fail(
            FailStage::EmptyTranscript,
            "the provider returned an empty transcript".to_string(),
        );
        return advance(state, Event::Aborted).0;
    }
    // This dictation produced words, so a clip kept from an earlier failure is
    // stale: the user has moved on.
    if worker.held.take().is_some() {
        let _ = events.send(PipelineEvent::ClipHeld { held: false });
    }

    let Some(shaped) = shape_transcript(worker, &transcript) else {
        return PipelineState::Idle;
    };
    let cleaned = shaped.cleaned;
    let Some(result) = worker
        .control
        .inject(|| inject(&cleaned.text, &worker.inject))
    else {
        return PipelineState::Idle;
    };
    let total_ms = released.elapsed().as_millis() as u64;
    let chars = cleaned.text.chars().count();
    let request_ms = transcript.request_ms;
    let record = build_record(
        worker,
        RecordParts {
            transcript,
            cleaned,
            fired: shaped.fired,
            pass_one_replacements: shaped.pass_one_replacements,
            provider_cleaned: shaped.provider_cleaned,
            source,
            delivered_by_stream,
            audio_ms,
            total_ms,
            // Advisory metadata must never hold up successful delivery.
            foreground_app: foreground.and_then(|rx| rx.try_recv().ok().flatten()),
        },
    );
    match result {
        Ok(()) => {
            log::info!(
                "dictation injected: {chars} chars, {} {request_ms} ms, release-to-inject {total_ms} ms",
                match source {
                    Source::Cloud => "request",
                    _ => "on-device decode",
                },
            );
            let _ = events.send(PipelineEvent::Injected(Box::new(record)));
            advance(state, Event::Injected).0
        }
        Err(e) => {
            // The words exist; the paste did not land. Hand them to the UI so
            // they are not thrown away with the error.
            log::error!("injection failed ({chars} chars kept for the UI): {e}");
            let _ = events.send(PipelineEvent::NotPasted {
                record: Box::new(record),
                detail: e.to_string(),
            });
            advance(state, Event::Aborted).0
        }
    }
}

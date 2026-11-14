//! Hark dictation pipeline: the worker-thread state machine that glues
//! audio capture, push-to-talk edges, cloud STT, and injection.
//!
//! `run(settings, api_key, events)` spawns the three worker threads (audio
//! capture, keyboard hook, pipeline worker) and returns a handle; the
//! calling thread stays free. The `events` sender carries advisory
//! [`PipelineEvent`]s toward the UI: sends are non-blocking and a
//! disconnected receiver is ignored, so callers that do not care (hark-cli)
//! may drop the receiver immediately.

mod events;
mod foreground;
mod input;
mod lifecycle;
mod local;
pub mod meeting;
mod plan;
mod retry;
mod route;
mod state;
mod stream;
mod worker;

pub use events::{DictationRecord, FailStage, PipelineEvent};
pub use hark_audio::LevelMeter;
pub use retry::should_retry;
pub use route::{batch_stt_route, provider_config, stt_route};
pub use state::{advance, Action, Event, PipelineState};

use hark_config::Settings;
use hark_stt::SttError;
use plan::{
    build_cleanup, build_expander, build_local, inject_settings, live_adapter, window_params,
};
use route::fallback_config;
pub(crate) use route::meeting_chunk_config;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

/// How long shutdown waits for the worker to finish the dictation it is in.
///
/// Long enough for one that is about to land: release to injected text
/// measured under a second, and injection itself restores the clipboard it
/// borrowed, which is worth letting complete. Not long enough to sit out a
/// stuck provider. Joining without a bound held the UI thread — on quit, and on
/// every settings save — for as long as a stalled request took: 15 s in one
/// user's log, with a relaunch in the meantime finding the old instance still
/// holding the single-instance lock.
const WORKER_SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

#[derive(Debug, Error)]
pub enum PipelineError {
    #[error(transparent)]
    Hotkey(#[from] hark_hotkey::HotkeyError),
    #[error(transparent)]
    Capture(#[from] hark_audio::CaptureError),
    #[error(transparent)]
    Stt(#[from] SttError),
    #[error("invalid configuration: {0}")]
    Config(String),
}

/// The running pipeline. Dropping it shuts everything down in order: the
/// hotkey listener stops (closing the event channel), the worker drains and
/// exits, and the capture stream stops with its handle.
pub struct PipelineHandle {
    // Declaration order is load-bearing for Drop: listener first (closes the
    // channel), then the worker join, then capture.
    listener: Option<hark_hotkey::ListenerHandle>,
    worker: Option<std::thread::JoinHandle<()>>,
    /// Disconnects when the worker returns (or unwinds); never carries a value.
    worker_done: mpsc::Receiver<()>,
    /// Live mic-level meter, cloned from the capture handle. Advisory UI data
    /// (the recording overlay's audio-reactive pulse); reading it never
    /// touches the audio path.
    level: Arc<LevelMeter>,
    /// True exactly while a dictation is capturing audio.
    recording: Arc<AtomicBool>,
    control: Arc<lifecycle::RunControl>,
    /// Recovery requests onto the worker's input channel. Taken and dropped
    /// first in `Drop`: while this sender lives the worker's `recv` cannot
    /// disconnect, and the join below would wait out its whole grace.
    commands: Option<mpsc::Sender<input::WorkerInput>>,
    _capture: hark_audio::CaptureHandle,
}

impl PipelineHandle {
    /// Ask the worker to transcribe again the one clip it kept after a failed
    /// transcription. Fire and forget: the answer arrives as
    /// [`PipelineEvent::Recovered`] (text, never pasted), or as
    /// `Failed` + [`PipelineEvent::ClipHeld`] when it fails or the worker is
    /// busy, or as `ClipHeld { held: false }` when there is nothing to retry.
    pub fn retry_held_clip(&self) {
        self.send_command(input::WorkerInput::RetryHeld);
    }

    /// Drop the held clip. No reply: the UI has already let go of it.
    pub fn dismiss_held_clip(&self) {
        self.send_command(input::WorkerInput::DismissHeld);
    }

    fn send_command(&self, command: input::WorkerInput) {
        if let Some(commands) = &self.commands {
            // Only fails once the worker is gone, when there is nothing to ask.
            let _ = commands.send(command);
        }
    }

    /// Hand the settings recorder the listener's raw key stream. The hook stays
    /// exactly where it is — the pipeline is not stopped and no second hook is
    /// installed — so recording rides the code path dictation already proves
    /// works, backed up by a scanner that reads real key state. `None` if there
    /// is no listener to tap.
    pub fn arm_capture(&self) -> Option<std::sync::Arc<hark_hotkey::CaptureTap>> {
        self.listener.as_ref()?.arm_capture()
    }

    /// Give the stream back to the chord tracker.
    pub fn disarm_capture(&self) {
        if let Some(listener) = self.listener.as_ref() {
            listener.disarm_capture();
        }
    }

    /// Drain the edges either source has produced since the last call.
    pub fn drain_capture(&self, f: impl FnMut(hark_hotkey::CaptureEvent)) {
        if let Some(listener) = self.listener.as_ref() {
            listener.drain_capture(f);
        }
    }

    /// The live input-level meter for UI feedback. Cheap to clone.
    pub fn level_meter(&self) -> Arc<LevelMeter> {
        self.level.clone()
    }

    /// Whether a dictation is capturing right now — the same fact
    /// [`PipelineEvent::Recording`] carries, published as state instead of as
    /// an event.
    ///
    /// The event lane is the truth for anything that has to *react* (status,
    /// tray, history). This exists for the one consumer that must be able to
    /// read the answer without a UI pass having happened: the recording
    /// overlay, which is a second OS window painting on its own schedule and
    /// which must be able to take itself off screen the moment a dictation
    /// ends, even when the main window has not yet run the pass that would
    /// retire it. Cheap to clone, relaxed on both ends: like the level meter,
    /// this is advisory UI state, never ordered against the audio.
    pub fn recording_flag(&self) -> Arc<AtomicBool> {
        self.recording.clone()
    }
}

impl Drop for PipelineHandle {
    fn drop(&mut self) {
        self.control.cancel();
        drop(self.commands.take());
        // Stop the hook thread; its exit drops the sender, which ends the
        // worker's receive loop.
        drop(self.listener.take());
        if let Some(w) = self.worker.take() {
            if !join_within(w, &self.worker_done, WORKER_SHUTDOWN_GRACE) {
                // A late provider result cannot start further work or inject.
                // An already admitted paste may finish restoring its clipboard.
                log::warn!(
                    "pipeline worker still inside a dictation after {} ms; not waiting for it",
                    WORKER_SHUTDOWN_GRACE.as_millis()
                );
            }
        }
        // _capture drops last, stopping the audio stream.
    }
}

/// Join `worker` if it finishes within `grace`, else leave it running.
/// `done` must be the receiver whose sender the thread drops on exit. Returns
/// whether the thread was joined.
fn join_within(
    worker: std::thread::JoinHandle<()>,
    done: &mpsc::Receiver<()>,
    grace: Duration,
) -> bool {
    match done.recv_timeout(grace) {
        Err(mpsc::RecvTimeoutError::Timeout) => false,
        // Disconnected: the thread has returned, so this join is immediate.
        _ => {
            let _ = worker.join();
            true
        }
    }
}

/// Build and start the whole pipeline: shared HTTP client, provider adapter,
/// continuous audio capture, the keyboard hook, and the worker thread.
/// Blocks only until the pieces are up (the pre-warm request runs on the
/// worker thread so startup stays fast).
pub fn run(
    settings: &Settings,
    api_key: String,
    events: mpsc::Sender<PipelineEvent>,
) -> Result<PipelineHandle, PipelineError> {
    run_inner(settings, api_key, events, None)
}

/// Start dictation with an externally owned shortcut source. The desktop app
/// uses this to share one hook with meetings, even when dictation is unavailable.
/// The caller must close the input sender before dropping the returned handle.
pub fn run_with_input(
    settings: &Settings,
    api_key: String,
    events: mpsc::Sender<PipelineEvent>,
    input: mpsc::Receiver<hark_hotkey::PttEvent>,
) -> Result<PipelineHandle, PipelineError> {
    run_inner(settings, api_key, events, Some(input))
}

fn run_inner(
    settings: &Settings,
    api_key: String,
    events: mpsc::Sender<PipelineEvent>,
    input: Option<mpsc::Receiver<hark_hotkey::PttEvent>>,
) -> Result<PipelineHandle, PipelineError> {
    let chord = hark_hotkey::PttChord::parse(&settings.hotkey.ptt_key)
        .map_err(hark_hotkey::HotkeyError::from)?;
    let client = hark_stt::shared_client()?;
    // Before provider_config consumes the key: the inherit path reuses it.
    let cleanup = build_cleanup(settings, client.clone(), &api_key);
    let provider_cfg = provider_config(settings, api_key)?;
    // The streaming adapter keeps `provider_cfg`; the batch one, which
    // carries every dictation the stream does not, takes the batch route.
    let fallback_cfg = fallback_config(&settings.provider, &provider_cfg);
    if fallback_cfg.kind != provider_cfg.kind {
        log::info!(
            "{} streams to {}; a dictation the stream cannot carry goes to batch {}",
            provider_cfg.label,
            provider_cfg.model,
            fallback_cfg.model
        );
    }
    let prewarm_url = provider_cfg.base_url.clone();

    let local = build_local(settings);
    let mode = settings.local_stt.mode;

    // Primary mode never contacts a provider, so it must not require a key
    // (that is the whole point of "I don't want to use a cloud model").
    let provider = if mode.uses_cloud() {
        // A ready local fallback earns the cloud a *shorter* leash: the full
        // 15 s timeout plus on-device decoding would make a rescued dictation
        // slower than no fallback at all.
        let fallback_armed = mode == hark_config::LocalMode::Fallback
            && local.as_ref().is_some_and(|p| p.is_ready());
        if fallback_armed {
            let leash_ms = settings.local_stt.fallback_after_ms;
            log::info!("on-device fallback armed; cloud requests capped at {leash_ms} ms");
            // The adapter sets a per-request timeout, which replaces the
            // client's. Building it with the same fixed budget is what keeps
            // the leash from being stretched back out to the clip-length one.
            Some(hark_stt::build_with_total_timeout(
                &fallback_cfg,
                hark_stt::client_with_timeout(leash_ms)?,
                leash_ms,
            )?)
        } else {
            Some(hark_stt::build(&fallback_cfg, client.clone())?)
        }
    } else {
        log::info!("local STT is primary; no cloud provider will be contacted");
        None
    };

    let window = window_params(&settings.audio);
    let (capture, consumer) = hark_audio::start(
        hark_audio::window::ring_seconds(&window),
        settings.audio.input_device.clone(),
    )?;
    let sample_rate = capture.sample_rate();
    let level = capture.level_meter();
    let discontinuities = capture.discontinuities();
    log::info!(
        "capture live at {sample_rate} Hz; ptt chord: {chord}; provider: {}",
        provider_cfg.label
    );

    let (listener, ptt_rx) = match input {
        Some(input) => (None, input),
        None => {
            let (ptt_tx, ptt_rx) = mpsc::channel();
            let listener =
                hark_hotkey::spawn_listener(chord, settings.hotkey.swallow_lock_keys, ptt_tx)?;
            (Some(listener), ptt_rx)
        }
    };

    let recording = Arc::new(AtomicBool::new(false));
    let control = Arc::new(lifecycle::RunControl::default());
    let (ptt_rx, commands) = input::route(ptt_rx, consumer.reader(), control.clone())
        .map_err(|e| PipelineError::Config(format!("cannot start dictation input: {e}")))?;
    let w = worker::Worker {
        consumer,
        sample_rate,
        window,
        inject: inject_settings(&settings.inject),
        provider,
        // Only ever consulted while a key is held; a provider without a
        // streaming path simply leaves this None and nothing changes.
        live: live_adapter(mode, &provider_cfg, hark_stt::build_live),
        cloud_label: provider_cfg.label.clone(),
        local,
        corrector: hark_spellbook::Corrector::new(&settings.spellbook.corrector_entries()),
        expander: build_expander(&settings.invocations),
        cleanup,
        prewarm_url,
        client,
        stream_model: provider_cfg.model.clone(),
        batch_model: fallback_cfg.model.clone(),
        strip_single_word_period: settings.output.strip_single_word_period,
        track_apps: settings.insights.track_apps,
        events,
        recording: recording.clone(),
        control: control.clone(),
        discontinuities,
        held: None,
    };
    let (done_tx, worker_done) = mpsc::channel();
    let worker = std::thread::Builder::new()
        .name("hark-pipeline-worker".to_string())
        .spawn(move || {
            // Dropped on return and on unwind alike, which is the signal.
            let _done: mpsc::Sender<()> = done_tx;
            worker::run(w, ptt_rx)
        })
        .expect("spawning the worker thread cannot fail");

    Ok(PipelineHandle {
        listener,
        worker: Some(worker),
        worker_done,
        level,
        recording,
        control,
        commands: Some(commands),
        _capture: capture,
    })
}

#[cfg(test)]
mod tests;

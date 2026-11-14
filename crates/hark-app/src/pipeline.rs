//! Pipeline lifecycle as seen from the UI: start/stop, the advisory event
//! stream, and the status the footer renders. The pipeline itself runs on
//! worker threads (hark-pipeline); this module never blocks the UI thread.

use crate::storage::{self, RecordPolicy, StorageCmd};
use crate::ui::history::recovery::{self, Recovery, Request};
use hark_config::Settings;
use hark_pipeline::{FailStage, LevelMeter, PipelineEvent, PipelineHandle};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;

/// What the status footer shows. There is no silent dead state: the pipeline
/// is either listening (and says for what), mid-dictation, or stopped with a
/// visible cause.
#[derive(Clone, PartialEq, Eq)]
pub enum PipelineStatus {
    /// Running; waiting for the chord.
    Idle,
    /// Chord held; capturing audio.
    Recording,
    /// Chord released; request in flight.
    Processing,
    /// Reading the on-device model into RAM. Only the first dictation after a
    /// pipeline start sees this (the engine then stays resident), but it lasts
    /// seconds, so it gets its own state rather than looking like a hang.
    LoadingModel,
    /// Running, but the last dictation failed. Sticky until the next
    /// dictation starts.
    Errored { detail: String, key_related: bool },
    /// Running, and the last dictation ended in something worth saying out
    /// loud but which is not a failure: we captured nothing loud enough to be
    /// speech. Distinct from `Errored` because nothing is broken and the tray
    /// must not go red — but distinct from `Idle` because silently doing
    /// nothing is precisely the experience that leaves users convinced the app
    /// cannot hear them. Sticky until the next dictation starts.
    Hint { detail: String },
    /// Not running (no key, startup failure, or config error).
    Stopped { detail: String, key_related: bool },
}

/// Owns the `PipelineHandle` and the UI-side end of the event stream.
pub struct PipelineController {
    // The app owns the one hook independently of dictation startup. Keep it
    // before the worker handle so implicit destruction also closes input first.
    listener: Option<hark_hotkey::ListenerHandle>,
    handle: Option<PipelineHandle>,
    meeting_toggles: Option<Receiver<()>>,
    events: Option<Receiver<PipelineEvent>>,
    status: PipelineStatus,
    /// Successful dictations this app session (survives restarts; the Get
    /// Started card retires on the first one).
    injected: u64,
    /// Command lane to the storage thread; `None` when storage failed to
    /// open (dictation still works, nothing persists).
    storage: Option<Sender<StorageCmd>>,
    /// Live mic-level meter for the recording overlay's audio-reactive pulse;
    /// `None` while the pipeline is stopped. Read every frame the overlay
    /// paints, so it is a cheap `Arc`, not a per-frame lookup.
    level: Option<Arc<LevelMeter>>,
    /// The pipeline's live "a dictation is capturing" flag, read by the overlay
    /// so it can leave the screen without waiting for a UI pass. `None` while
    /// the pipeline is stopped.
    recording: Option<Arc<AtomicBool>>,
    feedback: Option<Arc<crate::overlay::Feedback>>,
    /// A push-to-talk key another program is intercepting (config name, e.g.
    /// "F12"). Outlives the status, which the next dictation replaces: the
    /// conflict is still there. Cleared when the pipeline restarts, which is
    /// what saving a new shortcut does.
    shortcut_warning: Option<String>,
    /// The last dictation whose words did not reach the cursor, shared with the
    /// History page through the egui context. This side keeps it in step with
    /// the event stream and carries the page's retry/dismiss requests to the
    /// pipeline, which only this controller can reach.
    recovery: Arc<Recovery>,
}

impl PipelineController {
    pub fn new(storage: Option<Sender<StorageCmd>>) -> Self {
        PipelineController {
            listener: None,
            handle: None,
            meeting_toggles: None,
            events: None,
            status: PipelineStatus::Stopped {
                detail: "Not started".to_string(),
                key_related: false,
            },
            injected: 0,
            storage,
            level: None,
            recording: None,
            feedback: None,
            shortcut_warning: None,
            recovery: Arc::new(Recovery::default()),
        }
    }

    pub fn status(&self) -> &PipelineStatus {
        &self.status
    }

    pub fn shortcut_warning(&self) -> Option<&str> {
        self.shortcut_warning.as_deref()
    }

    pub fn is_running(&self) -> bool {
        self.handle.is_some()
    }

    /// The live mic-level meter, or `None` while the pipeline is stopped. The
    /// recording overlay reads it every frame to drive its audio-reactive
    /// pulse; the `Arc` is cheap to clone into the overlay's paint closure.
    pub fn level_meter(&self) -> Option<Arc<LevelMeter>> {
        self.level.clone()
    }

    /// The pipeline's live capture flag, or `None` while it is stopped. The
    /// overlay reads this on its own passes; the UI's own notion of "recording"
    /// stays [`PipelineStatus`], which the event lane drives.
    pub fn recording_flag(&self) -> Option<Arc<AtomicBool>> {
        self.recording.clone()
    }

    pub fn overlay_feedback(&self) -> Option<Arc<crate::overlay::Feedback>> {
        self.feedback.clone()
    }

    pub fn injected_count(&self) -> u64 {
        self.injected
    }

    /// Resolve the key and start the pipeline. On failure the app keeps
    /// running with a visible cause in the footer.
    pub fn start(&mut self, settings: &Settings, ctx: &egui::Context) {
        self.stop();
        recovery::register(ctx, &self.recovery);
        if let Err(e) = self.start_shared(settings, ctx) {
            self.mark_stopped(e);
        }
    }

    fn start_shared(&mut self, settings: &Settings, ctx: &egui::Context) -> Result<(), String> {
        settings
            .validate_meeting_shortcut()
            .map_err(|e| e.to_string())?;
        let chord =
            hark_hotkey::PttChord::parse(&settings.hotkey.ptt_key).map_err(|e| e.to_string())?;
        let meeting = meeting_chord(settings)?;
        let (input_tx, input_rx) = mpsc::channel();
        self.start_worker(settings, ctx, input_rx);
        let (shortcut_tx, shortcut_rx) = mpsc::channel();
        self.listener = Some(
            hark_hotkey::spawn_shared_listener(
                chord,
                // No dictation worker means no visible recording to justify
                // suppressing its lock key. The meeting chord always observes.
                settings.hotkey.swallow_lock_keys && self.handle.is_some(),
                meeting,
                shortcut_tx,
            )
            .map_err(|e| e.to_string())?,
        );
        self.meeting_toggles = Some(
            spawn_shortcut_pump(shortcut_rx, input_tx, ctx.clone())
                .map_err(|e| format!("Cannot route keyboard shortcuts: {e}"))?,
        );
        Ok(())
    }

    fn start_worker(
        &mut self,
        settings: &Settings,
        ctx: &egui::Context,
        input: Receiver<hark_hotkey::PttEvent>,
    ) {
        // Every (re)start is a policy application point: retention changed
        // in a save takes effect now, and app startup prunes old entries
        // before the first dictation.
        if let Some(tx) = &self.storage {
            let _ = tx.send(StorageCmd::Prune(storage::retention(settings)));
        }
        let provider = settings.provider.kind.label();
        // Primary-mode local STT contacts no provider, so a missing key must
        // not stop the pipeline — running keyless is the entire point of that
        // mode. Every other mode still needs one.
        let api_key = match hark_keychain::resolve_key(provider) {
            Ok(k) => k,
            Err(_) if !settings.local_stt.mode.uses_cloud() => String::new(),
            Err(e) => {
                // Missing/unreadable key: by construction key-related.
                self.status = PipelineStatus::Stopped {
                    detail: e.to_string(),
                    key_related: true,
                };
                return;
            }
        };
        #[cfg(target_os = "macos")]
        if !crate::macos::microphone_ready() {
            self.mark_stopped("Allow Microphone access in Settings → General → Permissions, then retry dictation.".into());
            return;
        }
        let (tx, rx) = mpsc::channel();
        match hark_pipeline::run_with_input(settings, api_key, tx, input) {
            Ok(handle) => {
                // The record policy travels with this pipeline run: settings
                // changes restart the pipeline, so the pump never needs a
                // shared, mutable view of the config.
                let tee = self
                    .storage
                    .clone()
                    .map(|tx| (tx, storage::record_policy(settings)));
                self.level = Some(handle.level_meter());
                self.recording = Some(handle.recording_flag());
                self.handle = Some(handle);
                let feedback = Arc::new(crate::overlay::Feedback::new());
                self.events = Some(spawn_repaint_pump(
                    rx,
                    ctx.clone(),
                    tee,
                    Some(feedback.clone()),
                ));
                self.feedback = Some(feedback);
                self.status = PipelineStatus::Idle;
            }
            Err(e) => {
                self.status = PipelineStatus::Stopped {
                    detail: e.to_string(),
                    key_related: false,
                };
            }
        }
    }

    /// The tap remains available when dictation lacks a provider key. It
    /// bypasses both shortcuts on the same hook while the recorder is armed.
    pub fn arm_capture(&self) -> Option<Arc<hark_hotkey::CaptureTap>> {
        self.listener.as_ref()?.arm_capture()
    }

    pub fn disarm_capture(&self) {
        if let Some(handle) = self.listener.as_ref() {
            handle.disarm_capture();
        }
    }

    pub fn drain_capture(&self, f: impl FnMut(hark_hotkey::CaptureEvent)) {
        if let Some(handle) = self.listener.as_ref() {
            handle.drain_capture(f);
        }
    }

    /// Drop the handle; its Drop stops hook -> worker -> capture in order.
    /// The worker's sender drops with it, which ends the repaint pump.
    pub fn stop(&mut self) {
        if let Some(feedback) = self.feedback.take() {
            feedback.disable();
        }
        // The router releases its worker sender after the native listener
        // closes. A meeting lane can never keep dictation shutdown waiting.
        self.listener = None;
        self.meeting_toggles = None;
        self.handle = None;
        self.events = None;
        self.level = None;
        self.recording = None;
        self.shortcut_warning = None;
        // A held clip lived in the worker the line above dropped.
        self.recovery.pipeline_stopped();
    }

    pub fn drain_meeting_toggles(&self) -> usize {
        self.meeting_toggles
            .as_ref()
            .map_or(0, |rx| rx.try_iter().count())
    }

    /// Stop (if running) and surface a non-key cause in the footer, e.g. a
    /// config file that failed to parse.
    pub fn mark_stopped(&mut self, detail: String) {
        self.stop();
        self.status = PipelineStatus::Stopped {
            detail,
            key_related: false,
        };
    }

    /// Drain pending events; called from `App::logic` every frame. Never
    /// blocks: `try_recv` only. An `Errored` status is naturally sticky: it
    /// stands until the next event (the next dictation) replaces it.
    pub fn drain_events(&mut self) {
        self.forward_recovery_request();
        let Some(rx) = &self.events else { return };
        while let Ok(event) = rx.try_recv() {
            if matches!(event, PipelineEvent::Injected(_)) {
                self.injected += 1;
            }
            self.recovery.observe(&event);
            if let PipelineEvent::ShortcutIntercepted { key } = &event {
                self.shortcut_warning = Some(key.clone());
            }
            let Some(next) = next_status(event) else {
                continue;
            };
            // One line per transition (labels only, no detail text). This is
            // the UI half of a dictation in the log: if the pipeline's own
            // lines are there and these are not, the window never ran a pass
            // to notice — the shape of a stranded recording overlay.
            if next != self.status {
                log::info!("status: {} -> {}", label(&self.status), label(&next));
            }
            self.status = next;
        }
    }

    /// Pass the History banner's click to the pipeline. The page cannot reach
    /// the handle, and a click repaints the window, so the next pass of
    /// `App::logic` (which calls `drain_events`) is prompt enough.
    fn forward_recovery_request(&mut self) {
        match (self.recovery.take_request(), &self.handle) {
            (Some(Request::Retry), Some(handle)) => handle.retry_held_clip(),
            (Some(Request::Dismiss), Some(handle)) => handle.dismiss_held_clip(),
            // No pipeline means no clip: let the banner go rather than spin.
            (Some(_), None) => self.recovery.pipeline_stopped(),
            (None, _) => {}
        }
    }

    /// The shared slot behind the History banner.
    #[cfg(test)]
    fn recovery(&self) -> &Recovery {
        &self.recovery
    }
}

/// Carry the STT key across a schema migration that moved it to a new
/// keychain account: the retired Groq preset now reads `openai-compatible`,
/// and the account is the kind's label.
///
/// Owed while the STT provider is `openai-compatible` on Groq's endpoint
/// ([`hark_config::Provider::groq_key_carry`]), or when this load has just
/// converted a Groq table (which may name a proxy instead). A condition, not a
/// once-only step: the file is already v7 when this runs, so a keychain that
/// failed on the migrating launch would otherwise leave the key stranded under
/// `groq`. The copy fills only an empty slot from a stored key, so asking on a
/// later launch costs one or two keychain reads and changes nothing once done.
///
/// Called from `load_settings`, once at startup and before the first pipeline
/// start resolves the key; never per frame. Never overwrites a stored key,
/// and logs account labels only.
pub(crate) fn carry_migrated_key(settings: &Settings, migrations: &hark_config::Migrations) {
    carry_key_with(settings, migrations, hark_keychain::copy_key_if_empty);
}

/// [`carry_migrated_key`] over an injected copy, so the decision is testable
/// without a real keychain.
fn carry_key_with(
    settings: &Settings,
    migrations: &hark_config::Migrations,
    copy: impl FnOnce(&str, &str) -> Result<hark_keychain::CopyOutcome, hark_keychain::KeyError>,
) {
    use hark_keychain::CopyOutcome;
    let Some((from, to)) = migrations
        .key_copy()
        .or_else(|| settings.provider.groq_key_carry())
    else {
        return;
    };
    let just_migrated = migrations.stt_from_groq;
    match copy(from, to) {
        Ok(CopyOutcome::Copied) => {
            log::info!("config migration: copied the \"{from}\" keychain key into \"{to}\"")
        }
        Ok(CopyOutcome::DestinationOccupied) if just_migrated => log::warn!(
            "config migration: \"{to}\" already holds a key, so the \"{from}\" key was not \
             copied over it; store the right key in Settings if dictation reports it rejected"
        ),
        Ok(CopyOutcome::SourceMissing) if just_migrated => {
            log::info!("config migration: no \"{from}\" key stored, nothing to copy")
        }
        // A later launch: carried already, or there was never a key to carry.
        Ok(CopyOutcome::DestinationOccupied | CopyOutcome::SourceMissing) => {
            log::debug!("config migration: no \"{from}\" key left to carry into \"{to}\"")
        }
        Err(e) => log::warn!(
            "config migration: could not copy the \"{from}\" key ({e}); trying again next launch"
        ),
    }
}

fn meeting_chord(settings: &Settings) -> Result<Option<hark_hotkey::PttChord>, String> {
    if !settings.meeting.enabled || !hark_pipeline::meeting::meetings_supported() {
        return Ok(None);
    }
    settings
        .meeting
        .toggle_key
        .as_deref()
        .map(hark_hotkey::PttChord::parse)
        .transpose()
        .map_err(|e| e.to_string())
}

/// Dispatch outside the hook. A missing/busy dictation worker has no effect
/// on meeting toggles; each toggle wakes the root UI even while it is hidden.
fn spawn_shortcut_pump(
    rx: Receiver<hark_hotkey::ShortcutEvent>,
    dictation: Sender<hark_hotkey::PttEvent>,
    ctx: egui::Context,
) -> std::io::Result<Receiver<()>> {
    let (meetings, ui_rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("hark-shortcut-dispatch".into())
        .spawn(move || {
            for event in rx {
                match event {
                    hark_hotkey::ShortcutEvent::Dictation(event) => {
                        let _ = dictation.send(event);
                    }
                    hark_hotkey::ShortcutEvent::MeetingToggle => {
                        if meetings.send(()).is_err() {
                            break;
                        }
                        crate::app::wake_ui(&ctx);
                    }
                }
            }
        })?;
    Ok(ui_rx)
}

/// Variant name only: the detail strings belong to the UI, and the pipeline
/// already logs its own causes.
fn label(status: &PipelineStatus) -> &'static str {
    match status {
        PipelineStatus::Idle => "idle",
        PipelineStatus::Recording => "recording",
        PipelineStatus::Processing => "processing",
        PipelineStatus::LoadingModel => "loading-model",
        PipelineStatus::Errored { .. } => "errored",
        PipelineStatus::Hint { .. } => "hint",
        PipelineStatus::Stopped { .. } => "stopped",
    }
}

/// Pure event -> status mapping (the testable seam). `None` for an advisory
/// that says nothing about the dictation in progress and must not move the
/// footer — an interception report lands mid-hold, while it says "Recording".
fn next_status(event: PipelineEvent) -> Option<PipelineStatus> {
    Some(match event {
        PipelineEvent::Recording => PipelineStatus::Recording,
        PipelineEvent::Processing => PipelineStatus::Processing,
        PipelineEvent::LoadingLocalModel => PipelineStatus::LoadingModel,
        // CP4 forwards the record to the storage thread; for now reaching
        // Idle is the whole story the footer needs.
        PipelineEvent::Injected(_) => PipelineStatus::Idle,
        PipelineEvent::ShortcutIntercepted { .. } => return None,
        // The words exist; only the paste failed. The detail stays out of the
        // line: it is an OS error summary, and the useful part is where the
        // text went.
        PipelineEvent::NotPasted { .. } => PipelineStatus::Errored {
            detail: "Couldn't paste. Open History to copy the text.".to_string(),
            key_related: false,
        },
        // Worker bookkeeping about a held clip; `Failed` already moved the
        // footer, and the History banner carries the rest.
        PipelineEvent::ClipHeld { .. } => return None,
        // A retry's text is waiting in History; nothing is in flight now.
        PipelineEvent::Recovered(_) => PipelineStatus::Idle,
        PipelineEvent::Failed { stage, detail } => match stage {
            // A tap on the chord is the user's own doing and needs no reply;
            // an empty transcript means the provider heard nothing to write.
            // A recording whose release went missing was already thrown away
            // by the pipeline; the footer's job is simply to stop saying
            // "Recording" and go back to waiting for the chord.
            FailStage::GatedTooShort | FailStage::EmptyTranscript | FailStage::Abandoned => {
                PipelineStatus::Idle
            }
            // But "we captured nothing loud enough to be speech" is worth
            // saying, with the way to fix it one click away: the usual cause
            // is the wrong input device selected, or one turned down.
            FailStage::GatedTooQuiet => PipelineStatus::Hint {
                detail: "Didn't catch that. Check your microphone.".to_string(),
            },
            // The pipeline keeps this clip in memory, so the footer can say
            // where to go; the key check reads the error, not the pointer.
            FailStage::Transcribe => PipelineStatus::Errored {
                key_related: detail.to_ascii_lowercase().contains("key"),
                detail: format!("{}. Try again from History.", detail.trim_end_matches('.')),
            },
            FailStage::Audio | FailStage::Inject | FailStage::Internal => {
                PipelineStatus::Errored {
                    // Crude but effective: auth errors say "check your API
                    // key", keychain errors name the keychain.
                    key_related: detail.to_ascii_lowercase().contains("key"),
                    detail,
                }
            }
        },
    })
}

/// The storage command an event calls for, if any. A delivered dictation is
/// recorded whatever the capture setting (the counters tick; capture decides
/// whether the text is kept). One that never reached the cursor is a History
/// row and nothing else, so with capture off nothing is stored at all: the
/// banner's Copy still works from memory, which is all that setting allows.
fn storage_command(event: &PipelineEvent, policy: &RecordPolicy) -> Option<StorageCmd> {
    match event {
        PipelineEvent::Injected(record) => Some(StorageCmd::Record {
            record: record.clone(),
            capture: policy.capture,
            retention: policy.retention,
        }),
        PipelineEvent::NotPasted { record, .. } | PipelineEvent::Recovered(record)
            if policy.capture =>
        {
            Some(StorageCmd::RecordUndelivered {
                record: record.clone(),
                retention: policy.retention,
            })
        }
        _ => None,
    }
}

/// Forward pipeline events onto a UI-side channel, waking the event loop per
/// event (`request_repaint` is the sanctioned cross-thread wake-up), and tee
/// records to the storage thread ([`storage_command`]). Both sends are non-blocking
/// (unbounded channels), so the UI lane never waits on storage or vice
/// versa. The injection already happened before the event was emitted, so a
/// DB write can never precede it (CP4 acceptance). Exits when the pipeline
/// drops its sender or the UI drops its receiver; zero idle cost either way.
fn spawn_repaint_pump(
    rx: Receiver<PipelineEvent>,
    ctx: egui::Context,
    storage: Option<(Sender<StorageCmd>, RecordPolicy)>,
    feedback: Option<Arc<crate::overlay::Feedback>>,
) -> Receiver<PipelineEvent> {
    let (tx, ui_rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("hark-ui-event-pump".to_string())
        .spawn(move || {
            while let Ok(event) = rx.recv() {
                if let Some(feedback) = &feedback {
                    feedback.publish(&event);
                    ctx.request_repaint_of(crate::overlay::viewport_id());
                }
                if let Some((storage_tx, policy)) = &storage {
                    if let Some(cmd) = storage_command(&event, policy) {
                        let _ = storage_tx.send(cmd);
                    }
                }
                if tx.send(event).is_err() {
                    break;
                }
                crate::app::wake_ui(&ctx);
            }
        })
        .expect("spawning the event pump thread cannot fail");
    ui_rx
}

#[cfg(test)]
mod tests;

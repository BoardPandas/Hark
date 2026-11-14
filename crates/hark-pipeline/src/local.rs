//! The on-device half of transcription: the resident engine (preloaded when
//! the mode can need it) and the cloud/local policy that decides which one
//! produces a given dictation.
//!
//! The policy is pure enough to unit-test without a network or a model: it
//! is expressed over a [`Transcriber`] trait that the real cloud adapter and
//! the real engine both satisfy, and that the tests fake.

use crate::lifecycle::RunControl;
use hark_config::LocalMode;
use hark_local_stt::{LocalEngine, LocalSttError, ModelSpec};
use hark_stt::{SttError, Transcript};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;

/// Whether to read the model into RAM as soon as the pipeline starts, so the
/// first dictation (Primary) or the first rescue (Backup) does not pay the
/// ~1.2 s cold load. Off never loads anything, and a model that is not on disk
/// has nothing to load.
pub(crate) fn should_preload(mode: LocalMode, model_on_disk: bool) -> bool {
    mode.uses_local() && model_on_disk
}

/// Where the engine is: not requested, being loaded by the preload helper, or
/// resident. Generic over the engine so the wait-for-preload behaviour is
/// testable without a 670 MB model.
enum Slot<E> {
    Empty,
    Loading(Receiver<Result<E, LocalSttError>>),
    Loaded(E),
}

struct EngineSlot<E> {
    slot: Slot<E>,
}

impl<E: Send + 'static> EngineSlot<E> {
    fn new() -> Self {
        EngineSlot { slot: Slot::Empty }
    }

    /// Run `load` on a short-lived helper thread and park its result here for
    /// the next caller. Never joined: the helper owns only a sender, so a
    /// pipeline that stops mid-load just lets the thread finish and drop the
    /// engine. A spawn failure is a warning; the lazy path still works.
    fn start_preload(&mut self, load: impl FnOnce() -> Result<E, LocalSttError> + Send + 'static) {
        if !matches!(self.slot, Slot::Empty) {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("hark-local-preload".into())
            .spawn(move || {
                let _ = tx.send(load());
            });
        match spawned {
            Ok(_) => self.slot = Slot::Loading(rx),
            Err(e) => log::warn!("could not start the on-device model preload ({e})"),
        }
    }

    /// Collect a finished preload without blocking. A failed or vanished
    /// preload is only a warning: the slot goes back to `Empty` and the next
    /// `get_or_load` loads lazily, exactly as if no preload had run.
    fn poll(&mut self) {
        let Slot::Loading(rx) = &self.slot else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(engine)) => self.slot = Slot::Loaded(engine),
            Ok(Err(e)) => {
                log::warn!("on-device model preload failed ({e}); will load on first use");
                self.slot = Slot::Empty;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                log::warn!(
                    "on-device model preload ended without a result; will load on first use"
                );
                self.slot = Slot::Empty;
            }
        }
    }

    fn is_loaded(&self) -> bool {
        matches!(self.slot, Slot::Loaded(_))
    }

    fn is_loading(&self) -> bool {
        matches!(self.slot, Slot::Loading(_))
    }

    /// The engine. Takes the preloaded one if it has arrived, WAITS for one
    /// still loading (a second load would hold two copies of the weights), and
    /// only otherwise runs `load` itself.
    fn get_or_load(
        &mut self,
        load: impl FnOnce() -> Result<E, LocalSttError>,
    ) -> Result<&E, LocalSttError> {
        if let Slot::Loading(rx) = &self.slot {
            match rx.recv() {
                Ok(Ok(engine)) => self.slot = Slot::Loaded(engine),
                Ok(Err(e)) => {
                    log::warn!("on-device model preload failed ({e}); loading now");
                    self.slot = Slot::Empty;
                }
                Err(_) => {
                    log::warn!("on-device model preload ended without a result; loading now");
                    self.slot = Slot::Empty;
                }
            }
        }
        if matches!(self.slot, Slot::Empty) {
            self.slot = Slot::Loaded(load()?);
        }
        match &self.slot {
            Slot::Loaded(engine) => Ok(engine),
            // Both other states were resolved above; an error is safer than a
            // panic on the dictation path if that ever stops being true.
            _ => Err(LocalSttError::EngineUnavailable),
        }
    }
}

/// Everything the worker needs to run the on-device engine, including the
/// engine itself once something has forced it to load.
pub(crate) struct LocalPlan {
    pub mode: LocalMode,
    pub spec: &'static ModelSpec,
    pub dir: PathBuf,
    pub threads: u32,
    /// Resident once loaded, by the startup preload or by first use: reloading
    /// ~670 MB of weights per dictation would dominate release-to-inject
    /// latency. Empty means "not loaded yet", not "unavailable".
    engine: EngineSlot<LocalEngine>,
}

impl LocalPlan {
    pub fn new(mode: LocalMode, spec: &'static ModelSpec, dir: PathBuf, threads: u32) -> Self {
        LocalPlan {
            mode,
            spec,
            dir,
            threads,
            engine: EngineSlot::new(),
        }
    }

    /// Start loading the model in the background when the mode can need it
    /// ([`should_preload`]). Returns at once; the first dictation never waits
    /// for this to *start*, only (briefly) for a load already under way.
    pub fn start_preload(&mut self, control: Arc<RunControl>) {
        if !should_preload(self.mode, self.is_ready()) || control.is_cancelled() {
            return;
        }
        let (spec, dir, threads) = (self.spec, self.dir.clone(), self.threads);
        self.engine.start_preload(move || {
            // A pipeline retired before this thread got going has no use for
            // 670 MB of weights.
            if control.is_cancelled() {
                return Err(LocalSttError::EngineUnavailable);
            }
            LocalEngine::load(spec, &dir, threads)
        });
    }

    /// The engine, loading it if nothing has yet. The load is seconds long, so
    /// callers should have already told the UI something is happening (see
    /// [`LocalPlan::needs_load_wait`]).
    pub fn engine(&mut self) -> Result<&LocalEngine, LocalSttError> {
        let (spec, dir, threads) = (self.spec, &self.dir, self.threads);
        self.engine
            .get_or_load(|| LocalEngine::load(spec, dir, threads))
    }

    /// Whether using the engine now would block on a load: nothing is loaded
    /// and either a preload is still running or a lazy load is next. False when
    /// the preload has already finished, so "loading model" is only announced
    /// for a wait the user actually feels.
    pub fn needs_load_wait(&mut self) -> bool {
        self.engine.poll();
        !self.engine.is_loaded()
    }

    /// Whether the weights are on disk (or already in RAM) right now. Cheap
    /// (metadata only).
    pub fn is_ready(&self) -> bool {
        self.engine.is_loaded()
            || self.engine.is_loading()
            || self.spec.status_in(&self.dir).is_ready()
    }
}

/// Which engine produced a transcript. Drives the dictation record so history
/// never misattributes a line to the cloud provider that did not write it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Source {
    Cloud,
    /// Local ran because the user chose it as their primary engine.
    LocalPrimary,
    /// Local ran because the cloud attempt failed.
    LocalFallback,
}

impl Source {
    /// The `stt_provider` label for the dictation record.
    pub fn label(self, cloud_label: &str) -> String {
        match self {
            Source::Cloud => cloud_label.to_string(),
            Source::LocalPrimary => "local".to_string(),
            Source::LocalFallback => "local (fallback)".to_string(),
        }
    }
}

pub(crate) struct Outcome {
    pub transcript: Transcript,
    pub source: Source,
}

/// The two things the policy can call. Abstracted so the policy is testable
/// without a network or a 670 MB model on disk.
pub(crate) trait Transcriber {
    /// Cloud attempt, retries included. `None` when this mode never calls the
    /// cloud (primary), so the policy cannot accidentally reach for it.
    fn cloud(&mut self) -> Option<Result<Transcript, SttError>>;
    /// On-device attempt, loading the model if needed.
    fn local(&mut self) -> Result<Transcript, LocalSttError>;
}

/// Decide which engine transcribes this dictation.
///
/// - **Primary** — local only. A local failure is the dictation's failure;
///   the cloud is never contacted (there may not even be a key).
/// - **Fallback** — cloud first, then local on *any* cloud error. Auth
///   failures fall back too: a user with a stale key would rather have their
///   words than a diagnostic, and the record still says local produced them.
/// - **Off** — cloud only, exactly as before this feature existed.
///
/// `live_failed` means a live stream already spent this dictation's first
/// attempt and produced nothing. In fallback mode local then goes *first* and
/// the cloud replay becomes the rescue: a stalled Live turn means the provider
/// is struggling right now, and in the same windows its batch endpoint took
/// 4.5-7 s on top of the 3 s finalise already spent, so replaying made a
/// rescued dictation take 8-11 s. A SMART refusal arrives here as a live
/// failure too; the on-device engine is verbatim, so it serves as well as a
/// VERBATIM replay would.
///
/// When both fail, the cloud error is what surfaces: it is the one the user
/// configured and the one they can act on.
pub(crate) fn transcribe(
    mode: LocalMode,
    local_ready: bool,
    live_failed: bool,
    t: &mut dyn Transcriber,
) -> Result<Outcome, SttError> {
    if mode == LocalMode::Primary {
        return t
            .local()
            .map(|transcript| Outcome {
                transcript,
                source: Source::LocalPrimary,
            })
            .map_err(local_as_stt_error);
    }

    if mode == LocalMode::Fallback && local_ready && live_failed {
        log::warn!("live stream failed; transcribing on-device instead of replaying to the cloud");
        match t.local() {
            Ok(transcript) => {
                return Ok(Outcome {
                    transcript,
                    source: Source::LocalFallback,
                })
            }
            Err(e) => log::warn!("on-device decode failed ({e}); replaying to the cloud"),
        }
        return t
            .cloud()
            .unwrap_or_else(|| Err(no_cloud_adapter()))
            .map(|transcript| Outcome {
                transcript,
                source: Source::Cloud,
            });
    }

    let cloud = match t.cloud() {
        Some(result) => result,
        None => return Err(no_cloud_adapter()),
    };

    let cloud_err = match cloud {
        Ok(transcript) => {
            return Ok(Outcome {
                transcript,
                source: Source::Cloud,
            })
        }
        Err(e) => e,
    };

    if mode != LocalMode::Fallback || !local_ready {
        return Err(cloud_err);
    }

    log::warn!("cloud transcription failed ({cloud_err}); falling back to the on-device model");
    match t.local() {
        Ok(transcript) => Ok(Outcome {
            transcript,
            source: Source::LocalFallback,
        }),
        Err(local_err) => {
            // Both engines are down. Report the cloud error: it is the
            // configured path and the actionable one.
            log::error!("on-device fallback also failed: {local_err}");
            Err(cloud_err)
        }
    }
}

/// Mode says cloud, but no cloud adapter exists. Only reachable via a misbuilt
/// worker; treat it as a config error rather than panicking.
fn no_cloud_adapter() -> SttError {
    SttError::Provider {
        provider: "pipeline".to_string(),
        detail: "no cloud provider is configured".to_string(),
    }
}

/// Present an on-device failure through the pipeline's existing error
/// taxonomy so every caller keeps one error type.
fn local_as_stt_error(e: LocalSttError) -> SttError {
    match e {
        LocalSttError::ModelMissing { .. } | LocalSttError::EngineUnavailable => {
            SttError::Provider {
                provider: "local".to_string(),
                detail: e.to_string(),
            }
        }
        other => SttError::Provider {
            provider: "local".to_string(),
            detail: other.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Scripted transcriber: each side yields its next scripted result and
    /// counts how many times it was asked.
    struct Fake {
        cloud: Option<Vec<Result<Transcript, SttError>>>,
        local: Vec<Result<Transcript, LocalSttError>>,
        cloud_calls: u32,
        local_calls: u32,
    }

    impl Fake {
        fn new() -> Self {
            Fake {
                cloud: Some(Vec::new()),
                local: Vec::new(),
                cloud_calls: 0,
                local_calls: 0,
            }
        }
        fn cloud_ok(mut self, text: &str) -> Self {
            self.cloud.get_or_insert_with(Vec::new).push(Ok(ok(text)));
            self
        }
        fn cloud_err(mut self, e: SttError) -> Self {
            self.cloud.get_or_insert_with(Vec::new).push(Err(e));
            self
        }
        fn no_cloud(mut self) -> Self {
            self.cloud = None;
            self
        }
        fn local_ok(mut self, text: &str) -> Self {
            self.local.push(Ok(ok(text)));
            self
        }
        fn local_err(mut self) -> Self {
            self.local.push(Err(LocalSttError::EngineUnavailable));
            self
        }
    }

    fn ok(text: &str) -> Transcript {
        Transcript {
            text: text.to_string(),
            cleaned: None,
            request_ms: 1,
        }
    }

    fn timeout() -> SttError {
        SttError::Timeout {
            provider: "cloud".to_string(),
            configured_ms: 6_000,
        }
    }

    fn auth() -> SttError {
        SttError::Auth {
            provider: "cloud".to_string(),
            status: 401,
            detail: String::new(),
        }
    }

    impl Transcriber for Fake {
        fn cloud(&mut self) -> Option<Result<Transcript, SttError>> {
            let script = self.cloud.as_mut()?;
            self.cloud_calls += 1;
            Some(script.remove(0))
        }
        fn local(&mut self) -> Result<Transcript, LocalSttError> {
            self.local_calls += 1;
            self.local.remove(0)
        }
    }

    fn text_of(outcome: Result<Outcome, SttError>) -> (String, Source) {
        match outcome {
            Ok(o) => (o.transcript.text, o.source),
            Err(e) => panic!("expected a transcript, got {e}"),
        }
    }

    #[test]
    fn off_mode_uses_cloud_and_never_touches_local() {
        let mut f = Fake::new().cloud_ok("from cloud");
        let (text, source) = text_of(transcribe(LocalMode::Off, true, false, &mut f));
        assert_eq!(text, "from cloud");
        assert_eq!(source, Source::Cloud);
        assert_eq!(f.local_calls, 0, "Off must never load the model");
    }

    #[test]
    fn off_mode_surfaces_the_cloud_error_even_when_a_model_is_ready() {
        // A downloaded model must not silently rescue a user who set Off.
        let mut f = Fake::new().cloud_err(timeout()).local_ok("unused");
        let err = transcribe(LocalMode::Off, true, false, &mut f)
            .err()
            .expect("fails");
        assert!(matches!(err, SttError::Timeout { .. }));
        assert_eq!(f.local_calls, 0);
    }

    #[test]
    fn primary_mode_uses_local_and_never_calls_cloud() {
        let mut f = Fake::new().no_cloud().local_ok("from local");
        let (text, source) = text_of(transcribe(LocalMode::Primary, true, false, &mut f));
        assert_eq!(text, "from local");
        assert_eq!(source, Source::LocalPrimary);
        assert_eq!(f.cloud_calls, 0, "primary must not need a cloud key");
    }

    #[test]
    fn primary_mode_failure_is_the_dictations_failure() {
        let mut f = Fake::new().no_cloud().local_err();
        let err = transcribe(LocalMode::Primary, true, false, &mut f)
            .err()
            .expect("a local failure in primary mode fails the dictation");
        assert!(matches!(err, SttError::Provider { .. }));
        assert_eq!(f.cloud_calls, 0);
    }

    #[test]
    fn fallback_prefers_cloud_when_it_works() {
        let mut f = Fake::new().cloud_ok("from cloud").local_ok("unused");
        let (text, source) = text_of(transcribe(LocalMode::Fallback, true, false, &mut f));
        assert_eq!(text, "from cloud");
        assert_eq!(source, Source::Cloud);
        assert_eq!(f.local_calls, 0, "a healthy cloud must not load the model");
    }

    #[test]
    fn fallback_rescues_a_timed_out_dictation() {
        let mut f = Fake::new().cloud_err(timeout()).local_ok("rescued");
        let (text, source) = text_of(transcribe(LocalMode::Fallback, true, false, &mut f));
        assert_eq!(text, "rescued");
        assert_eq!(source, Source::LocalFallback);
    }

    #[test]
    fn fallback_also_rescues_an_auth_failure() {
        // A stale or revoked key is exactly when a user most wants their
        // words to survive; the record still credits local.
        let mut f = Fake::new().cloud_err(auth()).local_ok("rescued");
        let (_, source) = text_of(transcribe(LocalMode::Fallback, true, false, &mut f));
        assert_eq!(source, Source::LocalFallback);
    }

    #[test]
    fn fallback_without_a_downloaded_model_reports_the_cloud_error() {
        let mut f = Fake::new().cloud_err(timeout());
        let err = transcribe(LocalMode::Fallback, false, false, &mut f)
            .err()
            .expect("no model means no rescue");
        assert!(matches!(err, SttError::Timeout { .. }));
        assert_eq!(f.local_calls, 0, "an absent model must never be loaded");
    }

    #[test]
    fn when_both_engines_fail_the_cloud_error_is_what_surfaces() {
        // The cloud error is the actionable one (bad key, no network); a
        // secondary "engine unavailable" would just bury it.
        let mut f = Fake::new().cloud_err(auth()).local_err();
        let err = transcribe(LocalMode::Fallback, true, false, &mut f)
            .err()
            .expect("both down means failure");
        assert!(matches!(err, SttError::Auth { .. }), "got {err}");
        assert_eq!(f.local_calls, 1, "local was genuinely attempted");
    }

    #[test]
    fn a_cloud_mode_worker_with_no_adapter_errors_instead_of_panicking() {
        let mut f = Fake::new().no_cloud();
        let err = transcribe(LocalMode::Off, false, false, &mut f)
            .err()
            .expect("no adapter is an error");
        assert!(matches!(err, SttError::Provider { .. }));
    }

    #[test]
    fn a_failed_live_stream_goes_on_device_without_a_cloud_replay() {
        // The replay to a provider whose Live turn just stalled was the
        // 4.5-7 s half of an 8-11 s dictation.
        let mut f = Fake::new().cloud_ok("unused").local_ok("on-device");
        let (text, source) = text_of(transcribe(LocalMode::Fallback, true, true, &mut f));
        assert_eq!(text, "on-device");
        assert_eq!(source, Source::LocalFallback);
        assert_eq!(
            f.cloud_calls, 0,
            "the stalled provider must not be asked again"
        );
    }

    #[test]
    fn after_a_failed_live_stream_the_cloud_replay_rescues_a_local_failure() {
        let mut f = Fake::new().cloud_ok("replayed").local_err();
        let (text, source) = text_of(transcribe(LocalMode::Fallback, true, true, &mut f));
        assert_eq!(text, "replayed");
        assert_eq!(source, Source::Cloud);
        assert_eq!((f.local_calls, f.cloud_calls), (1, 1));
    }

    #[test]
    fn after_a_failed_live_stream_with_both_engines_down_the_cloud_error_surfaces() {
        let mut f = Fake::new().cloud_err(auth()).local_err();
        let err = transcribe(LocalMode::Fallback, true, true, &mut f)
            .err()
            .expect("both down means failure");
        assert!(matches!(err, SttError::Auth { .. }), "got {err}");
        assert_eq!(
            f.local_calls, 1,
            "local is tried once, not again after the replay"
        );
    }

    #[test]
    fn a_failed_live_stream_still_replays_to_the_cloud_without_a_model() {
        for (mode, ready) in [(LocalMode::Off, true), (LocalMode::Fallback, false)] {
            let mut f = Fake::new().cloud_ok("replayed").local_ok("unused");
            let (text, source) = text_of(transcribe(mode, ready, true, &mut f));
            assert_eq!(text, "replayed", "{mode:?}");
            assert_eq!(source, Source::Cloud);
            assert_eq!(f.local_calls, 0, "{mode:?} must not load the model");
        }
    }

    // --- preload: which modes, and one load only ---

    #[test]
    fn only_backup_and_primary_preload_and_only_with_a_model_on_disk() {
        assert!(should_preload(LocalMode::Fallback, true));
        assert!(should_preload(LocalMode::Primary, true));
        assert!(!should_preload(LocalMode::Off, true), "Off never loads");
        assert!(!should_preload(LocalMode::Fallback, false));
        assert!(!should_preload(LocalMode::Primary, false));
        assert!(!should_preload(LocalMode::Off, false));
    }

    #[test]
    fn the_real_engine_can_cross_to_the_preload_thread() {
        fn assert_send<T: Send + 'static>() {}
        assert_send::<LocalEngine>();
    }

    /// A preload that finishes only when the test opens its gate.
    fn gated_preload(
        slot: &mut EngineSlot<u32>,
        loads: &Arc<std::sync::atomic::AtomicU32>,
        result: Result<u32, LocalSttError>,
    ) -> mpsc::Sender<()> {
        let (open, gate) = mpsc::channel::<()>();
        let loads = loads.clone();
        slot.start_preload(move || {
            loads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let _ = gate.recv();
            result
        });
        open
    }

    #[test]
    fn a_dictation_during_a_preload_waits_for_it_instead_of_loading_twice() {
        use std::sync::atomic::{AtomicU32, Ordering};
        let preloads = Arc::new(AtomicU32::new(0));
        let lazy_loads = Arc::new(AtomicU32::new(0));
        let mut slot = EngineSlot::<u32>::new();
        let open = gated_preload(&mut slot, &preloads, Ok(7));
        assert!(slot.is_loading());
        slot.poll();
        assert!(slot.is_loading(), "still loading: nothing to collect yet");

        let lazy = lazy_loads.clone();
        let dictation = std::thread::spawn(move || {
            let got = *slot
                .get_or_load(|| {
                    lazy.fetch_add(1, Ordering::SeqCst);
                    Ok(99)
                })
                .unwrap();
            (got, slot)
        });
        open.send(()).unwrap();
        let (got, slot) = dictation.join().unwrap();

        assert_eq!(got, 7, "the preloaded engine is the one used");
        assert_eq!(preloads.load(Ordering::SeqCst), 1);
        assert_eq!(lazy_loads.load(Ordering::SeqCst), 0, "no second load");
        assert!(slot.is_loaded());
    }

    #[test]
    fn a_finished_preload_is_collected_without_blocking_or_a_wait_notice() {
        let loads = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let mut slot = EngineSlot::<u32>::new();
        let open = gated_preload(&mut slot, &loads, Ok(5));
        open.send(()).unwrap();
        // Poll until the helper has delivered; bounded so a bug fails fast.
        for _ in 0..500 {
            slot.poll();
            if slot.is_loaded() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(slot.is_loaded(), "no wait is needed once it has arrived");
        assert_eq!(*slot.get_or_load(|| Ok(0)).unwrap(), 5);
    }

    #[test]
    fn a_failed_preload_falls_back_to_a_lazy_load() {
        let loads = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let mut slot = EngineSlot::<u32>::new();
        let open = gated_preload(&mut slot, &loads, Err(LocalSttError::EngineUnavailable));
        open.send(()).unwrap();
        assert_eq!(*slot.get_or_load(|| Ok(11)).unwrap(), 11);
        assert!(slot.is_loaded());
    }

    #[test]
    fn without_a_preload_the_first_use_loads_exactly_once() {
        let mut slot = EngineSlot::<u32>::new();
        assert!(!slot.is_loading() && !slot.is_loaded());
        assert_eq!(*slot.get_or_load(|| Ok(3)).unwrap(), 3);
        assert_eq!(
            *slot.get_or_load(|| Ok(4)).unwrap(),
            3,
            "resident after the first load"
        );
    }

    #[test]
    fn a_preload_never_starts_twice() {
        use std::sync::atomic::{AtomicU32, Ordering};
        let loads = Arc::new(AtomicU32::new(0));
        let mut slot = EngineSlot::<u32>::new();
        let open = gated_preload(&mut slot, &loads, Ok(1));
        slot.start_preload(|| Ok(2));
        open.send(()).unwrap();
        assert_eq!(*slot.get_or_load(|| Ok(3)).unwrap(), 1);
        assert_eq!(loads.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn source_labels_name_the_engine_that_actually_ran() {
        assert_eq!(Source::Cloud.label("deepgram"), "deepgram");
        assert_eq!(Source::LocalPrimary.label("deepgram"), "local");
        assert_eq!(Source::LocalFallback.label("deepgram"), "local (fallback)");
    }
}

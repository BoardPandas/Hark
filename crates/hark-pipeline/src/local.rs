//! The on-device half of transcription: the lazily-loaded engine and the
//! cloud/local policy that decides which one produces a given dictation.
//!
//! The policy is pure enough to unit-test without a network or a model: it
//! is expressed over a [`Transcriber`] trait that the real cloud adapter and
//! the real engine both satisfy, and that the tests fake.

use hark_config::LocalMode;
use hark_local_stt::{LocalEngine, LocalSttError, ModelSpec};
use hark_stt::{SttError, Transcript};
use std::path::PathBuf;

/// Everything the worker needs to run the on-device engine, including the
/// engine itself once something has forced it to load.
pub(crate) struct LocalPlan {
    pub mode: LocalMode,
    pub spec: &'static ModelSpec,
    pub dir: PathBuf,
    pub threads: u32,
    /// Loaded on first use and then kept resident: reloading ~670 MB of
    /// weights per dictation would dominate release-to-inject latency.
    /// `None` means "not loaded yet", not "unavailable".
    engine: Option<LocalEngine>,
}

impl LocalPlan {
    pub fn new(mode: LocalMode, spec: &'static ModelSpec, dir: PathBuf, threads: u32) -> Self {
        LocalPlan {
            mode,
            spec,
            dir,
            threads,
            engine: None,
        }
    }

    /// The engine, loading it if this is the first call. The load is seconds
    /// long, so callers should have already told the UI something is
    /// happening.
    pub fn engine(&mut self) -> Result<&LocalEngine, LocalSttError> {
        if self.engine.is_none() {
            let engine = LocalEngine::load(self.spec, &self.dir, self.threads)?;
            self.engine = Some(engine);
        }
        // Just populated above (or already present), so this cannot fail.
        Ok(self
            .engine
            .as_ref()
            .expect("engine was just loaded into place"))
    }

    /// Whether the weights are on disk right now. Cheap (metadata only).
    pub fn is_ready(&self) -> bool {
        self.engine.is_some() || self.spec.status_in(&self.dir).is_ready()
    }

    /// Whether the model has already been loaded into RAM.
    pub fn is_loaded(&self) -> bool {
        self.engine.is_some()
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
/// When both fail, the cloud error is what surfaces: it is the one the user
/// configured and the one they can act on.
pub(crate) fn transcribe(
    mode: LocalMode,
    local_ready: bool,
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

    let cloud = match t.cloud() {
        Some(result) => result,
        // Mode says cloud, but no cloud adapter exists. Only reachable via a
        // misbuilt worker; treat it as a config error rather than panicking.
        None => {
            return Err(SttError::Provider {
                provider: "pipeline".to_string(),
                detail: "no cloud provider is configured".to_string(),
            })
        }
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
        let (text, source) = text_of(transcribe(LocalMode::Off, true, &mut f));
        assert_eq!(text, "from cloud");
        assert_eq!(source, Source::Cloud);
        assert_eq!(f.local_calls, 0, "Off must never load the model");
    }

    #[test]
    fn off_mode_surfaces_the_cloud_error_even_when_a_model_is_ready() {
        // A downloaded model must not silently rescue a user who set Off.
        let mut f = Fake::new().cloud_err(timeout()).local_ok("unused");
        let err = transcribe(LocalMode::Off, true, &mut f)
            .err()
            .expect("fails");
        assert!(matches!(err, SttError::Timeout { .. }));
        assert_eq!(f.local_calls, 0);
    }

    #[test]
    fn primary_mode_uses_local_and_never_calls_cloud() {
        let mut f = Fake::new().no_cloud().local_ok("from local");
        let (text, source) = text_of(transcribe(LocalMode::Primary, true, &mut f));
        assert_eq!(text, "from local");
        assert_eq!(source, Source::LocalPrimary);
        assert_eq!(f.cloud_calls, 0, "primary must not need a cloud key");
    }

    #[test]
    fn primary_mode_failure_is_the_dictations_failure() {
        let mut f = Fake::new().no_cloud().local_err();
        let err = transcribe(LocalMode::Primary, true, &mut f)
            .err()
            .expect("a local failure in primary mode fails the dictation");
        assert!(matches!(err, SttError::Provider { .. }));
        assert_eq!(f.cloud_calls, 0);
    }

    #[test]
    fn fallback_prefers_cloud_when_it_works() {
        let mut f = Fake::new().cloud_ok("from cloud").local_ok("unused");
        let (text, source) = text_of(transcribe(LocalMode::Fallback, true, &mut f));
        assert_eq!(text, "from cloud");
        assert_eq!(source, Source::Cloud);
        assert_eq!(f.local_calls, 0, "a healthy cloud must not load the model");
    }

    #[test]
    fn fallback_rescues_a_timed_out_dictation() {
        let mut f = Fake::new().cloud_err(timeout()).local_ok("rescued");
        let (text, source) = text_of(transcribe(LocalMode::Fallback, true, &mut f));
        assert_eq!(text, "rescued");
        assert_eq!(source, Source::LocalFallback);
    }

    #[test]
    fn fallback_also_rescues_an_auth_failure() {
        // A stale or revoked key is exactly when a user most wants their
        // words to survive; the record still credits local.
        let mut f = Fake::new().cloud_err(auth()).local_ok("rescued");
        let (_, source) = text_of(transcribe(LocalMode::Fallback, true, &mut f));
        assert_eq!(source, Source::LocalFallback);
    }

    #[test]
    fn fallback_without_a_downloaded_model_reports_the_cloud_error() {
        let mut f = Fake::new().cloud_err(timeout());
        let err = transcribe(LocalMode::Fallback, false, &mut f)
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
        let err = transcribe(LocalMode::Fallback, true, &mut f)
            .err()
            .expect("both down means failure");
        assert!(matches!(err, SttError::Auth { .. }), "got {err}");
        assert_eq!(f.local_calls, 1, "local was genuinely attempted");
    }

    #[test]
    fn a_cloud_mode_worker_with_no_adapter_errors_instead_of_panicking() {
        let mut f = Fake::new().no_cloud();
        let err = transcribe(LocalMode::Off, false, &mut f)
            .err()
            .expect("no adapter is an error");
        assert!(matches!(err, SttError::Provider { .. }));
    }

    #[test]
    fn source_labels_name_the_engine_that_actually_ran() {
        assert_eq!(Source::Cloud.label("deepgram"), "deepgram");
        assert_eq!(Source::LocalPrimary.label("deepgram"), "local");
        assert_eq!(Source::LocalFallback.label("deepgram"), "local (fallback)");
    }
}

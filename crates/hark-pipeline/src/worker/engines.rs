//! The engines one clip is transcribed with: the cloud adapter and its
//! single retry, and the on-device engine, bound to the policy in
//! [`crate::local`].

use super::Worker;
use crate::events::PipelineEvent;
use crate::local::{LocalPlan, Outcome, Transcriber};
use crate::retry::should_retry;
use hark_stt::{SttError, SttProvider, Transcript};
use std::sync::mpsc::Sender;

/// Transcribe one clip under the normal policy: cloud, then the on-device
/// fallback when the mode allows it. `live_failed` means a live stream already
/// spent this clip's first attempt; `may_retry` is whether the cloud request
/// gets its single retry.
pub(super) fn transcribe_clip(
    worker: &mut Worker,
    samples: &[f32],
    wav: &[u8],
    live_failed: bool,
    may_retry: bool,
) -> Result<Outcome, SttError> {
    let events = worker.events.clone();
    let mode = worker
        .local
        .as_ref()
        .map_or(hark_config::LocalMode::Off, |p| p.mode);
    let local_ready = worker.local.as_ref().is_some_and(|p| p.is_ready());
    let mut engines = Engines {
        // Disjoint field borrows: `provider` shared, `local` mutable.
        provider: worker.provider.as_deref(),
        wav,
        local: worker.local.as_mut(),
        samples,
        events: &events,
        may_retry,
        control: &worker.control,
    };
    crate::local::transcribe(mode, local_ready, live_failed, &mut engines)
}

/// Binds one dictation's audio to whichever engines this worker has, so the
/// cloud/local policy in [`crate::local`] can stay pure and testable.
struct Engines<'a> {
    provider: Option<&'a dyn SttProvider>,
    /// Empty in primary mode, where no WAV is ever encoded.
    wav: &'a [u8],
    local: Option<&'a mut LocalPlan>,
    samples: &'a [f32],
    events: &'a Sender<PipelineEvent>,
    /// False when a live stream already spent this dictation's first attempt.
    may_retry: bool,
    control: &'a crate::lifecycle::RunControl,
}

impl Transcriber for Engines<'_> {
    fn cloud(&mut self) -> Option<Result<Transcript, SttError>> {
        let provider = self.provider?;
        Some(transcribe_with_retry(
            provider,
            self.wav,
            self.may_retry,
            self.control,
        ))
    }

    fn local(&mut self) -> Result<Transcript, hark_local_stt::LocalSttError> {
        if self.control.is_cancelled() {
            return Err(hark_local_stt::LocalSttError::EngineUnavailable);
        }
        let plan = self
            .local
            .as_mut()
            .ok_or(hark_local_stt::LocalSttError::EngineUnavailable)?;
        // Reading ~670 MB of weights into RAM takes seconds. Say so before
        // blocking, or the app just looks hung. Asked of the plan rather than
        // "is it loaded": a preload that has already finished costs this
        // dictation nothing, and one still running is a wait worth naming.
        if plan.needs_load_wait() {
            let _ = self.events.send(PipelineEvent::LoadingLocalModel);
        }
        let engine = plan.engine()?;
        if self.control.is_cancelled() {
            return Err(hark_local_stt::LocalSttError::EngineUnavailable);
        }
        let decoded = engine.transcribe(self.samples)?;
        Ok(Transcript {
            text: decoded.text,
            // The local engine transcribes only; cleanup stays a separate step.
            cleaned: None,
            request_ms: decoded.request_ms,
        })
    }
}

/// At most one retry, and only when `should_retry` says the failure class
/// warrants it (timeout / connect-class). `may_retry` is false when this call
/// is itself the retry of a live stream that failed.
pub(super) fn transcribe_with_retry(
    provider: &dyn SttProvider,
    wav: &[u8],
    may_retry: bool,
    control: &crate::lifecycle::RunControl,
) -> Result<Transcript, SttError> {
    if control.is_cancelled() {
        return Err(SttError::Provider {
            provider: "pipeline".into(),
            detail: "dictation cancelled".into(),
        });
    }
    match provider.transcribe(wav) {
        Ok(t) => Ok(t),
        Err(e) if may_retry && should_retry(&e) && !control.is_cancelled() => {
            log::warn!("transcription failed ({e}); retrying once");
            provider.transcribe(wav)
        }
        Err(e) => Err(e),
    }
}

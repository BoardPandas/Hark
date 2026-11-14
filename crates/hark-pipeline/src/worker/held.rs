//! The held clip: one dictation's audio kept in memory after its
//! transcription failed for good, and the retry the UI can ask for.

use super::engines::transcribe_clip;
use super::record::{build_record, RecordParts};
use super::shape::shape_transcript;
use super::Worker;
use crate::events::{FailStage, PipelineEvent};
use crate::state::PipelineState;
use std::time::Instant;

/// A dictation's audio, held after transcription failed. Dropped when a later
/// dictation or a retry succeeds, when the user dismisses it, and when the
/// pipeline stops (the worker owns it, so its thread ending frees it). Gated,
/// empty, abandoned and audio-assembly failures never produce one: there is
/// nothing worth transcribing again.
pub(crate) struct HeldClip {
    /// The clip as it went to the engines: 16 kHz mono, already normalised.
    pub(super) samples: Vec<f32>,
    pub(super) audio_ms: u64,
    /// App identity captured at the time, when the user opted in.
    pub(super) foreground_app: Option<String>,
}

/// The UI asked to retry the held clip. Honoured only when the worker is idle
/// and no hold is queued: a retry blocks the worker for as long as a
/// transcription takes, and it must never race a dictation for the cycle.
/// Always answers with an event, so the UI's busy state cannot stick.
pub(super) fn retry_requested(worker: &mut Worker, state: PipelineState) {
    let events = worker.events.clone();
    if worker.held.is_none() {
        let _ = events.send(PipelineEvent::ClipHeld { held: false });
        return;
    }
    if !matches!(state, PipelineState::Idle) || !worker.control.start_cycle() {
        log::info!("retry of the held clip declined: a dictation is in progress");
        let _ = events.send(PipelineEvent::ClipHeld { held: true });
        return;
    }
    let Some(clip) = worker.held.take() else {
        worker.control.finish_cycle();
        return;
    };
    // The chord is refused until this finishes; without this the pill and
    // footer would say "ready" while a press does nothing.
    let _ = events.send(PipelineEvent::Processing);
    let attempt =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| retry_clip(worker, &clip)));
    let recovered = match attempt {
        Ok(recovered) => recovered,
        Err(_) => {
            log::error!("retry of the held clip panicked; the clip is kept");
            let _ = events.send(PipelineEvent::Failed {
                stage: FailStage::Internal,
                detail: "the retry failed unexpectedly".to_string(),
            });
            false
        }
    };
    if !recovered && !worker.control.is_cancelled() {
        worker.held = Some(clip);
        let _ = events.send(PipelineEvent::ClipHeld { held: true });
    }
    worker.control.finish_cycle();
}

/// Run the held clip through transcribe -> spellbook -> invocation -> cleanup
/// and deliver the text as a record that was NOT pasted. The user asked from
/// Hark's own window, so the cursor is no longer where the dictation was
/// aimed; pasting now would put the words in the wrong place. Returns whether
/// text was produced (the clip is then spent).
fn retry_clip(worker: &mut Worker, clip: &HeldClip) -> bool {
    let started = Instant::now();
    let events = worker.events.clone();
    let fail = |stage: FailStage, detail: String| {
        let _ = events.send(PipelineEvent::Failed { stage, detail });
    };
    if worker.control.is_cancelled() {
        return false;
    }
    let wav = if worker.provider.is_some() {
        hark_stt::wav::encode_wav_16k_mono(&clip.samples)
    } else {
        Vec::new()
    };
    // One cloud attempt, then the on-device fallback: the worker refuses
    // push-to-talk for as long as this runs, and the clip already failed a
    // full attempt-plus-retry once.
    let outcome = match transcribe_clip(worker, &clip.samples, &wav, false, false) {
        Ok(outcome) => outcome,
        Err(e) => {
            log::error!("retry transcription failed: {e}");
            fail(FailStage::Transcribe, e.to_string());
            return false;
        }
    };
    if worker.control.is_cancelled() {
        return false;
    }
    if outcome.transcript.text.trim().is_empty() {
        log::info!("retry returned an empty transcript");
        fail(
            FailStage::EmptyTranscript,
            "the provider returned an empty transcript".to_string(),
        );
        return false;
    }
    let Some(shaped) = shape_transcript(worker, &outcome.transcript) else {
        return false;
    };
    let record = build_record(
        worker,
        RecordParts {
            transcript: outcome.transcript,
            cleaned: shaped.cleaned,
            fired: shaped.fired,
            pass_one_replacements: shaped.pass_one_replacements,
            provider_cleaned: shaped.provider_cleaned,
            source: outcome.source,
            delivered_by_stream: false,
            audio_ms: clip.audio_ms,
            total_ms: started.elapsed().as_millis() as u64,
            foreground_app: clip.foreground_app.clone(),
        },
    );
    log::info!(
        "held clip recovered: {} chars; not pasted",
        record.final_text.chars().count()
    );
    let _ = events.send(PipelineEvent::Recovered(Box::new(record)));
    true
}

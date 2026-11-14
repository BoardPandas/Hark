//! The dictation record: what history and stats are told about a dictation,
//! naming the engine, model and voice that actually produced its text.

use super::shape::CleanupOutcome;
use super::Worker;
use crate::events::DictationRecord;
use crate::local::Source;
use hark_stt::Transcript;
use hark_voice::Voice;

pub(super) struct RecordParts {
    pub(super) transcript: Transcript,
    pub(super) cleaned: CleanupOutcome,
    pub(super) fired: Option<String>,
    pub(super) pass_one_replacements: u64,
    pub(super) provider_cleaned: bool,
    pub(super) source: Source,
    pub(super) delivered_by_stream: bool,
    pub(super) audio_ms: u64,
    pub(super) total_ms: u64,
    pub(super) foreground_app: Option<String>,
}

pub(super) fn build_record(worker: &Worker, parts: RecordParts) -> DictationRecord {
    let RecordParts {
        transcript,
        cleaned,
        fired,
        pass_one_replacements,
        provider_cleaned,
        source,
        delivered_by_stream,
        audio_ms,
        total_ms,
        foreground_app,
    } = parts;
    // Name the engine and model that actually produced this line. A
    // fallback dictation credited to the cloud provider, or a batch
    // rescue credited to the realtime model, would make history lie
    // about why a transcript reads the way it does.
    let stt_model = match source {
        Source::Cloud if delivered_by_stream => worker.stream_model.clone(),
        Source::Cloud => worker.batch_model.clone(),
        _ => worker
            .local
            .as_ref()
            .map_or_else(String::new, |p| p.spec.id.to_string()),
    };
    // The labels state what actually shaped the text: a skipped,
    // gated, or failed cleanup call means the entry is verbatim, so
    // a disappointing result never blames the wrong model.
    let cleanup_ran = cleaned.request_ms.is_some();
    let (voice, cleanup_model) = match worker.cleanup.as_ref().filter(|_| cleanup_ran) {
        Some(plan) => (plan.voice.name().to_string(), Some(plan.model.clone())),
        // A fused provider did the cleanup inside the transcription
        // call, so the STT model is the cleanup model. Labelling this
        // "verbatim" would tell the user nothing rewrote their text
        // when something plainly did.
        None if provider_cleaned => ("smart".to_string(), Some(stt_model.clone())),
        None => (Voice::Verbatim.name().to_string(), None),
    };
    DictationRecord {
        raw_text: transcript.text,
        final_text: cleaned.text,
        voice,
        stt_provider: source.label(&worker.cloud_label),
        stt_model,
        cleanup_model,
        invocation: fired,
        audio_ms,
        stt_ms: transcript.request_ms as u64,
        cleanup_ms: cleaned.request_ms,
        total_ms,
        spellbook_replacements: Some(
            pass_one_replacements.saturating_add(cleaned.spellbook_replacements),
        ),
        foreground_app,
    }
}

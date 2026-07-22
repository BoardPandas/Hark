//! Advisory events the pipeline emits toward the UI over the channel passed
//! to [`run`](crate::run). Sends are non-blocking and best-effort: a
//! disconnected receiver is ignored, so dictation never waits on (or fails
//! because of) the UI.

/// One completed dictation. This is the sanctioned in-process carrier of
/// transcript content: its only destinations are the local history database
/// and the history panel. Deliberately no `Debug` impl, so the content
/// cannot drift into a log line via `{:?}` (logging discipline stays
/// counts, millis, and labels).
#[derive(Clone)]
pub struct DictationRecord {
    /// The raw STT transcript, exactly as the provider returned it.
    pub raw_text: String,
    /// The injected text (after dictionary passes and optional cleanup).
    pub final_text: String,
    /// Voice label that actually ran ("verbatim" when cleanup was off or
    /// degraded), for display next to the entry.
    pub voice: String,
    /// STT provider label (config taxonomy, e.g. "deepgram").
    pub stt_provider: String,
    /// STT model label (e.g. "nova-3").
    pub stt_model: String,
    /// Cleanup model label, present only when a cleanup response actually
    /// shaped the final text.
    pub cleanup_model: Option<String>,
    /// The invocation trigger phrase that fired, when the final text is
    /// canned text rather than something the user actually said. Drives the
    /// history badge, and tells the stats counter to value the words the
    /// user *spoke* rather than the ones Hark pasted.
    pub invocation: Option<String>,
    /// Duration of the assembled clip sent to the provider.
    pub audio_ms: u64,
    /// STT request wall time.
    pub stt_ms: u64,
    /// Cleanup request wall time, present only when cleanup shaped the text.
    pub cleanup_ms: Option<u64>,
    /// Release-to-inject wall time (the product metric).
    pub total_ms: u64,
}

/// Where a dictation ended when it did not inject. Labels only; the
/// accompanying detail string carries error summaries, never key material
/// and never transcript content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailStage {
    /// The hold was too short to be speech (a tap, a misfire). Stopped before
    /// any request was sent. Informational, not an error.
    GatedTooShort,
    /// Nothing loud enough to be speech was captured. Stopped before any
    /// request was sent. Informational, but unlike a misfire this one may mean
    /// the user's microphone is misconfigured, so the UI can offer help.
    GatedTooQuiet,
    /// Window assembly failed (ring buffer / resample error).
    Audio,
    /// The STT request failed (after the single eligible retry).
    Transcribe,
    /// The provider returned an empty transcript; nothing to inject.
    /// Informational, not an error.
    EmptyTranscript,
    /// Injection into the focused app failed.
    Inject,
}

/// What the pipeline tells the UI. Events are advisory: emitting one never
/// blocks, and no pipeline behavior depends on whether anyone listens.
#[derive(Clone)]
pub enum PipelineEvent {
    /// The chord went down; audio is being captured.
    Recording,
    /// The chord released; encode + STT request are in flight.
    Processing,
    /// The on-device model is being read into RAM. Emitted only for the first
    /// dictation that needs it (the engine then stays resident), but that load
    /// is a multi-second pause, so the UI must be able to say why rather than
    /// looking hung.
    LoadingLocalModel,
    /// The dictation injected successfully; the record goes to history.
    Injected(DictationRecord),
    /// The dictation ended without injecting.
    Failed { stage: FailStage, detail: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_are_send_for_cross_thread_delivery() {
        fn assert_send<T: Send>() {}
        assert_send::<PipelineEvent>();
    }

    #[test]
    fn a_dropped_receiver_makes_send_fail_without_panicking() {
        // The advisory contract: the worker's `let _ = send(..)` relies on a
        // disconnected channel being an Err, never a panic or a block.
        let (tx, rx) = std::sync::mpsc::channel::<PipelineEvent>();
        drop(rx);
        assert!(tx.send(PipelineEvent::Recording).is_err());
    }
}

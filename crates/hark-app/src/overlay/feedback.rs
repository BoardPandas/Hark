//! Content-free feedback shared with the event pump. The child viewport reads
//! this directly, including its expiry, even when the parent window is asleep.
use hark_pipeline::{FailStage, PipelineEvent};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum State {
    Hidden,
    Listening,
    Processing,
    LoadingModel,
    Inserted,
    AudioError,
    ProviderError,
    InsertError,
    Quiet,
    /// The words were transcribed but the paste failed; they are in History.
    NotPasted,
    /// Transcription failed and the audio is held for a retry from History.
    TranscriptionHeld,
}

impl State {
    pub fn label(self) -> &'static str {
        match self {
            Self::Hidden => "",
            Self::Listening => "Listening…",
            Self::Processing => "Processing…",
            Self::LoadingModel => "Loading model…",
            Self::Inserted => "Inserted",
            Self::AudioError => "Check microphone",
            Self::ProviderError => "Check provider",
            Self::InsertError => "Couldn't insert",
            Self::Quiet => "No speech heard",
            // The pill is a fixed 192 px, so these are the short forms of
            // "Couldn't paste. Open History to copy the text." and "Couldn't
            // transcribe. Try again from History."; the footer says it in full.
            Self::NotPasted => "Not pasted: History",
            Self::TranscriptionHeld => "Retry from History",
        }
    }

    fn duration_ms(self) -> Option<u64> {
        match self {
            Self::Inserted => Some(1_000),
            Self::AudioError | Self::ProviderError | Self::InsertError | Self::Quiet => Some(3_000),
            // Longer: these tell the user where to go, not just what happened.
            Self::NotPasted | Self::TranscriptionHeld => Some(4_000),
            _ => None,
        }
    }
}

const DISABLED: u64 = u64::MAX;

pub struct Feedback {
    started: Instant,
    /// Milliseconds since construction in the upper bits, state in the low 8.
    /// A single atomic snapshot prevents a new state inheriting an old expiry.
    value: AtomicU64,
}

impl Feedback {
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
            value: AtomicU64::new(0),
        }
    }

    pub fn publish(&self, event: &PipelineEvent) {
        let state = match event {
            // Not about the dictation on screen; the footer carries it.
            PipelineEvent::ShortcutIntercepted { .. } => return,
            PipelineEvent::Recording => State::Listening,
            PipelineEvent::Processing => State::Processing,
            PipelineEvent::LoadingLocalModel => State::LoadingModel,
            PipelineEvent::Injected(_) => State::Inserted,
            PipelineEvent::NotPasted { .. } => State::NotPasted,
            PipelineEvent::ClipHeld { held: true } => State::TranscriptionHeld,
            // The worker's bookkeeping, not a result for the pill: the words
            // that ended a held clip's story arrive as their own event.
            PipelineEvent::ClipHeld { held: false } => return,
            // A retry the user asked for from Hark's own window: the pill
            // belongs to dictation, and the banner already shows the result.
            PipelineEvent::Recovered(_) => State::Hidden,
            PipelineEvent::Failed { stage, .. } => match stage {
                FailStage::GatedTooShort | FailStage::EmptyTranscript | FailStage::Abandoned => {
                    State::Hidden
                }
                FailStage::GatedTooQuiet => State::Quiet,
                FailStage::Audio => State::AudioError,
                FailStage::Transcribe => State::ProviderError,
                FailStage::Inject => State::InsertError,
                // A bug in Hark, not the provider's doing -- but the overlay's
                // vocabulary is about where the dictation stopped, and from the
                // user's seat this stopped in the same place a provider failure
                // does: text was expected and none arrived.
                FailStage::Internal => State::ProviderError,
            },
        };
        self.publish_at(state, self.elapsed_ms());
    }

    pub fn disable(&self) {
        self.value.store(DISABLED, Ordering::Relaxed);
    }

    pub(super) fn state(&self, recording: bool) -> State {
        self.state_at(recording, self.elapsed_ms())
    }

    fn elapsed_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    fn publish_at(&self, state: State, now_ms: u64) {
        let mut value = self.value.load(Ordering::Relaxed);
        // Preserve the disabled sentinel even if shutdown races publication.
        // An explicit CAS loop also works on the workspace's older Rust MSRV.
        while value != DISABLED {
            match self.value.compare_exchange_weak(
                value,
                (now_ms << 8) | state as u64,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return,
                Err(current) => value = current,
            }
        }
    }

    fn state_at(&self, recording: bool, now_ms: u64) -> State {
        let value = self.value.load(Ordering::Relaxed);
        if value == DISABLED {
            return State::Hidden;
        }
        if recording {
            return State::Listening;
        }
        let state = match value as u8 {
            2 => State::Processing,
            3 => State::LoadingModel,
            4 => State::Inserted,
            5 => State::AudioError,
            6 => State::ProviderError,
            7 => State::InsertError,
            8 => State::Quiet,
            9 => State::NotPasted,
            10 => State::TranscriptionHeld,
            // The worker's live flag decides when recording ends, not an old
            // Recording event waiting for the parent's next pass.
            _ => State::Hidden,
        };
        if state
            .duration_ms()
            .is_some_and(|ttl| now_ms.saturating_sub(value >> 8) >= ttl)
        {
            State::Hidden
        } else {
            state
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_feedback_expires_without_a_parent_pass() {
        let feedback = Feedback::new();
        feedback.publish_at(State::Inserted, 500);
        assert_eq!(feedback.state_at(false, 1_499), State::Inserted);
        assert_eq!(feedback.state_at(false, 1_500), State::Hidden);
        feedback.publish_at(State::AudioError, 2_000);
        assert_eq!(feedback.state_at(false, 4_999), State::AudioError);
        assert_eq!(feedback.state_at(false, 5_000), State::Hidden);
    }

    #[test]
    fn live_capture_wins_and_release_cannot_leave_a_listening_pill() {
        let feedback = Feedback::new();
        feedback.publish_at(State::Inserted, 0);
        assert_eq!(feedback.state_at(true, 10), State::Listening);
        feedback.publish_at(State::Listening, 20);
        assert_eq!(feedback.state_at(false, 21), State::Hidden);
        feedback.publish_at(State::Processing, 22);
        assert_eq!(feedback.state_at(false, 60_000), State::Processing);
    }

    #[test]
    fn stopped_pipeline_cannot_resurrect_feedback() {
        let feedback = Feedback::new();
        feedback.disable();
        feedback.publish_at(State::Inserted, 20);
        assert_eq!(feedback.state_at(true, 21), State::Hidden);
    }

    fn record() -> Box<hark_pipeline::DictationRecord> {
        Box::new(hark_pipeline::DictationRecord {
            raw_text: String::new(),
            final_text: String::new(),
            voice: String::new(),
            stt_provider: String::new(),
            stt_model: String::new(),
            cleanup_model: None,
            invocation: None,
            audio_ms: 0,
            stt_ms: 0,
            cleanup_ms: None,
            total_ms: 0,
            spellbook_replacements: None,
            foreground_app: None,
        })
    }

    #[test]
    fn a_failed_paste_and_a_held_clip_both_point_at_history_and_then_expire() {
        let feedback = Feedback::new();
        feedback.publish(&PipelineEvent::NotPasted {
            record: record(),
            detail: String::new(),
        });
        assert_eq!(feedback.state_at(false, 0), State::NotPasted);
        assert!(State::NotPasted.label().contains("History"));
        feedback.publish_at(State::NotPasted, 1_000);
        assert_eq!(feedback.state_at(false, 4_999), State::NotPasted);
        assert_eq!(feedback.state_at(false, 5_000), State::Hidden);

        feedback.publish(&PipelineEvent::ClipHeld { held: true });
        assert_eq!(feedback.state_at(false, 0), State::TranscriptionHeld);
        assert!(State::TranscriptionHeld.label().contains("History"));
    }

    #[test]
    fn dropping_a_held_clip_leaves_the_pill_alone_and_a_recovery_clears_it() {
        let feedback = Feedback::new();
        feedback.publish(&PipelineEvent::Processing);
        feedback.publish(&PipelineEvent::ClipHeld { held: false });
        assert_eq!(feedback.state_at(false, 0), State::Processing);
        feedback.publish(&PipelineEvent::Recovered(record()));
        assert_eq!(feedback.state_at(false, 0), State::Hidden);
    }

    #[test]
    fn silent_gates_hide_but_quiet_audio_explains_what_happened() {
        let feedback = Feedback::new();
        for stage in [
            FailStage::GatedTooShort,
            FailStage::EmptyTranscript,
            FailStage::Abandoned,
        ] {
            feedback.publish(&PipelineEvent::Failed {
                stage,
                detail: String::new(),
            });
            assert_eq!(feedback.state_at(false, 0), State::Hidden);
        }
        feedback.publish(&PipelineEvent::Failed {
            stage: FailStage::GatedTooQuiet,
            detail: String::new(),
        });
        assert_eq!(feedback.state_at(false, 0), State::Quiet);
    }
}

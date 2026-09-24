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
        }
    }

    fn duration_ms(self) -> Option<u64> {
        match self {
            Self::Inserted => Some(1_000),
            Self::AudioError | Self::ProviderError | Self::InsertError | Self::Quiet => Some(3_000),
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
        let _ = self
            .value
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                (value != DISABLED).then_some((now_ms << 8) | state as u64)
            });
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

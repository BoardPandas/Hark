//! Pure pipeline state machine: Idle -> Recording -> Transcribing ->
//! Injecting -> Idle, with defined handling for every edge case. No I/O, no
//! clocks; sample indices are opaque numbers here.

/// Where the pipeline is in the dictation cycle. `Recording` carries the
/// absolute sample index observed when the chord went down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineState {
    Idle,
    Recording { down_abs: u64 },
    Transcribing,
    Injecting,
}

/// Everything that can advance the state machine. Ptt events carry the
/// absolute sample index observed at processing time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    PttDown {
        at_abs: u64,
    },
    PttUp {
        at_abs: u64,
    },
    /// Transcription returned text: move on to injection.
    TranscriptReady,
    /// Injection completed: the dictation is done.
    Injected,
    /// The dictation ended early (silence-gated, transcription failed, empty
    /// transcript, injection failed): back to Idle, ready for the next press.
    Aborted,
}

/// What the worker must do after a transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    /// Assemble the window for these bounds and run it through
    /// transcribe -> inject.
    Dictate {
        down_abs: u64,
        up_abs: u64,
    },
}

/// Advance the machine. Total: every (state, event) pair is defined, and
/// unexpected pairs are explicitly inert rather than panicking (a stray hook
/// event must never take the pipeline down).
pub fn advance(state: PipelineState, event: Event) -> (PipelineState, Action) {
    use Event::*;
    use PipelineState::*;
    match (state, event) {
        // The happy path.
        (Idle, PttDown { at_abs }) => (Recording { down_abs: at_abs }, Action::None),
        (Recording { down_abs }, PttUp { at_abs }) => (
            Transcribing,
            Action::Dictate {
                down_abs,
                up_abs: at_abs,
            },
        ),
        (Transcribing, TranscriptReady) => (Injecting, Action::None),
        (Injecting, Injected) => (Idle, Action::None),

        // Any stage can abort back to Idle.
        (_, Aborted) => (Idle, Action::None),

        // Up with no matching Down (stray release at startup, or the Down
        // half was consumed by a previous cycle): ignore.
        (Idle, PttUp { .. }) => (Idle, Action::None),

        // A duplicate Down while recording keeps the ORIGINAL down index
        // (the edge tracker suppresses these; belt and braces).
        (Recording { down_abs }, PttDown { .. }) => (Recording { down_abs }, Action::None),

        // New presses while a dictation is in flight are ignored, not queued
        // (spec §3.19 decision: ignore).
        (Transcribing, PttDown { .. } | PttUp { .. }) => (Transcribing, Action::None),
        (Injecting, PttDown { .. } | PttUp { .. }) => (Injecting, Action::None),

        // Completion events arriving in the wrong stage (worker logic bug or
        // reordering): inert, never a panic.
        (Idle, TranscriptReady | Injected) => (Idle, Action::None),
        (s @ Recording { .. }, TranscriptReady | Injected) => (s, Action::None),
        (Transcribing, Injected) => (Transcribing, Action::None),
        (Injecting, TranscriptReady) => (Injecting, Action::None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Event::*;
    use PipelineState::*;

    #[test]
    fn happy_path_full_cycle() {
        let (s, a) = advance(Idle, PttDown { at_abs: 100 });
        assert_eq!(s, Recording { down_abs: 100 });
        assert_eq!(a, Action::None);

        let (s, a) = advance(s, PttUp { at_abs: 900 });
        assert_eq!(s, Transcribing);
        assert_eq!(
            a,
            Action::Dictate {
                down_abs: 100,
                up_abs: 900
            }
        );

        let (s, a) = advance(s, TranscriptReady);
        assert_eq!(s, Injecting);
        assert_eq!(a, Action::None);

        let (s, a) = advance(s, Injected);
        assert_eq!(s, Idle);
        assert_eq!(a, Action::None);
    }

    #[test]
    fn up_without_down_is_ignored() {
        let (s, a) = advance(Idle, PttUp { at_abs: 42 });
        assert_eq!(s, Idle);
        assert_eq!(a, Action::None);
    }

    #[test]
    fn duplicate_down_keeps_original_index() {
        let (s, _) = advance(Idle, PttDown { at_abs: 100 });
        let (s, a) = advance(s, PttDown { at_abs: 500 });
        assert_eq!(s, Recording { down_abs: 100 });
        assert_eq!(a, Action::None);
    }

    #[test]
    fn presses_during_flight_are_ignored_not_queued() {
        for busy in [Transcribing, Injecting] {
            let (s, a) = advance(busy, PttDown { at_abs: 1 });
            assert_eq!(s, busy);
            assert_eq!(a, Action::None);
            let (s, a) = advance(busy, PttUp { at_abs: 2 });
            assert_eq!(s, busy);
            assert_eq!(a, Action::None);
        }
    }

    #[test]
    fn abort_returns_to_idle_from_every_state() {
        for state in [Idle, Recording { down_abs: 7 }, Transcribing, Injecting] {
            let (s, a) = advance(state, Aborted);
            assert_eq!(s, Idle, "abort from {state:?} must land in Idle");
            assert_eq!(a, Action::None);
        }
    }

    #[test]
    fn misplaced_completion_events_are_inert() {
        assert_eq!(advance(Idle, TranscriptReady).0, Idle);
        assert_eq!(advance(Idle, Injected).0, Idle);
        assert_eq!(
            advance(Recording { down_abs: 3 }, TranscriptReady).0,
            Recording { down_abs: 3 }
        );
        assert_eq!(
            advance(Recording { down_abs: 3 }, Injected).0,
            Recording { down_abs: 3 }
        );
        assert_eq!(advance(Transcribing, Injected).0, Transcribing);
        assert_eq!(advance(Injecting, TranscriptReady).0, Injecting);
    }

    #[test]
    fn recording_survives_after_ignored_events_and_completes() {
        // A Recording state hit by stray completion events still dictates.
        let (s, _) = advance(Idle, PttDown { at_abs: 10 });
        let (s, _) = advance(s, TranscriptReady);
        let (s, a) = advance(s, PttUp { at_abs: 60 });
        assert_eq!(s, Transcribing);
        assert_eq!(
            a,
            Action::Dictate {
                down_abs: 10,
                up_abs: 60
            }
        );
    }
}

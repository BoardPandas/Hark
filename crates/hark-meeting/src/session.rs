//! Pure meeting-session state machine: Idle -> Recording -> Finalizing ->
//! Summarizing -> Done | Failed. No I/O, no clocks; the coordinator performs
//! each [`Action`] and reports back with an [`Event`].
//!
//! One machine per meeting, and a finished machine never restarts. Back-to-back
//! calls are normal (one meeting summarizing while the next starts recording),
//! so "only one meeting records at a time" is the coordinator's rule: it holds
//! a machine per meeting rather than recycling one through Idle.
//!
//! Deliberately separate from `hark-pipeline::state`. Dictation is a one-shot
//! press/release cycle; a meeting is long-lived and must never share, block,
//! or reset the push-to-talk machine.

/// Where one meeting is in its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// Created; capture not started yet.
    Idle,
    /// Capture is live: both channels spool to disk and feed the live chunkers.
    Recording,
    /// Capture has stopped. The spools are closing, the last live chunks are
    /// transcribing, and the final pass (if any) is running.
    Finalizing,
    /// The transcript is settled; the summary request (if any) is running.
    Summarizing,
    /// Transcript and notes are saved.
    Done,
    /// The meeting could not be kept. See [`Failure`].
    Failed(Failure),
}

/// Why a meeting ended in [`SessionState::Failed`].
///
/// Only the failures that leave nothing to keep belong here. A failed or
/// skipped final pass keeps the live transcript, and a failed or skipped
/// summary keeps the transcript without notes: the finisher reports those as
/// [`Event::Finalized`] and [`Event::Summarized`], never as a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// No capture stream could be opened, so nothing was recorded.
    Capture,
}

/// Everything that can advance a meeting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// Begin recording (manual start, or an accepted/automatic detection).
    Start,
    /// Stop recording (manual stop, the call ended, or capture was lost with
    /// audio already on disk: all of them keep what was recorded).
    Stop,
    /// Spools closed and the transcript is settled, refined by the final
    /// pass or not.
    Finalized,
    /// Notes are saved, or there were none to make.
    Summarized,
    /// Something happened that leaves nothing to keep.
    Fail(Failure),
}

/// What the coordinator must do after a transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    /// Open both capture streams and their spools, and start chunking.
    StartCapture,
    /// Stop capture, flush the chunkers, close the spools, and run the final
    /// pass. Report [`Event::Finalized`] when done.
    Finalize,
    /// Summarize the settled transcript and save. Report [`Event::Summarized`].
    Summarize,
    /// Tear down whatever capture was opened, without a final pass. Only
    /// issued when a meeting fails while recording.
    ReleaseCapture,
}

impl SessionState {
    /// Done and Failed absorb every later event.
    pub fn is_terminal(self) -> bool {
        matches!(self, SessionState::Done | SessionState::Failed(_))
    }
}

/// Advance the machine. Total: every (state, event) pair is defined, and
/// unexpected pairs are inert rather than panicking. Manual stop, auto-stop,
/// and a lost device can all report `Stop` for the same meeting, and a stray
/// duplicate must never take a recording down.
pub fn advance(state: SessionState, event: Event) -> (SessionState, Action) {
    use Event::*;
    use SessionState::*;
    match (state, event) {
        // Terminal states absorb everything, including a late failure: a
        // meeting that is already saved stays saved.
        (s @ (Done | Failed(_)), _) => (s, Action::None),

        // The happy path.
        (Idle, Start) => (Recording, Action::StartCapture),
        (Recording, Stop) => (Finalizing, Action::Finalize),
        (Finalizing, Finalized) => (Summarizing, Action::Summarize),
        (Summarizing, Summarized) => (Done, Action::None),

        // Failure from a live state. Capture only needs releasing if it may
        // have been opened, i.e. while recording (StartCapture runs after the
        // transition, so a stream that fails to open reports back here).
        (Recording, Fail(f)) => (Failed(f), Action::ReleaseCapture),
        (Idle | Finalizing | Summarizing, Fail(f)) => (Failed(f), Action::None),

        // A stop before anything started: nothing to finalize.
        (Idle, Stop) => (Idle, Action::None),

        // A second Start is a duplicate; this machine is one meeting.
        (s @ (Recording | Finalizing | Summarizing), Start) => (s, Action::None),

        // A second Stop (auto-stop racing a manual stop) is a duplicate.
        (s @ (Finalizing | Summarizing), Stop) => (s, Action::None),

        // Completion events in the wrong stage (coordinator bug or
        // reordering): inert, never a panic.
        (s @ (Idle | Recording | Summarizing), Finalized) => (s, Action::None),
        (s @ (Idle | Recording | Finalizing), Summarized) => (s, Action::None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Event::*;
    use SessionState::*;

    const ALL_EVENTS: [Event; 5] = [Start, Stop, Finalized, Summarized, Fail(Failure::Capture)];

    #[test]
    fn happy_path_full_lifecycle() {
        let (s, a) = advance(Idle, Start);
        assert_eq!((s, a), (Recording, Action::StartCapture));
        let (s, a) = advance(s, Stop);
        assert_eq!((s, a), (Finalizing, Action::Finalize));
        let (s, a) = advance(s, Finalized);
        assert_eq!((s, a), (Summarizing, Action::Summarize));
        let (s, a) = advance(s, Summarized);
        assert_eq!((s, a), (Done, Action::None));
        assert!(s.is_terminal());
    }

    #[test]
    fn capture_that_fails_to_open_is_released() {
        let (s, _) = advance(Idle, Start);
        let (s, a) = advance(s, Fail(Failure::Capture));
        assert_eq!(s, Failed(Failure::Capture));
        assert_eq!(a, Action::ReleaseCapture);
    }

    #[test]
    fn failures_after_recording_release_nothing() {
        for state in [Idle, Finalizing, Summarizing] {
            let (s, a) = advance(state, Fail(Failure::Capture));
            assert_eq!(s, Failed(Failure::Capture), "from {state:?}");
            assert_eq!(a, Action::None, "from {state:?}");
        }
    }

    #[test]
    fn terminal_states_absorb_every_event() {
        for terminal in [Done, Failed(Failure::Capture)] {
            for event in ALL_EVENTS {
                assert_eq!(
                    advance(terminal, event),
                    (terminal, Action::None),
                    "{terminal:?} + {event:?}"
                );
            }
        }
    }

    #[test]
    fn a_finished_meeting_never_restarts() {
        // The coordinator makes a new machine per meeting; Start on an old
        // one must not reopen capture into a finished meeting's spool.
        assert_eq!(advance(Done, Start), (Done, Action::None));
    }

    #[test]
    fn duplicate_stop_during_finalizing_is_inert() {
        // Auto-stop fires just after the user pressed Stop.
        let (s, _) = advance(Recording, Stop);
        let (s2, a) = advance(s, Stop);
        assert_eq!((s2, a), (Finalizing, Action::None));
        assert_eq!(advance(Summarizing, Stop), (Summarizing, Action::None));
    }

    #[test]
    fn stop_before_start_does_nothing() {
        assert_eq!(advance(Idle, Stop), (Idle, Action::None));
    }

    #[test]
    fn duplicate_start_is_inert_in_every_live_state() {
        for state in [Recording, Finalizing, Summarizing] {
            assert_eq!(advance(state, Start), (state, Action::None), "{state:?}");
        }
    }

    #[test]
    fn misplaced_completion_events_are_inert() {
        for state in [Idle, Recording, Summarizing] {
            assert_eq!(
                advance(state, Finalized),
                (state, Action::None),
                "{state:?}"
            );
        }
        for state in [Idle, Recording, Finalizing] {
            assert_eq!(
                advance(state, Summarized),
                (state, Action::None),
                "{state:?}"
            );
        }
    }

    #[test]
    fn recording_survives_stray_events_and_still_finalizes() {
        let (mut s, _) = advance(Idle, Start);
        for stray in [Start, Finalized, Summarized] {
            s = advance(s, stray).0;
        }
        assert_eq!(advance(s, Stop), (Finalizing, Action::Finalize));
    }

    #[test]
    fn only_done_and_failed_are_terminal() {
        for state in [Idle, Recording, Finalizing, Summarizing] {
            assert!(!state.is_terminal(), "{state:?}");
        }
    }
}

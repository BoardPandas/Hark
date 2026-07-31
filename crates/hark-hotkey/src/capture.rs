//! "Record a shortcut" support: raw key edges and the buffer that turns them
//! into a chord. Split from `edges.rs` because the two answer different
//! questions — `edges.rs` decides when a *configured* chord engages, this
//! decides what the user just pressed.

use crate::edges::{PttChord, PttKeyCode};

/// A raw key edge streamed while the settings UI is recording a shortcut.
/// Unlike `PttEvent` (already-resolved chord engage/disengage), this is one
/// per chord-capable key press or release, fed to a [`CaptureBuffer`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureEvent {
    pub key: PttKeyCode,
    pub down: bool,
}

/// Why a recorded combination was not accepted as a shortcut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejected {
    /// A lone Ctrl/Shift/Alt/Win. Binding push-to-talk to a bare modifier means
    /// every Ctrl press in every app starts a dictation, which is never what
    /// the user meant and is miserable to undo from inside the app it breaks.
    LoneModifier(PttKeyCode),
}

impl Rejected {
    pub fn message(&self) -> String {
        match self {
            Rejected::LoneModifier(key) => format!(
                "{} on its own would start a dictation every time you press it. \
                 Hold it together with another key, or use an F-key or Caps Lock.",
                key.label()
            ),
        }
    }
}

/// Builds a chord from a stream of raw capture edges (the "record a shortcut"
/// UX).
///
/// The chord is the PEAK — the largest set held simultaneously — and it commits
/// only once every key has been let go. Committing on the *first* release, the
/// obvious reading of "press the combo and let go", is wrong on real hands: a
/// combination is never pressed or released in one instant, and a user reaching
/// for Ctrl+Shift whose Ctrl comes up a few milliseconds before Shift goes down
/// records a bare "Left Ctrl". They then save it and every Ctrl press in every
/// app starts a dictation. Waiting for all-released costs nothing and cannot
/// truncate a combination.
///
/// Release-to-commit at all, rather than the commit-on-first-non-modifier rule
/// most shortcut recorders use, because push-to-talk chords are usually
/// modifier-only (Discord, OBS and every other PTT app work the same way): a
/// recorder that waited for a "real" key could never record Ctrl+Win.
#[derive(Debug, Default)]
pub struct CaptureBuffer {
    /// Every key seen down since the last commit: the peak, in press order.
    held: Vec<PttKeyCode>,
    /// The subset still physically down. The chord commits when this empties.
    down: Vec<PttKeyCode>,
    saw_any: bool,
    rejected: Option<Rejected>,
}

impl CaptureBuffer {
    pub fn new() -> CaptureBuffer {
        CaptureBuffer::default()
    }

    /// Feed one raw edge. Returns `Some(chord)` once the user has pressed a
    /// combination and let go of all of it. Duplicates (key auto-repeat) and a
    /// 5th simultaneous key are ignored, so the result is always 1..=4 distinct
    /// keys. A combination that is not usable as a shortcut resets the buffer
    /// and leaves a reason in [`Self::rejected`] instead of committing.
    pub fn on_event(&mut self, key: PttKeyCode, down: bool) -> Option<PttChord> {
        if down {
            self.saw_any = true;
            self.rejected = None;
            if !self.held.contains(&key) && self.held.len() < 4 {
                self.held.push(key);
            }
            // Auto-repeat re-sends the press; the key must appear here once.
            if self.held.contains(&key) && !self.down.contains(&key) {
                self.down.push(key);
            }
            return None;
        }

        self.down.retain(|k| *k != key);
        // Still mid-combination, or a stray release of a key that was already
        // held when recording began (which must not commit anything).
        if !self.saw_any || !self.down.is_empty() || self.held.is_empty() {
            return None;
        }

        let chord = PttChord::from_keys(std::mem::take(&mut self.held));
        self.saw_any = false;
        if let Some(only) = chord.lone_modifier() {
            self.rejected = Some(Rejected::LoneModifier(only));
            return None;
        }
        Some(chord)
    }

    /// Keys held so far, for a live "Left Ctrl + F12" display while recording.
    pub fn held(&self) -> &[PttKeyCode] {
        &self.held
    }

    /// Why the last completed combination was turned down, if it was.
    pub fn rejected(&self) -> Option<Rejected> {
        self.rejected
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(text: &str) -> PttChord {
        PttChord::parse(text).unwrap()
    }

    #[test]
    fn capture_commits_the_peak_once_everything_is_released() {
        let mut b = CaptureBuffer::new();
        assert_eq!(b.on_event(PttKeyCode::LCtrl, true), None);
        assert_eq!(b.on_event(PttKeyCode::LWin, true), None);
        assert_eq!(b.held(), &[PttKeyCode::LCtrl, PttKeyCode::LWin]);
        // The first release must NOT commit: hands do not let go in one instant.
        assert_eq!(b.on_event(PttKeyCode::LCtrl, false), None);
        assert_eq!(
            b.on_event(PttKeyCode::LWin, false),
            Some(chord("LCtrl+LWin"))
        );
    }

    /// The bug this exists for: Ctrl released a moment before Shift goes down
    /// used to record a bare "LCtrl", which the user then saved -- and every
    /// Ctrl press in every app started a dictation.
    #[test]
    fn a_staggered_press_still_records_the_whole_combination() {
        let mut b = CaptureBuffer::new();
        b.on_event(PttKeyCode::LCtrl, true);
        b.on_event(PttKeyCode::LShift, true);
        assert_eq!(b.on_event(PttKeyCode::LCtrl, false), None);
        assert_eq!(
            b.on_event(PttKeyCode::LShift, false),
            Some(chord("LCtrl+LShift"))
        );
    }

    #[test]
    fn a_lone_modifier_is_turned_down_with_a_reason() {
        let mut b = CaptureBuffer::new();
        b.on_event(PttKeyCode::LCtrl, true);
        assert_eq!(b.on_event(PttKeyCode::LCtrl, false), None);
        assert_eq!(
            b.rejected(),
            Some(Rejected::LoneModifier(PttKeyCode::LCtrl))
        );
        assert!(b.rejected().unwrap().message().contains("Left Ctrl"));
        // The buffer is clean afterwards, so the next attempt starts fresh.
        assert!(b.held().is_empty());
        b.on_event(PttKeyCode::LCtrl, true);
        b.on_event(PttKeyCode::F(12), true);
        b.on_event(PttKeyCode::LCtrl, false);
        assert_eq!(
            b.on_event(PttKeyCode::F(12), false),
            Some(chord("LCtrl+F12"))
        );
    }

    #[test]
    fn capture_records_a_single_key_chord_when_it_is_not_a_modifier() {
        let mut b = CaptureBuffer::new();
        assert_eq!(b.on_event(PttKeyCode::F(13), true), None);
        assert_eq!(b.on_event(PttKeyCode::F(13), false), Some(chord("F13")));

        let mut b = CaptureBuffer::new();
        b.on_event(PttKeyCode::CapsLock, true);
        assert_eq!(
            b.on_event(PttKeyCode::CapsLock, false),
            Some(chord("CapsLock"))
        );
    }

    #[test]
    fn capture_dedupes_auto_repeat_and_caps_at_four() {
        let mut b = CaptureBuffer::new();
        b.on_event(PttKeyCode::LCtrl, true);
        // Windows repeats WM_KEYDOWN while held: no duplicate members, and the
        // repeat must not leave a phantom entry that blocks the commit.
        b.on_event(PttKeyCode::LCtrl, true);
        b.on_event(PttKeyCode::LShift, true);
        b.on_event(PttKeyCode::LAlt, true);
        b.on_event(PttKeyCode::LWin, true);
        // A fifth simultaneous key is ignored (chords support at most four).
        b.on_event(PttKeyCode::RCtrl, true);
        assert_eq!(
            b.held(),
            &[
                PttKeyCode::LCtrl,
                PttKeyCode::LShift,
                PttKeyCode::LAlt,
                PttKeyCode::LWin
            ]
        );
        for key in [
            PttKeyCode::LCtrl,
            PttKeyCode::LShift,
            PttKeyCode::LAlt,
            PttKeyCode::RCtrl,
        ] {
            assert_eq!(b.on_event(key, false), None);
        }
        assert_eq!(
            b.on_event(PttKeyCode::LWin, false),
            Some(chord("LCtrl+LShift+LAlt+LWin"))
        );
    }

    #[test]
    fn capture_ignores_releases_of_keys_held_before_recording_began() {
        let mut b = CaptureBuffer::new();
        // Clicking "Change" with Ctrl still down: its release is not a chord.
        assert_eq!(b.on_event(PttKeyCode::LCtrl, false), None);
        assert_eq!(b.on_event(PttKeyCode::LShift, false), None);
        assert_eq!(b.rejected(), None);
        // Recording then proceeds normally.
        assert_eq!(b.on_event(PttKeyCode::F(9), true), None);
        assert_eq!(b.on_event(PttKeyCode::F(9), false), Some(chord("F9")));
    }
}

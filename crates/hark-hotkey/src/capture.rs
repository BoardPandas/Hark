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

/// Builds a chord from a stream of raw capture edges (the "record a shortcut"
/// UX). The chord is the set of keys held at the instant the FIRST key is
/// released (the peak), mirroring `ChordTracker`'s engage-on-last-down /
/// disengage-on-first-up semantics: press the combo, then let go to set it.
///
/// Release-to-commit rather than the commit-on-first-non-modifier rule most
/// shortcut recorders use, because push-to-talk chords are usually
/// modifier-only (Discord, OBS and every other PTT app work the same way): a
/// recorder that waited for a "real" key could never record Ctrl+Win.
#[derive(Debug, Default)]
pub struct CaptureBuffer {
    held: Vec<PttKeyCode>,
    saw_any: bool,
}

impl CaptureBuffer {
    pub fn new() -> CaptureBuffer {
        CaptureBuffer::default()
    }

    /// Feed one raw edge. Returns `Some(chord)` once the user completes a
    /// chord (a release after at least one key was held); `None` while still
    /// gathering. Duplicates (key auto-repeat) and a 5th simultaneous key are
    /// ignored, so the result is always 1..=4 distinct keys.
    pub fn on_event(&mut self, key: PttKeyCode, down: bool) -> Option<PttChord> {
        if down {
            self.saw_any = true;
            if !self.held.contains(&key) && self.held.len() < 4 {
                self.held.push(key);
            }
            None
        } else if self.saw_any && !self.held.is_empty() {
            Some(PttChord::from_keys(self.held.clone()))
        } else {
            // A stray release before anything was held: keep waiting.
            None
        }
    }

    /// Keys held so far, for a live "Left Ctrl + F12" display while recording.
    pub fn held(&self) -> &[PttKeyCode] {
        &self.held
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(text: &str) -> PttChord {
        PttChord::parse(text).unwrap()
    }

    #[test]
    fn capture_completes_on_first_release_at_the_peak() {
        let mut b = CaptureBuffer::new();
        // Press the combo up...
        assert_eq!(b.on_event(PttKeyCode::LCtrl, true), None);
        assert_eq!(b.on_event(PttKeyCode::LWin, true), None);
        assert_eq!(b.held(), &[PttKeyCode::LCtrl, PttKeyCode::LWin]);
        // ...then the first release sets it, held keys and all.
        assert_eq!(
            b.on_event(PttKeyCode::LCtrl, false),
            Some(chord("LCtrl+LWin"))
        );
    }

    #[test]
    fn capture_records_a_single_key_chord() {
        let mut b = CaptureBuffer::new();
        assert_eq!(b.on_event(PttKeyCode::F(13), true), None);
        assert_eq!(b.on_event(PttKeyCode::F(13), false), Some(chord("F13")));
    }

    #[test]
    fn capture_dedupes_auto_repeat_and_caps_at_four() {
        let mut b = CaptureBuffer::new();
        b.on_event(PttKeyCode::LCtrl, true);
        // Windows repeats WM_KEYDOWN while held: no duplicate members.
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
        assert_eq!(
            b.on_event(PttKeyCode::LCtrl, false),
            Some(chord("LCtrl+LShift+LAlt+LWin"))
        );
    }

    #[test]
    fn capture_ignores_a_release_before_anything_is_held() {
        let mut b = CaptureBuffer::new();
        // A key already down when recording began releases first: not a chord.
        assert_eq!(b.on_event(PttKeyCode::LCtrl, false), None);
        // Recording then proceeds normally.
        assert_eq!(b.on_event(PttKeyCode::LWin, true), None);
        assert_eq!(b.on_event(PttKeyCode::LWin, false), Some(chord("LWin")));
    }
}

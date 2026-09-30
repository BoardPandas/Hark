//! Pure routing for the shared native keyboard hook (Windows, Linux, macOS).
//! The meeting chord only emits a toggle on its physical engage edge;
//! dictation keeps its full edge stream.

use crate::{ChordTracker, PttChord, PttEvent, PttKeyCode};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutEvent {
    Dictation(PttEvent),
    MeetingToggle,
}

pub(crate) struct ShortcutTracker {
    ptt: ChordTracker,
    meeting: Option<ChordTracker>,
}

impl ShortcutTracker {
    pub(crate) fn new(ptt: PttChord, swallow_locks: bool, meeting: Option<PttChord>) -> Self {
        Self {
            ptt: ChordTracker::with_lock_suppression(ptt, swallow_locks),
            meeting: meeting.map(ChordTracker::new),
        }
    }

    pub(crate) fn on_event(
        &mut self,
        key: PttKeyCode,
        down: bool,
        injected: bool,
        mut physical: impl FnMut(PttKeyCode) -> bool,
        now: Instant,
    ) -> [Option<ShortcutEvent>; 2] {
        let ptt = self
            .ptt
            .on_event_verified(key, down, injected, &mut physical, now);
        let meeting = self
            .meeting
            .as_mut()
            .and_then(|tracker| tracker.on_event_verified(key, down, injected, physical, now));
        [
            ptt.map(ShortcutEvent::Dictation),
            (meeting == Some(PttEvent::Down)).then_some(ShortcutEvent::MeetingToggle),
        ]
    }

    /// Heal releases for both trackers, but only dictation needs an outgoing
    /// release event. Never invent a meeting toggle from a physical-state poll.
    pub(crate) fn resync(
        &mut self,
        mut physical: impl FnMut(PttKeyCode) -> bool,
        now: Instant,
    ) -> Option<ShortcutEvent> {
        let ptt = self.ptt.resync_released(&mut physical, now);
        if let Some(meeting) = &mut self.meeting {
            meeting.resync_released(physical, now);
        }
        ptt.map(ShortcutEvent::Dictation)
    }

    pub(crate) fn engaged(&self) -> bool {
        self.ptt.is_engaged() || self.meeting.as_ref().is_some_and(ChordTracker::is_engaged)
    }

    // Windows-only: the Linux hook cannot swallow a key without grabbing the
    // whole device, so it never asks.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn swallow(&self, key: PttKeyCode, down: bool, injected: bool) -> bool {
        self.ptt.swallow(key, down, injected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use PttKeyCode as K;

    fn tracker() -> ShortcutTracker {
        ShortcutTracker::new(
            PttChord::parse("LCtrl+F12").unwrap(),
            true,
            Some(PttChord::parse("LCtrl+F11").unwrap()),
        )
    }

    fn press(t: &mut ShortcutTracker, key: PttKeyCode, down: bool) -> Vec<ShortcutEvent> {
        t.on_event(key, down, false, |_| true, Instant::now())
            .into_iter()
            .flatten()
            .collect()
    }

    #[test]
    fn conflicts_are_order_independent_and_include_subsets() {
        let ptt = PttChord::parse("LCtrl+LWin").unwrap();
        for other in ["lwin+lctrl", "LCtrl+LWin+M", "LCtrl", "LCtrl+LWin+LCtrl"] {
            let other = PttChord::parse(other).unwrap();
            assert!(ptt.conflicts_with(&other));
            assert!(other.conflicts_with(&ptt));
        }
        assert!(!ptt.conflicts_with(&PttChord::parse("LCtrl+F11").unwrap()));
    }

    #[test]
    fn meeting_engage_toggles_once_and_never_emits_dictation_edges() {
        let mut t = tracker();
        assert!(press(&mut t, K::LCtrl, true).is_empty());
        assert_eq!(press(&mut t, K::F11, true), [ShortcutEvent::MeetingToggle]);
        assert!(press(&mut t, K::F11, true).is_empty());
        assert!(press(&mut t, K::F11, false).is_empty());
        assert_eq!(press(&mut t, K::F11, true), [ShortcutEvent::MeetingToggle]);
    }

    #[test]
    fn watchdog_stays_armed_until_both_chords_release() {
        let mut t = tracker();
        press(&mut t, K::LCtrl, true);
        assert_eq!(
            press(&mut t, K::F12, true),
            [ShortcutEvent::Dictation(PttEvent::Down)]
        );
        assert_eq!(press(&mut t, K::F11, true), [ShortcutEvent::MeetingToggle]);
        assert_eq!(
            press(&mut t, K::F12, false),
            [ShortcutEvent::Dictation(PttEvent::Up)]
        );
        assert!(t.engaged());
        press(&mut t, K::F11, false);
        assert!(!t.engaged());
    }

    #[test]
    fn missed_meeting_release_heals_without_toggling_and_next_press_works() {
        let mut t = tracker();
        press(&mut t, K::LCtrl, true);
        press(&mut t, K::F11, true);
        // Confirm the held keys once, then the next poll sees their release.
        assert_eq!(t.resync(|_| true, Instant::now()), None);
        assert_eq!(t.resync(|_| false, Instant::now()), None);
        assert!(!t.engaged());
        press(&mut t, K::LCtrl, true);
        assert_eq!(press(&mut t, K::F11, true), [ShortcutEvent::MeetingToggle]);
    }

    #[test]
    fn injected_keys_and_stale_other_members_cannot_toggle() {
        let mut t = tracker();
        for key in [K::LCtrl, K::F11] {
            assert_eq!(
                t.on_event(key, true, true, |_| true, Instant::now()),
                [None, None]
            );
        }
        press(&mut t, K::LCtrl, true);
        // After held-evidence expires, a missed modifier release cannot turn
        // the meeting shortcut into a bare F11 key.
        let later = Instant::now() + crate::edges::HELD_EVIDENCE * 2;
        assert_eq!(
            t.on_event(K::F11, true, false, |_| false, later),
            [None, None]
        );
        assert!(!t.engaged());
    }
}

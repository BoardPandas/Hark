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

    /// Replay a kernel edge before the remaining buffered transitions. A
    /// member's next release proves it is held now; its next press proves it
    /// is up now. Only members without a queued transition use the live poll,
    /// preserving stale-member rejection without applying future releases to
    /// an earlier shortcut. Linux drops auto-repeat before this boundary.
    #[cfg(any(target_os = "linux", test))]
    pub(crate) fn on_buffered_event(
        &mut self,
        key: PttKeyCode,
        down: bool,
        remaining: &[(PttKeyCode, bool)],
        mut physical: impl FnMut(PttKeyCode) -> bool,
        now: Instant,
    ) -> [Option<ShortcutEvent>; 2] {
        self.on_event(
            key,
            down,
            false,
            |member| {
                remaining
                    .iter()
                    .find(|(next, _)| *next == member)
                    .map_or_else(|| physical(member), |(_, next_down)| !*next_down)
            },
            now,
        )
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

    fn buffered(
        t: &mut ShortcutTracker,
        edges: &[(PttKeyCode, bool)],
        physically_held: bool,
    ) -> Vec<ShortcutEvent> {
        let now = Instant::now();
        edges
            .iter()
            .enumerate()
            .flat_map(|(index, &(key, down))| {
                t.on_buffered_event(key, down, &edges[index + 1..], |_| physically_held, now)
                    .into_iter()
                    .flatten()
            })
            .collect()
    }

    #[test]
    fn buffered_complete_dictation_survives_final_released_key_state() {
        let mut t = tracker();
        let edges = [
            (K::LCtrl, true),
            (K::F12, true),
            (K::LCtrl, false),
            (K::F12, false),
        ];
        assert_eq!(
            buffered(&mut t, &edges, false),
            [
                ShortcutEvent::Dictation(PttEvent::Down),
                ShortcutEvent::Dictation(PttEvent::Up),
            ]
        );
        assert!(!t.engaged());
    }

    #[test]
    fn buffered_meeting_toggle_fires_once_with_interleaved_dictation() {
        let mut t = tracker();
        let edges = [
            (K::LCtrl, true),
            (K::F12, true),
            (K::F11, true),
            (K::F11, true),
            (K::F12, false),
            (K::F11, false),
            (K::LCtrl, false),
        ];
        assert_eq!(
            buffered(&mut t, &edges, false),
            [
                ShortcutEvent::Dictation(PttEvent::Down),
                ShortcutEvent::MeetingToggle,
                ShortcutEvent::Dictation(PttEvent::Up),
            ]
        );
        assert!(!t.engaged());
    }

    #[test]
    fn buffered_release_preserves_a_modifier_pressed_in_an_earlier_batch() {
        let mut t = tracker();
        press(&mut t, K::LCtrl, true);
        let edges = [(K::F11, true), (K::LCtrl, false), (K::F11, false)];
        assert_eq!(
            buffered(&mut t, &edges, false),
            [ShortcutEvent::MeetingToggle]
        );
        assert!(!t.engaged());
    }

    #[test]
    fn buffered_events_reject_a_stale_modifier_without_a_queued_transition() {
        let mut t = tracker();
        press(&mut t, K::LCtrl, true);
        // A missed release, e.g. after device removal, is still checked when
        // a subsequent batch tries to complete the shortcut with a bare F11.
        assert!(buffered(&mut t, &[(K::F11, true), (K::F11, false)], false).is_empty());
        assert!(!t.engaged());
    }

    #[test]
    fn buffered_later_press_cannot_retroactively_complete_an_earlier_chord() {
        let mut t = tracker();
        press(&mut t, K::LCtrl, true);
        // Ctrl's old release was missed. Its next press proves it was not
        // held during the preceding F11 tap, even if the final poll says down.
        let edges = [(K::F11, true), (K::F11, false), (K::LCtrl, true)];
        assert!(buffered(&mut t, &edges, true).is_empty());
        assert!(!t.engaged());
    }

    #[test]
    fn buffered_holds_still_heal_missed_releases_without_a_meeting_toggle() {
        let mut t = tracker();
        let edges = [(K::LCtrl, true), (K::F12, true), (K::F11, true)];
        assert_eq!(
            buffered(&mut t, &edges, true),
            [
                ShortcutEvent::Dictation(PttEvent::Down),
                ShortcutEvent::MeetingToggle
            ]
        );
        assert_eq!(
            t.resync(|_| false, Instant::now()),
            Some(ShortcutEvent::Dictation(PttEvent::UpMissed))
        );
        assert!(!t.engaged());
        assert_eq!(t.resync(|_| false, Instant::now()), None);
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

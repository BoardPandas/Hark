//! Pure chord edge detection: a stream of raw per-key events becomes clean
//! `PttEvent::Down` / `PttEvent::Up` edges for the configured chord.
//!
//! Semantics (default chord LCtrl+LWin, confirmed 2026-07-16):
//! - `Down` fires when the LAST chord member goes down (all held).
//! - `Up` fires when the FIRST chord member is released.
//! - Auto-repeat (down while already down) never re-fires an edge.
//! - Injected events (our own synthesized Ctrl+V) are ignored entirely, or
//!   dictation would paste-inject into an infinite PTT loop.
//! - Keys outside the chord are ignored (we observe, never swallow).

use std::fmt;
use thiserror::Error;

/// A key that can participate in a push-to-talk chord. Deliberately small:
/// modifiers and function keys hold well; typing keys would fight text entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PttKeyCode {
    LCtrl,
    RCtrl,
    LShift,
    RShift,
    LAlt,
    RAlt,
    LWin,
    RWin,
    CapsLock,
    /// F1..=F24.
    F(u8),
}

impl fmt::Display for PttKeyCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PttKeyCode::LCtrl => write!(f, "LCtrl"),
            PttKeyCode::RCtrl => write!(f, "RCtrl"),
            PttKeyCode::LShift => write!(f, "LShift"),
            PttKeyCode::RShift => write!(f, "RShift"),
            PttKeyCode::LAlt => write!(f, "LAlt"),
            PttKeyCode::RAlt => write!(f, "RAlt"),
            PttKeyCode::LWin => write!(f, "LWin"),
            PttKeyCode::RWin => write!(f, "RWin"),
            PttKeyCode::CapsLock => write!(f, "CapsLock"),
            PttKeyCode::F(n) => write!(f, "F{n}"),
        }
    }
}

impl PttKeyCode {
    /// Is this a key people hold to modify another? Bare modifiers make poor
    /// shortcuts on their own: bind push-to-talk to one and every Ctrl press
    /// in every app starts a dictation. Caps Lock and F1..F24 are fine alone —
    /// nothing else is competing for them.
    pub fn is_modifier(&self) -> bool {
        matches!(
            self,
            PttKeyCode::LCtrl
                | PttKeyCode::RCtrl
                | PttKeyCode::LShift
                | PttKeyCode::RShift
                | PttKeyCode::LAlt
                | PttKeyCode::RAlt
                | PttKeyCode::LWin
                | PttKeyCode::RWin
        )
    }

    /// The name a person reads, as opposed to the config token `Display`
    /// writes. Config keeps "LCtrl" because it round-trips through TOML; the
    /// UI shows "Left Ctrl" because that is what the key is called.
    pub fn label(&self) -> String {
        match self {
            PttKeyCode::LCtrl => "Left Ctrl".to_string(),
            PttKeyCode::RCtrl => "Right Ctrl".to_string(),
            PttKeyCode::LShift => "Left Shift".to_string(),
            PttKeyCode::RShift => "Right Shift".to_string(),
            PttKeyCode::LAlt => "Left Alt".to_string(),
            PttKeyCode::RAlt => "Right Alt".to_string(),
            PttKeyCode::LWin => "Left Win".to_string(),
            PttKeyCode::RWin => "Right Win".to_string(),
            PttKeyCode::CapsLock => "Caps Lock".to_string(),
            PttKeyCode::F(n) => format!("F{n}"),
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ChordParseError {
    #[error("push-to-talk chord is empty")]
    Empty,
    #[error(
        "unknown key name \"{0}\" (valid: LCtrl, RCtrl, LShift, RShift, LAlt, RAlt, \
         LWin, RWin, CapsLock, F1..F24)"
    )]
    UnknownKey(String),
    #[error("chord has {0} keys; at most 4 are supported")]
    TooManyKeys(usize),
}

/// The configured push-to-talk chord: 1..=4 distinct keys, all of which must
/// be held simultaneously.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PttChord {
    keys: Vec<PttKeyCode>,
}

impl PttChord {
    /// Parse a config string like "LCtrl+LWin" or "F13". Case-insensitive,
    /// whitespace-tolerant, duplicates collapsed.
    pub fn parse(text: &str) -> Result<PttChord, ChordParseError> {
        let mut keys = Vec::new();
        for part in text.split('+') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let key =
                parse_key(part).ok_or_else(|| ChordParseError::UnknownKey(part.to_string()))?;
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        if keys.is_empty() {
            return Err(ChordParseError::Empty);
        }
        if keys.len() > 4 {
            return Err(ChordParseError::TooManyKeys(keys.len()));
        }
        Ok(PttChord { keys })
    }

    /// Build a chord straight from captured keys. The caller guarantees the
    /// slice is 1..=4 distinct keys (the key-capture UI enforces both via
    /// `CaptureBuffer`); this constructor does not re-validate.
    pub fn from_keys(keys: Vec<PttKeyCode>) -> PttChord {
        PttChord { keys }
    }

    pub fn keys(&self) -> &[PttKeyCode] {
        &self.keys
    }

    /// The single bare modifier this chord consists of, if that is all it is.
    /// Push-to-talk on a lone Ctrl/Shift/Alt/Win means every press of that key
    /// in every app opens the microphone, and it is uniquely miserable to undo
    /// because the fix has to be made in the app it just broke.
    pub fn lone_modifier(&self) -> Option<PttKeyCode> {
        match self.keys[..] {
            [only] if only.is_modifier() => Some(only),
            _ => None,
        }
    }

    /// "Left Ctrl + F12" — the form the settings page and the onboarding card
    /// show. `Display` stays the config form ("LCtrl+F12").
    pub fn pretty(&self) -> String {
        self.keys
            .iter()
            .map(|k| k.label())
            .collect::<Vec<_>>()
            .join(" + ")
    }
}

/// Human-readable rendering of a configured chord string, falling back to the
/// raw text when it does not parse (a hand-edited config.toml). Shared by
/// every place that shows the user their shortcut.
pub fn pretty_chord(text: &str) -> String {
    match PttChord::parse(text) {
        Ok(chord) => chord.pretty(),
        Err(_) => text.to_string(),
    }
}

impl fmt::Display for PttChord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, k) in self.keys.iter().enumerate() {
            if i > 0 {
                write!(f, "+")?;
            }
            write!(f, "{k}")?;
        }
        Ok(())
    }
}

fn parse_key(name: &str) -> Option<PttKeyCode> {
    let lower = name.to_ascii_lowercase();
    let key = match lower.as_str() {
        "lctrl" | "lcontrol" => PttKeyCode::LCtrl,
        "rctrl" | "rcontrol" => PttKeyCode::RCtrl,
        "lshift" => PttKeyCode::LShift,
        "rshift" => PttKeyCode::RShift,
        "lalt" => PttKeyCode::LAlt,
        "ralt" | "altgr" => PttKeyCode::RAlt,
        "lwin" | "lcmd" | "lsuper" => PttKeyCode::LWin,
        "rwin" | "rcmd" | "rsuper" => PttKeyCode::RWin,
        "capslock" => PttKeyCode::CapsLock,
        _ => {
            let n: u8 = lower.strip_prefix('f')?.parse().ok()?;
            if (1..=24).contains(&n) {
                PttKeyCode::F(n)
            } else {
                return None;
            }
        }
    };
    Some(key)
}

/// A push-to-talk edge, sent to the pipeline worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PttEvent {
    Down,
    Up,
    /// The chord is no longer physically held, but the release event itself
    /// never arrived: [`ChordTracker::resync_released`] found the keys up.
    /// Same meaning as `Up` for the state machine, but the moment of release
    /// is unknown — it can be anywhere between the last poll and the press —
    /// so the pipeline treats an over-long one as abandoned rather than
    /// injecting whatever the room said after the user let go.
    UpMissed,
}

/// The chord state machine. Feed every raw key event the platform hook sees;
/// it emits an edge only on engage/disengage transitions.
pub struct ChordTracker {
    chord: PttChord,
    member_down: Vec<bool>,
    engaged: bool,
}

impl ChordTracker {
    pub fn new(chord: PttChord) -> ChordTracker {
        let n = chord.keys.len();
        ChordTracker {
            chord,
            member_down: vec![false; n],
            engaged: false,
        }
    }

    /// Process one raw key event, trusting the tracker's own record of what is
    /// held. Pure: the platform is never consulted. Production feeds
    /// [`Self::on_event_verified`] instead; this is the seam the edge-semantics
    /// tests drive.
    pub fn on_event(&mut self, key: PttKeyCode, down: bool, injected: bool) -> Option<PttEvent> {
        self.on_event_verified(key, down, injected, |_| true)
    }

    /// Process one raw key event. `injected` marks synthesized input
    /// (LLKHF_INJECTED on Windows): always ignored, so our own Ctrl+V can
    /// never re-trigger PTT.
    ///
    /// `physically_down` answers "is this key really held right now?" and is
    /// consulted for the *other* chord members only, and only on a press that
    /// is about to engage the chord. A hook does not see every release (see
    /// [`Self::resync_released`]), and while the watchdog heals a lost release
    /// during a hold, a release lost with the chord already disengaged leaves
    /// its member stuck `true` with nothing polling — which quietly demotes the
    /// chord to whichever key is left. With LCtrl+F12 configured and F12's
    /// release missed, a bare Left Ctrl press then starts a dictation. The
    /// engage edge is the only place a stale `true` can do harm, and checking
    /// only there costs at most three key-state reads on the press that starts
    /// a dictation — nothing on any other key, and never a synthesized press
    /// (the poll can clear members, never set them).
    pub fn on_event_verified(
        &mut self,
        key: PttKeyCode,
        down: bool,
        injected: bool,
        mut physically_down: impl FnMut(PttKeyCode) -> bool,
    ) -> Option<PttEvent> {
        if injected {
            return None;
        }
        let idx = self.chord.keys.iter().position(|k| *k == key)?;

        if self.member_down[idx] == down {
            // Auto-repeat (down while down) or a stray duplicate release:
            // no state change, no edge.
            return None;
        }
        self.member_down[idx] = down;

        // About to engage: confirm the members we did not just see pressed are
        // genuinely still held. Skipped while engaged (the watchdog owns that
        // window) and on releases (they only ever clear state).
        if down && !self.engaged && self.member_down.iter().all(|d| *d) {
            for (i, member) in self.chord.keys.iter().enumerate() {
                if i != idx && !physically_down(*member) {
                    log::warn!("chord member {member} was marked held but is up; ignoring");
                    self.member_down[i] = false;
                }
            }
        }

        let all_down = self.member_down.iter().all(|d| *d);
        match (self.engaged, all_down) {
            (false, true) => {
                self.engaged = true;
                Some(PttEvent::Down)
            }
            (true, false) => {
                self.engaged = false;
                Some(PttEvent::Up)
            }
            _ => None,
        }
    }

    /// Reconcile the tracker with the keyboard's real state while the chord is
    /// engaged, and emit the release that never arrived.
    ///
    /// The tracker only ever learns about a release from a hook callback, and
    /// a low-level hook does not see every one: releases on another desktop
    /// (lock screen, UAC, Ctrl+Alt+Del), across a sleep/resume, or after
    /// Windows quietly unhooks a callback that ran long, all go missing. One
    /// lost release is not one lost dictation — it wedges the tracker
    /// `engaged` forever, so the recording never ends, the overlay never goes
    /// away, and no later press produces an edge either (the members it needs
    /// are already marked down). This is the way out: ask the platform which
    /// chord keys are *actually* held and heal the difference.
    ///
    /// Release-only on purpose. A missing press is harmless (nothing started),
    /// while synthesizing one from a poll would let a chord the user had
    /// already been holding before the hook existed start a dictation nobody
    /// asked for.
    pub fn resync_released(
        &mut self,
        mut physically_down: impl FnMut(PttKeyCode) -> bool,
    ) -> Option<PttEvent> {
        if !self.engaged {
            return None;
        }
        let mut released = false;
        for (idx, key) in self.chord.keys.iter().enumerate() {
            if self.member_down[idx] && !physically_down(*key) {
                self.member_down[idx] = false;
                released = true;
            }
        }
        if !released {
            return None;
        }
        // Engaged means every member was down, so any member now up breaks
        // the chord — the same "first release wins" rule `on_event` applies.
        self.engaged = false;
        Some(PttEvent::UpMissed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(text: &str) -> PttChord {
        PttChord::parse(text).unwrap()
    }

    #[test]
    fn parse_default_chord() {
        let c = chord("LCtrl+LWin");
        assert_eq!(c.keys(), &[PttKeyCode::LCtrl, PttKeyCode::LWin]);
        assert_eq!(c.to_string(), "LCtrl+LWin");
    }

    #[test]
    fn parse_is_case_insensitive_and_whitespace_tolerant() {
        assert_eq!(chord(" lctrl + lwin "), chord("LCtrl+LWin"));
        assert_eq!(chord("F13").keys(), &[PttKeyCode::F(13)]);
        assert_eq!(chord("altgr").keys(), &[PttKeyCode::RAlt]);
    }

    #[test]
    fn parse_rejects_unknown_and_empty() {
        assert_eq!(
            PttChord::parse("LCtrl+Banana").unwrap_err(),
            ChordParseError::UnknownKey("Banana".to_string())
        );
        assert_eq!(PttChord::parse("").unwrap_err(), ChordParseError::Empty);
        assert_eq!(PttChord::parse(" + ").unwrap_err(), ChordParseError::Empty);
        assert_eq!(
            PttChord::parse("F0").unwrap_err(),
            ChordParseError::UnknownKey("F0".to_string())
        );
        assert_eq!(
            PttChord::parse("F25").unwrap_err(),
            ChordParseError::UnknownKey("F25".to_string())
        );
    }

    #[test]
    fn parse_dedupes_and_caps_length() {
        assert_eq!(chord("LCtrl+LCtrl+LWin"), chord("LCtrl+LWin"));
        assert!(matches!(
            PttChord::parse("LCtrl+RCtrl+LShift+RShift+LAlt").unwrap_err(),
            ChordParseError::TooManyKeys(5)
        ));
    }

    #[test]
    fn single_key_chord_edges() {
        let mut t = ChordTracker::new(chord("RCtrl"));
        assert_eq!(
            t.on_event(PttKeyCode::RCtrl, true, false),
            Some(PttEvent::Down)
        );
        assert_eq!(
            t.on_event(PttKeyCode::RCtrl, false, false),
            Some(PttEvent::Up)
        );
    }

    #[test]
    fn chord_engages_on_last_member_down_releases_on_first_up() {
        let mut t = ChordTracker::new(chord("LCtrl+LWin"));
        // First member down: nothing yet.
        assert_eq!(t.on_event(PttKeyCode::LCtrl, true, false), None);
        // Second member down: chord engaged.
        assert_eq!(
            t.on_event(PttKeyCode::LWin, true, false),
            Some(PttEvent::Down)
        );
        // First release disengages...
        assert_eq!(
            t.on_event(PttKeyCode::LCtrl, false, false),
            Some(PttEvent::Up)
        );
        // ...and the second release is silent.
        assert_eq!(t.on_event(PttKeyCode::LWin, false, false), None);
    }

    #[test]
    fn order_of_presses_does_not_matter() {
        let mut t = ChordTracker::new(chord("LCtrl+LWin"));
        assert_eq!(t.on_event(PttKeyCode::LWin, true, false), None);
        assert_eq!(
            t.on_event(PttKeyCode::LCtrl, true, false),
            Some(PttEvent::Down)
        );
    }

    #[test]
    fn auto_repeat_is_suppressed() {
        let mut t = ChordTracker::new(chord("LCtrl+LWin"));
        t.on_event(PttKeyCode::LCtrl, true, false);
        t.on_event(PttKeyCode::LWin, true, false);
        // Holding keys makes Windows repeat WM_KEYDOWN for them.
        assert_eq!(t.on_event(PttKeyCode::LCtrl, true, false), None);
        assert_eq!(t.on_event(PttKeyCode::LWin, true, false), None);
        assert_eq!(t.on_event(PttKeyCode::LWin, true, false), None);
        // The chord is still engaged: release works normally.
        assert_eq!(
            t.on_event(PttKeyCode::LWin, false, false),
            Some(PttEvent::Up)
        );
    }

    #[test]
    fn injected_events_are_ignored() {
        let mut t = ChordTracker::new(chord("LCtrl+LWin"));
        t.on_event(PttKeyCode::LWin, true, false);
        // An injected LCtrl press (e.g. enigo synthesizing Ctrl+V while the
        // user still holds LWin) must NOT complete the chord.
        assert_eq!(t.on_event(PttKeyCode::LCtrl, true, true), None);
        // Nor may an injected release tear it down once engaged.
        assert_eq!(
            t.on_event(PttKeyCode::LCtrl, true, false),
            Some(PttEvent::Down)
        );
        assert_eq!(t.on_event(PttKeyCode::LCtrl, false, true), None);
        assert_eq!(
            t.on_event(PttKeyCode::LCtrl, false, false),
            Some(PttEvent::Up)
        );
    }

    #[test]
    fn non_chord_keys_are_ignored() {
        let mut t = ChordTracker::new(chord("LCtrl+LWin"));
        t.on_event(PttKeyCode::LCtrl, true, false);
        assert_eq!(t.on_event(PttKeyCode::RShift, true, false), None);
        assert_eq!(t.on_event(PttKeyCode::F(5), true, false), None);
        // Chord still completes normally afterwards.
        assert_eq!(
            t.on_event(PttKeyCode::LWin, true, false),
            Some(PttEvent::Down)
        );
    }

    #[test]
    fn resync_emits_the_release_the_hook_never_delivered() {
        let mut t = ChordTracker::new(chord("LCtrl+LWin"));
        t.on_event(PttKeyCode::LCtrl, true, false);
        assert_eq!(
            t.on_event(PttKeyCode::LWin, true, false),
            Some(PttEvent::Down)
        );
        // The user let go on the lock screen, so no release ever arrived.
        assert_eq!(t.resync_released(|_| false), Some(PttEvent::UpMissed));
        // Once healed the tracker is idle again: a second poll adds nothing,
        // and the next press engages normally rather than being swallowed as
        // a duplicate.
        assert_eq!(t.resync_released(|_| false), None);
        assert_eq!(t.on_event(PttKeyCode::LCtrl, true, false), None);
        assert_eq!(
            t.on_event(PttKeyCode::LWin, true, false),
            Some(PttEvent::Down)
        );
    }

    #[test]
    fn resync_leaves_a_chord_that_is_really_held_alone() {
        let mut t = ChordTracker::new(chord("LCtrl+LWin"));
        t.on_event(PttKeyCode::LCtrl, true, false);
        t.on_event(PttKeyCode::LWin, true, false);
        // Polled mid-hold, every 250 ms, for as long as the user talks.
        for _ in 0..8 {
            assert_eq!(t.resync_released(|_| true), None);
        }
        // The real release still produces the normal edge.
        assert_eq!(
            t.on_event(PttKeyCode::LWin, false, false),
            Some(PttEvent::Up)
        );
    }

    #[test]
    fn resync_never_starts_a_dictation() {
        // Keys held before the chord was ever tracked (or a press whose event
        // went missing) must not engage from a poll alone.
        let mut t = ChordTracker::new(chord("LCtrl+LWin"));
        assert_eq!(t.resync_released(|_| true), None);
        assert_eq!(t.resync_released(|_| false), None);
        // One member down is not the chord either.
        t.on_event(PttKeyCode::LCtrl, true, false);
        assert_eq!(t.resync_released(|_| true), None);
    }

    #[test]
    fn resync_after_a_normal_release_is_silent() {
        let mut t = ChordTracker::new(chord("LCtrl+LWin"));
        t.on_event(PttKeyCode::LCtrl, true, false);
        t.on_event(PttKeyCode::LWin, true, false);
        assert_eq!(
            t.on_event(PttKeyCode::LCtrl, false, false),
            Some(PttEvent::Up)
        );
        // The other member is still physically down; no second edge.
        assert_eq!(t.resync_released(|k| k == PttKeyCode::LWin), None);
    }

    /// The bug this guards: a release the hook never saw, with the chord
    /// already disengaged, leaves that member stuck `true`. Nothing polls in
    /// that state (the watchdog runs only during a hold), so the chord silently
    /// becomes a one-key hotkey — press Left Ctrl alone and a dictation starts.
    #[test]
    fn a_member_whose_release_was_missed_cannot_engage_the_chord_alone() {
        let mut t = ChordTracker::new(chord("LCtrl+F12"));
        t.on_event(PttKeyCode::LCtrl, true, false);
        assert_eq!(
            t.on_event(PttKeyCode::F(12), true, false),
            Some(PttEvent::Down)
        );
        // LCtrl's release arrives; F12's never does (an Fn-layer keyboard
        // reporting the break under a different key, a lock screen, a sleep).
        assert_eq!(
            t.on_event(PttKeyCode::LCtrl, false, false),
            Some(PttEvent::Up)
        );

        // Later, with nothing physically held, Left Ctrl alone must do nothing.
        assert_eq!(
            t.on_event_verified(PttKeyCode::LCtrl, true, false, |_| false),
            None
        );
        // ...and the chord still works normally afterwards: the stale member
        // was cleared, not left to poison every future press.
        assert_eq!(
            t.on_event_verified(PttKeyCode::F(12), true, false, |_| true),
            Some(PttEvent::Down)
        );
    }

    #[test]
    fn a_genuinely_held_member_still_engages() {
        let mut t = ChordTracker::new(chord("LCtrl+F12"));
        // The user really is holding LCtrl when F12 goes down. The key in the
        // event itself is never polled — a low-level hook runs before the
        // platform's key state catches up, so F12 may still read as up here.
        t.on_event_verified(PttKeyCode::LCtrl, true, false, |_| false);
        assert_eq!(
            t.on_event_verified(PttKeyCode::F(12), true, false, |k| k == PttKeyCode::LCtrl),
            Some(PttEvent::Down)
        );
    }

    /// The poll may clear members, never set them: a chord the user was already
    /// holding before Hark started must not launch a dictation on its own.
    #[test]
    fn verification_never_manufactures_an_engage() {
        let mut t = ChordTracker::new(chord("LCtrl+LWin"));
        assert_eq!(
            t.on_event_verified(PttKeyCode::LCtrl, true, false, |_| true),
            None
        );
    }

    #[test]
    fn stray_release_without_press_is_silent() {
        let mut t = ChordTracker::new(chord("LCtrl+LWin"));
        assert_eq!(t.on_event(PttKeyCode::LCtrl, false, false), None);
        assert_eq!(t.on_event(PttKeyCode::LWin, false, false), None);
    }

    #[test]
    fn partial_release_and_repress_re_engages() {
        let mut t = ChordTracker::new(chord("LCtrl+LWin"));
        t.on_event(PttKeyCode::LCtrl, true, false);
        assert_eq!(
            t.on_event(PttKeyCode::LWin, true, false),
            Some(PttEvent::Down)
        );
        assert_eq!(
            t.on_event(PttKeyCode::LWin, false, false),
            Some(PttEvent::Up)
        );
        // LCtrl never left: pressing LWin again re-engages (a quick second
        // dictation without fully letting go).
        assert_eq!(
            t.on_event(PttKeyCode::LWin, true, false),
            Some(PttEvent::Down)
        );
    }

    #[test]
    fn from_keys_round_trips_through_parse() {
        let c = PttChord::from_keys(vec![PttKeyCode::LCtrl, PttKeyCode::LWin]);
        assert_eq!(c.to_string(), "LCtrl+LWin");
        assert_eq!(PttChord::parse(&c.to_string()).unwrap(), c);
    }
}

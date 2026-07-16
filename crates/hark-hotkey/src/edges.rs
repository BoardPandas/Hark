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

    pub fn keys(&self) -> &[PttKeyCode] {
        &self.keys
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

    /// Process one raw key event. `injected` marks synthesized input
    /// (LLKHF_INJECTED on Windows): always ignored, so our own Ctrl+V can
    /// never re-trigger PTT.
    pub fn on_event(&mut self, key: PttKeyCode, down: bool, injected: bool) -> Option<PttEvent> {
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
}

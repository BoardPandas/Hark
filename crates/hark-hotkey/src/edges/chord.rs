use crate::capture::Rejected;
use crate::keycode::{parse_key, KeyClass, PttKeyCode};
use std::fmt;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ChordParseError {
    #[error("push-to-talk chord is empty")]
    Empty,
    #[error(
        "unknown key name \"{0}\" (modifiers LCtrl/RCtrl/LShift/RShift/LAlt/RAlt/\
         LWin/RWin, F1..F24, letters, digits, arrows, the navigation and numpad \
         blocks, punctuation, and the lock keys)"
    )]
    UnknownKey(String),
    #[error("chord has {0} keys; at most 4 are supported")]
    TooManyKeys(usize),
}

/// The configured push-to-talk chord: 1..=4 distinct keys, all of which must
/// be held simultaneously.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PttChord {
    pub(super) keys: Vec<PttKeyCode>,
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

    /// Whether either chord can engage while pressing the other. Trackers
    /// ignore unrelated keys, so equality and subset/superset both conflict.
    pub fn conflicts_with(&self, other: &Self) -> bool {
        self.keys.iter().all(|key| other.keys.contains(key))
            || other.keys.iter().all(|key| self.keys.contains(key))
    }

    /// Why this chord cannot be bound, if it cannot.
    ///
    /// Policy, not syntax: [`Self::parse`] stays a pure syntax check, so a
    /// hand-edited config can never hard-fail startup and leave the user with
    /// no push-to-talk at all. The recorder refuses these outright; the typed
    /// field warns and lets the user proceed.
    ///
    /// The principle: refuse chords that occur as a byproduct of ordinary
    /// writing, allow chords that only ever happen on purpose. Ctrl, Alt and
    /// Win are command modifiers — nobody holds them while writing prose — so
    /// they lift a chord clear of typing. Shift does not, and that is the
    /// subtle one: Shift+A is a capital A, Shift+Left selects a character,
    /// Shift+Enter is a soft line break in every chat box. A chord qualified
    /// only by Shift fires while the user is simply writing.
    pub fn rejection(&self) -> Option<Rejected> {
        if let [only] = self.keys[..] {
            if only.is_modifier() {
                return Some(Rejected::LoneModifier(only));
            }
        }
        // Hark pastes with Ctrl+V (Command+V on macOS). A chord built on it fights its own
        // injection — and V only became bindable when the key set opened up.
        let has_paste_modifier = self.keys.iter().any(|k| {
            if cfg!(target_os = "macos") {
                matches!(k, PttKeyCode::LWin | PttKeyCode::RWin)
            } else {
                matches!(k, PttKeyCode::LCtrl | PttKeyCode::RCtrl)
            }
        });
        if has_paste_modifier && self.keys.contains(&PttKeyCode::V) {
            return Some(Rejected::CollidesWithInjection);
        }
        if self.keys.iter().any(|k| k.is_command_modifier()) {
            return None;
        }
        // No command modifier, so Shift is the only modifier that can be here,
        // which is why it is the only one worth naming in the message.
        let with_shift = self.keys.iter().any(|k| k.is_modifier());
        if let Some(&key) = self
            .keys
            .iter()
            .find(|k| matches!(k.class(), KeyClass::Typing))
        {
            return Some(Rejected::TypesText { key, with_shift });
        }
        if let Some(&key) = self
            .keys
            .iter()
            .find(|k| matches!(k.class(), KeyClass::Navigation))
        {
            return Some(Rejected::MovesTheCaret { key, with_shift });
        }
        None
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edges::tests::chord;

    #[test]
    fn parse_default_chord() {
        let c = chord("LCtrl+LWin");
        assert_eq!(c.keys(), &[PttKeyCode::LCtrl, PttKeyCode::LWin]);
        assert_eq!(c.to_string(), "LCtrl+LWin");
    }

    #[test]
    fn parse_is_case_insensitive_and_whitespace_tolerant() {
        assert_eq!(chord(" lctrl + lwin "), chord("LCtrl+LWin"));
        assert_eq!(chord("F13").keys(), &[PttKeyCode::F13]);
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

    /// Shift is a *typing* modifier: holding it is how you write a capital, so
    /// it must not lift a chord out of ordinary typing the way Ctrl/Alt/Win do.
    /// Without this, Shift+A binds dictation to every capital A you type.
    #[test]
    fn shift_does_not_qualify_a_typing_chord() {
        assert!(matches!(
            chord("LShift+A").rejection(),
            Some(Rejected::TypesText {
                key: PttKeyCode::A,
                with_shift: true
            })
        ));
        assert!(matches!(
            chord("RShift+Enter").rejection(),
            Some(Rejected::TypesText {
                with_shift: true,
                ..
            })
        ));
        assert!(matches!(
            chord("LShift+Left").rejection(),
            Some(Rejected::MovesTheCaret {
                with_shift: true,
                ..
            })
        ));
        // ...but Shift with a key nothing competes for is not typing.
        assert_eq!(chord("LShift+F5").rejection(), None);
        assert_eq!(chord("LShift+RShift").rejection(), None);
    }

    #[test]
    fn a_command_modifier_makes_any_key_bindable() {
        for text in ["LCtrl+A", "RAlt+Space", "LWin+Left", "LCtrl+LShift+Numpad4"] {
            assert_eq!(chord(text).rejection(), None, "{text} should be allowed");
        }
    }

    #[test]
    fn bare_typing_and_navigation_keys_are_refused() {
        for text in ["A", "Space", "Enter", "Tab", "Semicolon", "Numpad4", "5"] {
            assert!(
                matches!(chord(text).rejection(), Some(Rejected::TypesText { .. })),
                "{text} should be refused as typing"
            );
        }
        for text in ["Left", "Home", "PageDown", "Insert"] {
            assert!(
                matches!(
                    chord(text).rejection(),
                    Some(Rejected::MovesTheCaret { .. })
                ),
                "{text} should be refused as navigation"
            );
        }
        // Keys nothing competes for stay bindable alone, as they always were.
        for text in ["F13", "CapsLock", "NumLock", "ScrollLock"] {
            assert_eq!(chord(text).rejection(), None, "{text} should be allowed");
        }
    }

    /// Hark pastes with Ctrl+V. Binding push-to-talk to it would have the
    /// dictation fight its own injection -- newly reachable now that V binds.
    #[test]
    fn platform_paste_chord_collides_with_harks_own_paste() {
        let left = if cfg!(target_os = "macos") {
            "LWin+V"
        } else {
            "LCtrl+V"
        };
        let right = if cfg!(target_os = "macos") {
            "RWin+LShift+V"
        } else {
            "RCtrl+LShift+V"
        };
        assert_eq!(
            chord(left).rejection(),
            Some(Rejected::CollidesWithInjection)
        );
        assert_eq!(
            chord(right).rejection(),
            Some(Rejected::CollidesWithInjection)
        );
        // Without Ctrl there is no collision.
        assert_eq!(chord("LAlt+V").rejection(), None);
    }

    #[test]
    fn the_shipped_defaults_and_existing_configs_stay_valid() {
        for text in [
            "LCtrl+LWin",
            "LCtrl+F12",
            "LWin+LCtrl+LShift",
            "RCtrl",
            "F13",
        ] {
            let c = chord(text);
            // Round-trips through the config form...
            assert_eq!(PttChord::parse(&c.to_string()).unwrap(), c, "{text}");
            // ...and only the lone bare modifier is refused, as before.
            let expected = if text == "RCtrl" {
                Some(Rejected::LoneModifier(PttKeyCode::RCtrl))
            } else {
                None
            };
            assert_eq!(c.rejection(), expected, "{text}");
        }
    }

    #[test]
    fn from_keys_round_trips_through_parse() {
        let c = PttChord::from_keys(vec![PttKeyCode::LCtrl, PttKeyCode::LWin]);
        assert_eq!(c.to_string(), "LCtrl+LWin");
        assert_eq!(PttChord::parse(&c.to_string()).unwrap(), c);
    }
}

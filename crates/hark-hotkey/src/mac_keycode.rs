//! Quartz virtual key positions from HIToolbox/Events.h. Config tokens remain
//! portable; Win means Command and Alt means Option on a Mac.
use crate::PttKeyCode as K;

const KEYS: &[(K, u16)] = &[
    (K::A, 0x00),
    (K::S, 0x01),
    (K::D, 0x02),
    (K::F, 0x03),
    (K::H, 0x04),
    (K::G, 0x05),
    (K::Z, 0x06),
    (K::X, 0x07),
    (K::C, 0x08),
    (K::V, 0x09),
    (K::Oem102, 0x0a),
    (K::B, 0x0b),
    (K::Q, 0x0c),
    (K::W, 0x0d),
    (K::E, 0x0e),
    (K::R, 0x0f),
    (K::Y, 0x10),
    (K::T, 0x11),
    (K::Digit1, 0x12),
    (K::Digit2, 0x13),
    (K::Digit3, 0x14),
    (K::Digit4, 0x15),
    (K::Digit6, 0x16),
    (K::Digit5, 0x17),
    (K::Equals, 0x18),
    (K::Digit9, 0x19),
    (K::Digit7, 0x1a),
    (K::Minus, 0x1b),
    (K::Digit8, 0x1c),
    (K::Digit0, 0x1d),
    (K::RightBracket, 0x1e),
    (K::O, 0x1f),
    (K::U, 0x20),
    (K::LeftBracket, 0x21),
    (K::I, 0x22),
    (K::P, 0x23),
    (K::Enter, 0x24),
    (K::L, 0x25),
    (K::J, 0x26),
    (K::Quote, 0x27),
    (K::K, 0x28),
    (K::Semicolon, 0x29),
    (K::Backslash, 0x2a),
    (K::Comma, 0x2b),
    (K::Slash, 0x2c),
    (K::N, 0x2d),
    (K::M, 0x2e),
    (K::Period, 0x2f),
    (K::Tab, 0x30),
    (K::Space, 0x31),
    (K::Backtick, 0x32),
    (K::Backspace, 0x33),
    (K::RWin, 0x36),
    (K::LWin, 0x37),
    (K::LShift, 0x38),
    (K::LAlt, 0x3a),
    (K::LCtrl, 0x3b),
    (K::RShift, 0x3c),
    (K::RAlt, 0x3d),
    (K::RCtrl, 0x3e),
    (K::F17, 0x40),
    (K::NumpadDecimal, 0x41),
    (K::NumpadMultiply, 0x43),
    (K::NumpadAdd, 0x45),
    (K::NumLock, 0x47),
    (K::NumpadDivide, 0x4b),
    (K::NumpadSubtract, 0x4e),
    (K::F18, 0x4f),
    (K::F19, 0x50),
    (K::Numpad0, 0x52),
    (K::Numpad1, 0x53),
    (K::Numpad2, 0x54),
    (K::Numpad3, 0x55),
    (K::Numpad4, 0x56),
    (K::Numpad5, 0x57),
    (K::Numpad6, 0x58),
    (K::Numpad7, 0x59),
    (K::F20, 0x5a),
    (K::Numpad8, 0x5b),
    (K::Numpad9, 0x5c),
    (K::F5, 0x60),
    (K::F6, 0x61),
    (K::F7, 0x62),
    (K::F3, 0x63),
    (K::F8, 0x64),
    (K::F9, 0x65),
    (K::F11, 0x67),
    (K::F13, 0x69),
    (K::F16, 0x6a),
    (K::F14, 0x6b),
    (K::F10, 0x6d),
    (K::Apps, 0x6e),
    (K::F12, 0x6f),
    (K::F15, 0x71),
    (K::Insert, 0x72),
    (K::Home, 0x73),
    (K::PageUp, 0x74),
    (K::Delete, 0x75),
    (K::F4, 0x76),
    (K::End, 0x77),
    (K::F2, 0x78),
    (K::PageDown, 0x79),
    (K::F1, 0x7a),
    (K::Left, 0x7b),
    (K::Right, 0x7c),
    (K::Down, 0x7d),
    (K::Up, 0x7e),
];

pub(crate) fn from_code(code: u16) -> Option<K> {
    // Like Windows, Hark treats main and keypad Enter as one binding.
    if code == 0x4c {
        return Some(K::Enter);
    }
    KEYS.iter()
        .find(|(_, value)| *value == code)
        .map(|(key, _)| *key)
}

pub(crate) fn to_code(key: K) -> Option<u16> {
    KEYS.iter()
        .find(|(value, _)| *value == key)
        .map(|(_, code)| *code)
}

/// Device-specific modifier flags from IOKit/hidsystem/IOLLEvent.h. The
/// aggregate Control/Shift/Option/Command flags cannot distinguish releasing
/// one side while the other is still held.
pub(crate) fn modifier_down(key: K, flags: u64) -> Option<bool> {
    let mask = match key {
        K::LCtrl => 0x0001,
        K::LShift => 0x0002,
        K::RShift => 0x0004,
        K::LWin => 0x0008,
        K::RWin => 0x0010,
        K::LAlt => 0x0020,
        K::RAlt => 0x0040,
        K::RCtrl => 0x2000,
        _ => return None,
    };
    Some(flags & mask != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mappings_round_trip_and_missing_keys_are_explicit() {
        for key in crate::ALL_KEYS {
            if let Some(code) = to_code(key) {
                assert_eq!(from_code(code), Some(key));
            } else {
                // Quartz reports the Caps Lock toggle, not physical release.
                assert!(
                    matches!(
                        key,
                        K::CapsLock | K::ScrollLock | K::F21 | K::F22 | K::F23 | K::F24 | K::Oem8
                    ),
                    "{key}"
                );
            }
        }
        assert_eq!(from_code(0x4c), Some(K::Enter));
        assert_eq!(from_code(0x35), None); // Escape cancels the recorder.
        assert_eq!(from_code(0xffff), None);
    }

    #[test]
    fn releasing_one_modifier_side_does_not_release_the_other() {
        assert_eq!(modifier_down(K::LCtrl, 0x2001), Some(true));
        assert_eq!(modifier_down(K::LCtrl, 0x2000), Some(false));
        assert_eq!(modifier_down(K::RCtrl, 0x2000), Some(true));
        assert_eq!(modifier_down(K::LWin, 0x0010), Some(false));
        assert_eq!(modifier_down(K::RWin, 0x0010), Some(true));
        assert_eq!(modifier_down(K::A, u64::MAX), None);
    }
}

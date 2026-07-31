//! Every key a push-to-talk chord can contain, and what kind of key it is.
//!
//! Split out of `edges.rs` because it is now the largest thing in the crate and
//! entirely mechanical: one variant per Win32 virtual key, generated from a
//! single table so the enum, the scanner's list, the config spelling, the UI
//! label and the safety class cannot drift apart.
//!
//! **Flat and fieldless on purpose.** A parameterised `Letter(char)` makes
//! `Letter('a') != Letter('A')` constructible, and `ChordTracker` matches
//! members by equality — a case mismatch between `parse` and the hook would be
//! push-to-talk silently dead with no error anywhere.
//!
//! **Punctuation is named for its US ANSI *position*, not the glyph on the
//! cap**, the way `kVK_ANSI_Semicolon` and the W3C `code` values do. Windows
//! assigns these through the active layout, so on a German keyboard the
//! `Semicolon` key types `Ü`; the token still means that key. `Oem8` and
//! `Oem102` keep constant-derived names because neither exists on a US ANSI
//! board to be named after.
//!
//! **Granularity invariant, load-bearing: one variant per virtual key, never
//! finer.** The scanner and the push-to-talk watchdog both resolve state
//! through `GetAsyncKeyState(vk)`, which cannot see `LLKHF_EXTENDED`. Anything
//! this enum distinguished that a virtual key does not, the hook and the
//! scanner would disagree about, and the scanner would manufacture phantom
//! edges. That is why there is no separate numpad Enter.

use std::fmt;

/// What a key is for. Drives which chords are safe to bind (see
/// [`crate::PttChord::rejection`]), not what the key does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyClass {
    /// Ctrl / Shift / Alt / Win.
    Modifier,
    /// Produces or destroys text. Delete is here, not in `Navigation`: it does
    /// not move the caret, it eats what is under it.
    Typing,
    /// Moves the caret or the view.
    Navigation,
    /// Nothing else competes for it: F1..F24, the locks, the Menu key.
    Dedicated,
}

/// Every key that can take part in a chord.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
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
    NumLock,
    ScrollLock,
    Apps,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    F13,
    F14,
    F15,
    F16,
    F17,
    F18,
    F19,
    F20,
    F21,
    F22,
    F23,
    F24,
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    Digit0,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,
    Left,
    Right,
    Up,
    Down,
    Insert,
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
    Tab,
    Space,
    Enter,
    Backspace,
    Numpad0,
    Numpad1,
    Numpad2,
    Numpad3,
    Numpad4,
    Numpad5,
    Numpad6,
    Numpad7,
    Numpad8,
    Numpad9,
    NumpadAdd,
    NumpadSubtract,
    NumpadMultiply,
    NumpadDivide,
    NumpadDecimal,
    Equals,
    Comma,
    Minus,
    Period,
    Semicolon,
    Slash,
    Backtick,
    LeftBracket,
    Backslash,
    RightBracket,
    Quote,
    Oem8,
    Oem102,
}

use PttKeyCode as K;

impl PttKeyCode {
    /// Number of variants; [`ALL_KEYS`] is sized off it.
    pub const COUNT: usize = 114;

    /// Dense index, for array-indexed state. Never persisted — config uses
    /// [`Self::token`] — so reordering the enum is safe.
    #[inline]
    pub const fn ordinal(self) -> usize {
        self as usize
    }

    pub const fn class(self) -> KeyClass {
        match self {
            K::LCtrl | K::RCtrl | K::LShift | K::RShift | K::LAlt | K::RAlt | K::LWin | K::RWin => {
                KeyClass::Modifier
            }
            K::CapsLock
            | K::NumLock
            | K::ScrollLock
            | K::Apps
            | K::F1
            | K::F2
            | K::F3
            | K::F4
            | K::F5
            | K::F6
            | K::F7
            | K::F8
            | K::F9
            | K::F10
            | K::F11
            | K::F12
            | K::F13
            | K::F14
            | K::F15
            | K::F16
            | K::F17
            | K::F18
            | K::F19
            | K::F20
            | K::F21
            | K::F22
            | K::F23
            | K::F24 => KeyClass::Dedicated,
            K::A
            | K::B
            | K::C
            | K::D
            | K::E
            | K::F
            | K::G
            | K::H
            | K::I
            | K::J
            | K::K
            | K::L
            | K::M
            | K::N
            | K::O
            | K::P
            | K::Q
            | K::R
            | K::S
            | K::T
            | K::U
            | K::V
            | K::W
            | K::X
            | K::Y
            | K::Z
            | K::Digit0
            | K::Digit1
            | K::Digit2
            | K::Digit3
            | K::Digit4
            | K::Digit5
            | K::Digit6
            | K::Digit7
            | K::Digit8
            | K::Digit9
            | K::Delete
            | K::Tab
            | K::Space
            | K::Enter
            | K::Backspace
            | K::Numpad0
            | K::Numpad1
            | K::Numpad2
            | K::Numpad3
            | K::Numpad4
            | K::Numpad5
            | K::Numpad6
            | K::Numpad7
            | K::Numpad8
            | K::Numpad9
            | K::NumpadAdd
            | K::NumpadSubtract
            | K::NumpadMultiply
            | K::NumpadDivide
            | K::NumpadDecimal
            | K::Equals
            | K::Comma
            | K::Minus
            | K::Period
            | K::Semicolon
            | K::Slash
            | K::Backtick
            | K::LeftBracket
            | K::Backslash
            | K::RightBracket
            | K::Quote
            | K::Oem8
            | K::Oem102 => KeyClass::Typing,
            K::Left
            | K::Right
            | K::Up
            | K::Down
            | K::Insert
            | K::Home
            | K::End
            | K::PageUp
            | K::PageDown => KeyClass::Navigation,
        }
    }

    pub const fn is_modifier(self) -> bool {
        matches!(self.class(), KeyClass::Modifier)
    }

    /// Ctrl, Alt and Win: modifiers nobody holds while writing prose, so they
    /// lift a chord out of ordinary typing. Shift is deliberately NOT one —
    /// Shift+A is a capital A and Shift+Left selects a character, so a chord
    /// qualified only by Shift fires while the user is simply writing.
    pub const fn is_command_modifier(self) -> bool {
        matches!(
            self,
            K::LCtrl | K::RCtrl | K::LAlt | K::RAlt | K::LWin | K::RWin
        )
    }

    /// The locks whose toggle a low-level hook can actually stop.
    ///
    /// Num Lock is deliberately absent: Windows applies its toggle ABOVE the
    /// hook, so suppressing it would eat the keystroke and flip the lock
    /// anyway (this is why PowerToys has to restore it with SendInput).
    pub const fn is_suppressible_lock(self) -> bool {
        matches!(self, K::CapsLock | K::ScrollLock)
    }

    /// Alt and Win — the modifiers that pop a menu on release unless Windows
    /// saw another key go down while they were held. A swallowed keypress is
    /// invisible to the system, so it cannot mark them "used in a chord", and
    /// suppressing inside such a chord would pop the Start menu or the menu
    /// bar on every dictation. Ctrl and Shift have no such release behaviour.
    pub const fn is_menu_modifier(self) -> bool {
        matches!(self, K::LAlt | K::RAlt | K::LWin | K::RWin)
    }

    /// The config.toml spelling. Stable forever: config files hold these.
    pub const fn token(self) -> &'static str {
        match self {
            K::LCtrl => "LCtrl",
            K::RCtrl => "RCtrl",
            K::LShift => "LShift",
            K::RShift => "RShift",
            K::LAlt => "LAlt",
            K::RAlt => "RAlt",
            K::LWin => "LWin",
            K::RWin => "RWin",
            K::CapsLock => "CapsLock",
            K::NumLock => "NumLock",
            K::ScrollLock => "ScrollLock",
            K::Apps => "Apps",
            K::F1 => "F1",
            K::F2 => "F2",
            K::F3 => "F3",
            K::F4 => "F4",
            K::F5 => "F5",
            K::F6 => "F6",
            K::F7 => "F7",
            K::F8 => "F8",
            K::F9 => "F9",
            K::F10 => "F10",
            K::F11 => "F11",
            K::F12 => "F12",
            K::F13 => "F13",
            K::F14 => "F14",
            K::F15 => "F15",
            K::F16 => "F16",
            K::F17 => "F17",
            K::F18 => "F18",
            K::F19 => "F19",
            K::F20 => "F20",
            K::F21 => "F21",
            K::F22 => "F22",
            K::F23 => "F23",
            K::F24 => "F24",
            K::A => "A",
            K::B => "B",
            K::C => "C",
            K::D => "D",
            K::E => "E",
            K::F => "F",
            K::G => "G",
            K::H => "H",
            K::I => "I",
            K::J => "J",
            K::K => "K",
            K::L => "L",
            K::M => "M",
            K::N => "N",
            K::O => "O",
            K::P => "P",
            K::Q => "Q",
            K::R => "R",
            K::S => "S",
            K::T => "T",
            K::U => "U",
            K::V => "V",
            K::W => "W",
            K::X => "X",
            K::Y => "Y",
            K::Z => "Z",
            K::Digit0 => "0",
            K::Digit1 => "1",
            K::Digit2 => "2",
            K::Digit3 => "3",
            K::Digit4 => "4",
            K::Digit5 => "5",
            K::Digit6 => "6",
            K::Digit7 => "7",
            K::Digit8 => "8",
            K::Digit9 => "9",
            K::Left => "Left",
            K::Right => "Right",
            K::Up => "Up",
            K::Down => "Down",
            K::Insert => "Insert",
            K::Delete => "Delete",
            K::Home => "Home",
            K::End => "End",
            K::PageUp => "PageUp",
            K::PageDown => "PageDown",
            K::Tab => "Tab",
            K::Space => "Space",
            K::Enter => "Enter",
            K::Backspace => "Backspace",
            K::Numpad0 => "Numpad0",
            K::Numpad1 => "Numpad1",
            K::Numpad2 => "Numpad2",
            K::Numpad3 => "Numpad3",
            K::Numpad4 => "Numpad4",
            K::Numpad5 => "Numpad5",
            K::Numpad6 => "Numpad6",
            K::Numpad7 => "Numpad7",
            K::Numpad8 => "Numpad8",
            K::Numpad9 => "Numpad9",
            K::NumpadAdd => "NumpadAdd",
            K::NumpadSubtract => "NumpadSubtract",
            K::NumpadMultiply => "NumpadMultiply",
            K::NumpadDivide => "NumpadDivide",
            K::NumpadDecimal => "NumpadDecimal",
            K::Equals => "Equals",
            K::Comma => "Comma",
            K::Minus => "Minus",
            K::Period => "Period",
            K::Semicolon => "Semicolon",
            K::Slash => "Slash",
            K::Backtick => "Backtick",
            K::LeftBracket => "LeftBracket",
            K::Backslash => "Backslash",
            K::RightBracket => "RightBracket",
            K::Quote => "Quote",
            K::Oem8 => "Oem8",
            K::Oem102 => "Oem102",
        }
    }

    /// The name a person reads. Config keeps `token` because it round-trips
    /// through TOML; the UI shows this because it is what the key is called.
    pub const fn label(self) -> &'static str {
        match self {
            K::LCtrl => "Left Ctrl",
            K::RCtrl => "Right Ctrl",
            K::LShift => "Left Shift",
            K::RShift => "Right Shift",
            K::LAlt => "Left Alt",
            K::RAlt => "Right Alt",
            K::LWin => "Left Win",
            K::RWin => "Right Win",
            K::CapsLock => "Caps Lock",
            K::NumLock => "Num Lock",
            K::ScrollLock => "Scroll Lock",
            K::Apps => "Menu",
            K::F1 => "F1",
            K::F2 => "F2",
            K::F3 => "F3",
            K::F4 => "F4",
            K::F5 => "F5",
            K::F6 => "F6",
            K::F7 => "F7",
            K::F8 => "F8",
            K::F9 => "F9",
            K::F10 => "F10",
            K::F11 => "F11",
            K::F12 => "F12",
            K::F13 => "F13",
            K::F14 => "F14",
            K::F15 => "F15",
            K::F16 => "F16",
            K::F17 => "F17",
            K::F18 => "F18",
            K::F19 => "F19",
            K::F20 => "F20",
            K::F21 => "F21",
            K::F22 => "F22",
            K::F23 => "F23",
            K::F24 => "F24",
            K::A => "A",
            K::B => "B",
            K::C => "C",
            K::D => "D",
            K::E => "E",
            K::F => "F",
            K::G => "G",
            K::H => "H",
            K::I => "I",
            K::J => "J",
            K::K => "K",
            K::L => "L",
            K::M => "M",
            K::N => "N",
            K::O => "O",
            K::P => "P",
            K::Q => "Q",
            K::R => "R",
            K::S => "S",
            K::T => "T",
            K::U => "U",
            K::V => "V",
            K::W => "W",
            K::X => "X",
            K::Y => "Y",
            K::Z => "Z",
            K::Digit0 => "0",
            K::Digit1 => "1",
            K::Digit2 => "2",
            K::Digit3 => "3",
            K::Digit4 => "4",
            K::Digit5 => "5",
            K::Digit6 => "6",
            K::Digit7 => "7",
            K::Digit8 => "8",
            K::Digit9 => "9",
            K::Left => "Left Arrow",
            K::Right => "Right Arrow",
            K::Up => "Up Arrow",
            K::Down => "Down Arrow",
            K::Insert => "Insert",
            K::Delete => "Delete",
            K::Home => "Home",
            K::End => "End",
            K::PageUp => "Page Up",
            K::PageDown => "Page Down",
            K::Tab => "Tab",
            K::Space => "Space",
            K::Enter => "Enter",
            K::Backspace => "Backspace",
            K::Numpad0 => "Numpad 0",
            K::Numpad1 => "Numpad 1",
            K::Numpad2 => "Numpad 2",
            K::Numpad3 => "Numpad 3",
            K::Numpad4 => "Numpad 4",
            K::Numpad5 => "Numpad 5",
            K::Numpad6 => "Numpad 6",
            K::Numpad7 => "Numpad 7",
            K::Numpad8 => "Numpad 8",
            K::Numpad9 => "Numpad 9",
            K::NumpadAdd => "Numpad +",
            K::NumpadSubtract => "Numpad -",
            K::NumpadMultiply => "Numpad *",
            K::NumpadDivide => "Numpad /",
            K::NumpadDecimal => "Numpad .",
            K::Equals => "=",
            K::Comma => ",",
            K::Minus => "-",
            K::Period => ".",
            K::Semicolon => ";",
            K::Slash => "/",
            K::Backtick => "`",
            K::LeftBracket => "[",
            K::Backslash => "\\",
            K::RightBracket => "]",
            K::Quote => "'",
            K::Oem8 => "OEM 8",
            K::Oem102 => "OEM 102",
        }
    }
}

impl fmt::Display for PttKeyCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.token())
    }
}

/// Every chord-capable key. The scanner walks this each tick; declaration order
/// is [`PttKeyCode::ordinal`] order.
pub const ALL_KEYS: [PttKeyCode; PttKeyCode::COUNT] = [
    K::LCtrl,
    K::RCtrl,
    K::LShift,
    K::RShift,
    K::LAlt,
    K::RAlt,
    K::LWin,
    K::RWin,
    K::CapsLock,
    K::NumLock,
    K::ScrollLock,
    K::Apps,
    K::F1,
    K::F2,
    K::F3,
    K::F4,
    K::F5,
    K::F6,
    K::F7,
    K::F8,
    K::F9,
    K::F10,
    K::F11,
    K::F12,
    K::F13,
    K::F14,
    K::F15,
    K::F16,
    K::F17,
    K::F18,
    K::F19,
    K::F20,
    K::F21,
    K::F22,
    K::F23,
    K::F24,
    K::A,
    K::B,
    K::C,
    K::D,
    K::E,
    K::F,
    K::G,
    K::H,
    K::I,
    K::J,
    K::K,
    K::L,
    K::M,
    K::N,
    K::O,
    K::P,
    K::Q,
    K::R,
    K::S,
    K::T,
    K::U,
    K::V,
    K::W,
    K::X,
    K::Y,
    K::Z,
    K::Digit0,
    K::Digit1,
    K::Digit2,
    K::Digit3,
    K::Digit4,
    K::Digit5,
    K::Digit6,
    K::Digit7,
    K::Digit8,
    K::Digit9,
    K::Left,
    K::Right,
    K::Up,
    K::Down,
    K::Insert,
    K::Delete,
    K::Home,
    K::End,
    K::PageUp,
    K::PageDown,
    K::Tab,
    K::Space,
    K::Enter,
    K::Backspace,
    K::Numpad0,
    K::Numpad1,
    K::Numpad2,
    K::Numpad3,
    K::Numpad4,
    K::Numpad5,
    K::Numpad6,
    K::Numpad7,
    K::Numpad8,
    K::Numpad9,
    K::NumpadAdd,
    K::NumpadSubtract,
    K::NumpadMultiply,
    K::NumpadDivide,
    K::NumpadDecimal,
    K::Equals,
    K::Comma,
    K::Minus,
    K::Period,
    K::Semicolon,
    K::Slash,
    K::Backtick,
    K::LeftBracket,
    K::Backslash,
    K::RightBracket,
    K::Quote,
    K::Oem8,
    K::Oem102,
];

/// Parse one key token. Case-insensitive; accepts a few historical and obvious
/// aliases so hand-written config keeps working.
pub fn parse_key(name: &str) -> Option<PttKeyCode> {
    let lower = name.trim().to_ascii_lowercase();
    let key = match lower.as_str() {
        "lctrl" => K::LCtrl,
        "rctrl" => K::RCtrl,
        "lshift" => K::LShift,
        "rshift" => K::RShift,
        "lalt" => K::LAlt,
        "ralt" => K::RAlt,
        "lwin" => K::LWin,
        "rwin" => K::RWin,
        "capslock" => K::CapsLock,
        "numlock" => K::NumLock,
        "scrolllock" => K::ScrollLock,
        "apps" => K::Apps,
        "f1" => K::F1,
        "f2" => K::F2,
        "f3" => K::F3,
        "f4" => K::F4,
        "f5" => K::F5,
        "f6" => K::F6,
        "f7" => K::F7,
        "f8" => K::F8,
        "f9" => K::F9,
        "f10" => K::F10,
        "f11" => K::F11,
        "f12" => K::F12,
        "f13" => K::F13,
        "f14" => K::F14,
        "f15" => K::F15,
        "f16" => K::F16,
        "f17" => K::F17,
        "f18" => K::F18,
        "f19" => K::F19,
        "f20" => K::F20,
        "f21" => K::F21,
        "f22" => K::F22,
        "f23" => K::F23,
        "f24" => K::F24,
        "a" => K::A,
        "b" => K::B,
        "c" => K::C,
        "d" => K::D,
        "e" => K::E,
        "f" => K::F,
        "g" => K::G,
        "h" => K::H,
        "i" => K::I,
        "j" => K::J,
        "k" => K::K,
        "l" => K::L,
        "m" => K::M,
        "n" => K::N,
        "o" => K::O,
        "p" => K::P,
        "q" => K::Q,
        "r" => K::R,
        "s" => K::S,
        "t" => K::T,
        "u" => K::U,
        "v" => K::V,
        "w" => K::W,
        "x" => K::X,
        "y" => K::Y,
        "z" => K::Z,
        "0" => K::Digit0,
        "1" => K::Digit1,
        "2" => K::Digit2,
        "3" => K::Digit3,
        "4" => K::Digit4,
        "5" => K::Digit5,
        "6" => K::Digit6,
        "7" => K::Digit7,
        "8" => K::Digit8,
        "9" => K::Digit9,
        "left" => K::Left,
        "right" => K::Right,
        "up" => K::Up,
        "down" => K::Down,
        "insert" => K::Insert,
        "delete" => K::Delete,
        "home" => K::Home,
        "end" => K::End,
        "pageup" => K::PageUp,
        "pagedown" => K::PageDown,
        "tab" => K::Tab,
        "space" => K::Space,
        "enter" => K::Enter,
        "backspace" => K::Backspace,
        "numpad0" => K::Numpad0,
        "numpad1" => K::Numpad1,
        "numpad2" => K::Numpad2,
        "numpad3" => K::Numpad3,
        "numpad4" => K::Numpad4,
        "numpad5" => K::Numpad5,
        "numpad6" => K::Numpad6,
        "numpad7" => K::Numpad7,
        "numpad8" => K::Numpad8,
        "numpad9" => K::Numpad9,
        "numpadadd" => K::NumpadAdd,
        "numpadsubtract" => K::NumpadSubtract,
        "numpadmultiply" => K::NumpadMultiply,
        "numpaddivide" => K::NumpadDivide,
        "numpaddecimal" => K::NumpadDecimal,
        "equals" => K::Equals,
        "comma" => K::Comma,
        "minus" => K::Minus,
        "period" => K::Period,
        "semicolon" => K::Semicolon,
        "slash" => K::Slash,
        "backtick" => K::Backtick,
        "leftbracket" => K::LeftBracket,
        "backslash" => K::Backslash,
        "rightbracket" => K::RightBracket,
        "quote" => K::Quote,
        "oem8" => K::Oem8,
        "oem102" => K::Oem102,
        // Aliases: pre-existing config spellings, and the obvious synonyms.
        "lcontrol" => K::LCtrl,
        "rcontrol" => K::RCtrl,
        "altgr" => K::RAlt,
        "lcmd" | "lsuper" => K::LWin,
        "rcmd" | "rsuper" => K::RWin,
        "return" => K::Enter,
        "esc" | "escape" => return None, // Escape cancels recording; never bindable
        "pgup" => K::PageUp,
        "pgdn" | "pgdown" => K::PageDown,
        "del" => K::Delete,
        "ins" => K::Insert,
        "menu" | "contextmenu" => K::Apps,
        "grave" | "backquote" => K::Backtick,
        "apostrophe" => K::Quote,
        "plus" => K::Equals,
        _ => return None,
    };
    Some(key)
}

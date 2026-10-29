//! The six categorical hues per palette, in `Hue` order: Blue, Teal, Green,
//! Gold, Rose, Violet. Each `ink` is at least 4.5:1 on every fill its palette
//! draws, `ink` on `wash` is at least 4.5:1, and the six inks of one palette
//! sit at least 0.06 apart in OKLab — the tests in `theme/tests.rs` hold every
//! row in `palette::ALL` to that. A value may move only with those tests green.
use super::palette::rgb;
use egui::Color32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct HueInk {
    /// Text and icons.
    pub ink: Color32,
    /// Chip and badge fill behind `ink`.
    pub wash: Color32,
}

const fn pair(ink: u32, wash: u32) -> HueInk {
    HueInk {
        ink: rgb(ink),
        wash: rgb(wash),
    }
}

pub(super) const LIGHT: [HueInk; 6] = [
    pair(0x2f5f9e, 0xe3ecf8),
    pair(0x1d6b6e, 0xdcefee),
    pair(0x3d6b26, 0xe2eed8),
    pair(0x81580a, 0xf5ead2),
    pair(0xa63a62, 0xf8e3ea),
    pair(0x6b4aa8, 0xece5f6),
];

pub(super) const DARK: [HueInk; 6] = [
    pair(0x9cc2f2, 0x1f3048),
    pair(0x84d2ca, 0x173a37),
    pair(0xb0d690, 0x24391c),
    pair(0xe8c47c, 0x3d3115),
    pair(0xf2a8c2, 0x45202f),
    pair(0xc9b4f4, 0x30264a),
];

/// Solarized's own blue, cyan, green, yellow, magenta and violet, shifted in
/// lightness to clear 4.5:1 on base3/base2 — the precedent `base01` set.
pub(super) const SOLARIZED_LIGHT: [HueInk; 6] = [
    pair(0x1d68a8, 0xdde9ef),
    pair(0x15736c, 0xd8ece5),
    pair(0x5a6800, 0xe6e9c8),
    pair(0x7d5c00, 0xf1e5bf),
    pair(0xa82a68, 0xf5dde3),
    pair(0x5458a8, 0xe4e1ef),
];

/// The same six Solarized accents, lightened to clear 4.5:1 on base03/base02.
pub(super) const SOLARIZED_DARK: [HueInk; 6] = [
    pair(0x6cb4ef, 0x0c3f5c),
    pair(0x4cc4ba, 0x07474a),
    pair(0xb3c34a, 0x23472a),
    pair(0xe0b33a, 0x3a3d1c),
    pair(0xef8fb8, 0x3b2a45),
    pair(0xa9adf0, 0x22305a),
];

//! Color roles for the approved Home/Insights design. Solarized's eight base
//! colors and cyan are from https://ethanschoonover.com/solarized/.
use super::*;
use egui::style::{Selection, WidgetVisuals, Widgets};
use egui::CornerRadius;

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

pub(super) struct Palette {
    pub text: Color32,
    pub text_strong: Color32,
    pub text_weak: Color32,
    pub window: Color32,
    pub surface: Color32,
    pub sidebar: Color32,
    pub tint: Color32,
    pub hairline: Color32,
    pub hairline_strong: Color32,
    pub fill_hover: Color32,
    pub accent: Color32,
    pub on_accent: Color32,
    pub chart: Color32,
    pub chart_soft: Color32,
    pub dark: bool,
}

pub(super) const LIGHT: Palette = Palette {
    text: rgb(0x252e29),
    text_strong: rgb(0x19251e),
    // Darkened from the preview's #687269 to read on the tinted hero/sidebar.
    text_weak: rgb(0x616b63),
    window: rgb(0xf7f7f3),
    surface: Color32::WHITE,
    sidebar: rgb(0xeeefe9),
    tint: rgb(0xe8eee2),
    hairline: rgb(0xe2e5dc),
    hairline_strong: rgb(0x9ba797),
    fill_hover: rgb(0xf2f3ed),
    accent: rgb(0x376c58),
    on_accent: Color32::WHITE,
    chart: rgb(0x527e69),
    chart_soft: rgb(0xc2d2bc),
    dark: false,
};

pub(super) const DARK: Palette = Palette {
    text: rgb(0xe9eee4),
    text_strong: rgb(0xf5f7f0),
    text_weak: rgb(0xa4b1a4),
    window: rgb(0x171c19),
    surface: rgb(0x202722),
    sidebar: rgb(0x131814),
    tint: rgb(0x2d3b2e),
    hairline: rgb(0x354137),
    hairline_strong: rgb(0x5b6d5e),
    fill_hover: rgb(0x272f29),
    accent: rgb(0xb5cca1),
    on_accent: rgb(0x182518),
    chart: rgb(0xa8c99a),
    chart_soft: rgb(0x4a644c),
    dark: true,
};

const BASE03: Color32 = rgb(0x002b36);
const BASE02: Color32 = rgb(0x073642);
const BASE01: Color32 = rgb(0x586e75);
const BASE1: Color32 = rgb(0x93a1a1);
const BASE2: Color32 = rgb(0xeee8d5);
const BASE3: Color32 = rgb(0xfdf6e3);
const CYAN: Color32 = rgb(0x2aa198);

pub(super) const SOLARIZED_LIGHT: Palette = Palette {
    // base01 blended 8% toward base02: the official base01/base2 pair is
    // 4.39:1. This small adjustment keeps body and muted text above 4.5:1
    // even on the darker sidebar and hero, without changing the base fills.
    text: rgb(0x526a71),
    text_strong: BASE02,
    text_weak: rgb(0x526a71),
    window: BASE3,
    surface: BASE3,
    sidebar: BASE2,
    tint: BASE2,
    // Supporting borders are muted blends, not additional accent colors.
    hairline: rgb(0xd9d4c4),
    hairline_strong: rgb(0x9fa99f),
    fill_hover: BASE2,
    accent: rgb(0x526a71),
    on_accent: BASE3,
    chart: CYAN,
    chart_soft: rgb(0xbdd8c5),
    dark: false,
};

pub(super) const SOLARIZED_DARK: Palette = Palette {
    text: BASE2,
    text_strong: BASE3,
    text_weak: BASE1,
    window: BASE03,
    surface: BASE02,
    sidebar: BASE02,
    tint: BASE03,
    hairline: rgb(0x27515b),
    hairline_strong: BASE01,
    fill_hover: rgb(0x123f49),
    accent: BASE1,
    on_accent: BASE03,
    chart: CYAN,
    chart_soft: rgb(0x185852),
    dark: true,
};

/// `Visuals` has no custom palette slot. The four deliberately distinct
/// canvas colors identify our palette; unknown egui visuals fall back to
/// the corresponding neutral theme. Per-widget color changes are harmless.
pub(super) fn palette(v: &Visuals) -> &'static Palette {
    if v.panel_fill == SOLARIZED_LIGHT.window {
        &SOLARIZED_LIGHT
    } else if v.panel_fill == SOLARIZED_DARK.window {
        &SOLARIZED_DARK
    } else if v.dark_mode {
        &DARK
    } else {
        &LIGHT
    }
}

pub(super) fn visuals(p: &Palette) -> Visuals {
    let base = if p.dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    let hairline = Stroke::new(1.0, p.hairline);
    let strong = Stroke::new(1.0, p.hairline_strong);
    let widget = |bg: Color32, fg: Color32, bg_stroke: Stroke| WidgetVisuals {
        bg_fill: bg,
        weak_bg_fill: bg,
        bg_stroke,
        fg_stroke: Stroke::new(1.0, fg),
        corner_radius: CornerRadius::same(CONTROL_RADIUS),
        expansion: 0.0,
    };
    Visuals {
        weak_text_color: Some(p.text_weak),
        widgets: Widgets {
            noninteractive: widget(p.window, p.text, hairline),
            inactive: widget(p.surface, p.text, hairline),
            hovered: widget(p.fill_hover, p.text_strong, strong),
            active: widget(p.tint, p.text_strong, Stroke::new(2.0, p.accent)),
            open: widget(p.fill_hover, p.text, hairline),
        },
        selection: Selection {
            bg_fill: p.accent.gamma_multiply(0.30),
            stroke: Stroke::new(2.0, p.accent),
        },
        hyperlink_color: p.accent,
        faint_bg_color: p.surface,
        extreme_bg_color: p.window,
        warn_fg_color: warning(&base),
        error_fg_color: danger(&base),
        window_corner_radius: CornerRadius::same(DIALOG_RADIUS),
        window_shadow: Shadow {
            offset: [0, 12],
            blur: 32,
            spread: 0,
            color: Color32::from_black_alpha(if p.dark { 110 } else { 24 }),
        },
        window_fill: p.window,
        window_stroke: strong,
        menu_corner_radius: CornerRadius::same(CONTROL_RADIUS),
        panel_fill: p.window,
        popup_shadow: Shadow {
            offset: [0, 6],
            blur: 18,
            spread: 0,
            color: Color32::from_black_alpha(if p.dark { 110 } else { 24 }),
        },
        ..base
    }
}

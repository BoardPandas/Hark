//! Hark's visual identity. Every design token lives here (Phase 4 spec
//! §3.10): embedded fonts, the type scale, both `Visuals`, spacing, and the
//! icon glyphs. `apply` runs once at startup; no panel sets ad-hoc colors,
//! sizes, or spacing inline.

use egui::epaint::Shadow;
use egui::style::{Selection, WidgetVisuals, Widgets};
use egui::{
    Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Stroke,
    TextStyle, Theme, Vec2, Visuals,
};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Phosphor glyphs, vendored from the egui-phosphor 0.12.0 crate package
/// (regular variant) because that crate still pins egui 0.34. Codepoints
/// were extracted from the same package's generated constants, so they
/// cannot drift from the embedded `Phosphor.ttf`.
pub mod icons {
    // The curated set is the CP2 icon decision (spec §3.10); the entries
    // not yet referenced belong to the CP3-CP5 panels (copy, trash, search,
    // spinner, ...). Keep the set whole rather than churning constants in
    // and out per checkpoint.
    #![allow(dead_code)]

    pub const BOOK_OPEN: &str = "\u{E0E6}";
    pub const CHART_BAR: &str = "\u{E150}";
    pub const CHECK: &str = "\u{E182}";
    pub const CIRCLE_NOTCH: &str = "\u{EB44}";
    pub const CLOCK: &str = "\u{E19A}";
    pub const CLOCK_COUNTER_CLOCKWISE: &str = "\u{E1A0}";
    pub const COPY: &str = "\u{E1CA}";
    pub const GEAR: &str = "\u{E270}";
    pub const KEY: &str = "\u{E2D6}";
    pub const LIGHTNING: &str = "\u{E2DE}";
    pub const MAGNIFYING_GLASS: &str = "\u{E30C}";
    pub const MICROPHONE: &str = "\u{E326}";
    pub const PLAY: &str = "\u{E3D0}";
    pub const SPINNER: &str = "\u{E66A}";
    pub const TRASH: &str = "\u{E4A6}";
    pub const WARNING: &str = "\u{E4E0}";
    pub const WAVEFORM: &str = "\u{E802}";
    pub const X: &str = "\u{E4F6}";
}

// --- palette (spec §3.10 starting values; contrast-tuned on real screens) ---

const DARK_WINDOW: Color32 = Color32::from_rgb(0x11, 0x13, 0x17);
const DARK_PANEL: Color32 = Color32::from_rgb(0x16, 0x18, 0x1D);
const DARK_HAIRLINE: Color32 = Color32::from_rgb(0x26, 0x28, 0x2F);
const DARK_TEXT: Color32 = Color32::from_rgb(0xE6, 0xE7, 0xEC);
const DARK_TEXT_STRONG: Color32 = Color32::from_rgb(0xF5, 0xF6, 0xFA);
const DARK_TEXT_WEAK: Color32 = Color32::from_rgb(0x9E, 0xA0, 0xAA);
/// Interactive accent: links, focus, selection (dark).
const DARK_ACCENT: Color32 = Color32::from_rgb(0x7C, 0x7F, 0xF2);
/// Accent as a fill behind [`ON_ACCENT`] text (nav pill, primary button):
/// deeper than [`DARK_ACCENT`] so white text keeps >= 4.5:1.
const DARK_ACCENT_FILL: Color32 = Color32::from_rgb(0x4F, 0x52, 0xC8);

const LIGHT_WINDOW: Color32 = Color32::from_rgb(0xFA, 0xFA, 0xFC);
const LIGHT_PANEL: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
const LIGHT_HAIRLINE: Color32 = Color32::from_rgb(0xE4, 0xE4, 0xEA);
const LIGHT_TEXT: Color32 = Color32::from_rgb(0x1C, 0x1E, 0x24);
const LIGHT_TEXT_STRONG: Color32 = Color32::from_rgb(0x0C, 0x0D, 0x10);
const LIGHT_TEXT_WEAK: Color32 = Color32::from_rgb(0x63, 0x65, 0x70);
const LIGHT_ACCENT: Color32 = Color32::from_rgb(0x5B, 0x5B, 0xD6);
const LIGHT_ACCENT_FILL: Color32 = LIGHT_ACCENT;

/// Text/icon color on top of an accent fill.
pub const ON_ACCENT: Color32 = Color32::WHITE;
/// Semantic colors, shared by both themes; always paired with an icon or
/// label (guardrails §3), never the sole carrier of a state.
pub const DANGER: Color32 = Color32::from_rgb(0xE5, 0x48, 0x4D);
/// Fill behind [`ON_ACCENT`] text on destructive buttons: deeper than
/// [`DANGER`] (an icon/stroke color) so white text keeps >= 4.5:1.
pub const DANGER_FILL: Color32 = Color32::from_rgb(0xC6, 0x2A, 0x30);
pub const SUCCESS: Color32 = Color32::from_rgb(0x30, 0xA4, 0x6C);
pub const WARNING: Color32 = Color32::from_rgb(0xF5, 0xA5, 0x24);

/// Tray icon fills (CP5): drawn into RGBA bitmaps, not painted by egui, so
/// they cannot follow the theme. Mid-tones legible on both light and dark
/// taskbars; the dark accent doubles as the tray accent.
pub const TRAY_ACCENT: Color32 = DARK_ACCENT;
pub const TRAY_STOPPED: Color32 = Color32::from_rgb(0x8A, 0x8F, 0x98);

/// Recording overlay tokens (the Phase 5 "floating recording pill"): a
/// floating always-on-top viewport shown while the push-to-talk chord is
/// held. It is always dark and reads over arbitrary desktop content, so its
/// palette is fixed rather than theme-paired. The accent is the tray/brand
/// purple.
pub const OVERLAY_ACCENT: Color32 = DARK_ACCENT;
/// The dark "pill" capsule behind the pulsing circle. Translucent so it
/// blends over the desktop through the transparent overlay window.
pub const OVERLAY_PILL_FILL: Color32 = Color32::from_rgba_premultiplied(0x0D, 0x0E, 0x12, 0xE6);
/// A hairline rim on the pill so it stays legible on same-tone backgrounds.
pub const OVERLAY_PILL_STROKE: Color32 = Color32::from_rgba_premultiplied(0x2E, 0x30, 0x3B, 0x80);

/// The one non-built-in text style (16 px Inter Medium).
pub fn subheading() -> TextStyle {
    TextStyle::Name("Subheading".into())
}

fn medium() -> FontFamily {
    FontFamily::Name("InterMedium".into())
}

fn semibold() -> FontFamily {
    FontFamily::Name("InterSemiBold".into())
}

pub fn accent(visuals: &Visuals) -> Color32 {
    if visuals.dark_mode {
        DARK_ACCENT
    } else {
        LIGHT_ACCENT
    }
}

/// Fill for the selected nav pill and the (at most one) primary button.
pub fn accent_fill(visuals: &Visuals) -> Color32 {
    if visuals.dark_mode {
        DARK_ACCENT_FILL
    } else {
        LIGHT_ACCENT_FILL
    }
}

/// Install fonts, type scale, spacing, and both theme palettes. Called once
/// at startup; egui follows the OS theme afterwards (`ThemePreference::
/// System`; a Light/Dark/System radio arrives with the settings form).
pub fn apply(ctx: &Context) {
    ctx.set_fonts(font_definitions());
    ctx.all_styles_mut(|style| {
        style.text_styles = text_styles();
        spacing(&mut style.spacing);
    });
    ctx.set_visuals_of(Theme::Dark, dark_visuals());
    ctx.set_visuals_of(Theme::Light, light_visuals());
    // Follow the OS by default, but never clobber a preference the Settings
    // radio persisted into egui memory (restored before app construction):
    // re-apply whatever is current instead of forcing System.
    let preference = ctx.options(|o| o.theme_preference);
    ctx.set_theme(preference);
}

/// Inter Regular/Medium/SemiBold each as their own family (egui cannot
/// interpolate variable-font weights, emilk/egui#1862), JetBrains Mono for
/// transcripts and latency figures, Phosphor for icons. egui's default
/// fonts stay appended as emoji/coverage fallback.
fn font_definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    for (name, bytes) in [
        ("Inter", &include_bytes!("../assets/Inter-Regular.ttf")[..]),
        (
            "InterMedium",
            &include_bytes!("../assets/Inter-Medium.ttf")[..],
        ),
        (
            "InterSemiBold",
            &include_bytes!("../assets/Inter-SemiBold.ttf")[..],
        ),
        (
            "JetBrainsMono",
            &include_bytes!("../assets/JetBrainsMono-Regular.ttf")[..],
        ),
        ("Phosphor", &include_bytes!("../assets/Phosphor.ttf")[..]),
    ] {
        fonts
            .font_data
            .insert(name.to_string(), Arc::new(FontData::from_static(bytes)));
    }

    let fallback = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let with_fallback = |primary: &str| {
        let mut list = vec![primary.to_string(), "Phosphor".to_string()];
        list.extend(fallback.iter().cloned());
        list
    };

    fonts
        .families
        .insert(FontFamily::Proportional, with_fallback("Inter"));
    fonts
        .families
        .insert(medium(), with_fallback("InterMedium"));
    fonts
        .families
        .insert(semibold(), with_fallback("InterSemiBold"));
    let mut mono = with_fallback("JetBrainsMono");
    // Keep egui's default monospace fonts reachable after ours.
    if let Some(default_mono) = fonts.families.get(&FontFamily::Monospace) {
        mono.extend(default_mono.iter().cloned());
    }
    fonts.families.insert(FontFamily::Monospace, mono);
    fonts
}

/// The type scale (spec §3.10). Secondary text uses `weak_text_color`,
/// never an ad-hoc smaller size.
fn text_styles() -> BTreeMap<TextStyle, FontId> {
    BTreeMap::from([
        (TextStyle::Heading, FontId::new(22.0, semibold())),
        (subheading(), FontId::new(16.0, medium())),
        (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(14.0, medium())),
        (
            TextStyle::Small,
            FontId::new(11.5, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(13.0, FontFamily::Monospace),
        ),
    ])
}

/// 4 px base grid; comfortable uniform targets.
fn spacing(spacing: &mut egui::style::Spacing) {
    spacing.item_spacing = Vec2::new(8.0, 10.0);
    spacing.button_padding = Vec2::new(14.0, 7.0);
    spacing.window_margin = Margin::same(16);
    spacing.menu_margin = Margin::same(16);
    spacing.indent = 18.0;
    spacing.interact_size.y = 30.0;
}

struct Palette {
    text: Color32,
    text_strong: Color32,
    text_weak: Color32,
    window: Color32,
    panel: Color32,
    hairline: Color32,
    hairline_strong: Color32,
    /// Widget resting / hovered / pressed fills (quiet buttons).
    fill_rest: Color32,
    fill_hover: Color32,
    fill_press: Color32,
    accent: Color32,
    shadow_alpha: u8,
}

fn build_visuals(base: Visuals, p: &Palette) -> Visuals {
    let hairline = Stroke::new(1.0, p.hairline);
    let widget = |bg: Color32, fg: Color32, bg_stroke: Stroke| WidgetVisuals {
        bg_fill: bg,
        weak_bg_fill: bg,
        bg_stroke,
        fg_stroke: Stroke::new(1.0, fg),
        corner_radius: CornerRadius::same(6),
        expansion: 0.0,
    };
    Visuals {
        weak_text_color: Some(p.text_weak),
        widgets: Widgets {
            noninteractive: widget(p.panel, p.text, hairline),
            inactive: widget(p.fill_rest, p.text, hairline),
            hovered: widget(
                p.fill_hover,
                p.text_strong,
                Stroke::new(1.0, p.hairline_strong),
            ),
            active: widget(
                p.fill_press,
                p.text_strong,
                Stroke::new(1.0, p.hairline_strong),
            ),
            open: widget(p.fill_rest, p.text, hairline),
        },
        selection: Selection {
            bg_fill: p.accent.gamma_multiply(0.35),
            // Doubles as the visible focus ring (2 px accent, guardrails §3).
            stroke: Stroke::new(2.0, p.accent),
        },
        hyperlink_color: p.accent,
        faint_bg_color: p.fill_rest,
        extreme_bg_color: p.window,
        warn_fg_color: WARNING,
        error_fg_color: DANGER,
        window_corner_radius: CornerRadius::same(10),
        window_shadow: Shadow {
            offset: [0, 6],
            blur: 20,
            spread: 0,
            color: Color32::from_black_alpha(p.shadow_alpha),
        },
        window_fill: p.window,
        window_stroke: hairline,
        menu_corner_radius: CornerRadius::same(8),
        panel_fill: p.panel,
        popup_shadow: Shadow {
            offset: [0, 4],
            blur: 12,
            spread: 0,
            color: Color32::from_black_alpha(p.shadow_alpha),
        },
        ..base
    }
}

fn dark_visuals() -> Visuals {
    build_visuals(
        Visuals::dark(),
        &Palette {
            text: DARK_TEXT,
            text_strong: DARK_TEXT_STRONG,
            text_weak: DARK_TEXT_WEAK,
            window: DARK_WINDOW,
            panel: DARK_PANEL,
            hairline: DARK_HAIRLINE,
            hairline_strong: Color32::from_rgb(0x33, 0x36, 0x3F),
            fill_rest: Color32::from_rgb(0x1C, 0x1E, 0x24),
            fill_hover: Color32::from_rgb(0x23, 0x26, 0x30),
            fill_press: Color32::from_rgb(0x2A, 0x2D, 0x38),
            accent: DARK_ACCENT,
            shadow_alpha: 96,
        },
    )
}

fn light_visuals() -> Visuals {
    build_visuals(
        Visuals::light(),
        &Palette {
            text: LIGHT_TEXT,
            text_strong: LIGHT_TEXT_STRONG,
            text_weak: LIGHT_TEXT_WEAK,
            window: LIGHT_WINDOW,
            panel: LIGHT_PANEL,
            hairline: LIGHT_HAIRLINE,
            hairline_strong: Color32::from_rgb(0xD4, 0xD4, 0xDE),
            fill_rest: Color32::from_rgb(0xF2, 0xF2, 0xF6),
            fill_hover: Color32::from_rgb(0xEA, 0xEA, 0xF0),
            fill_press: Color32::from_rgb(0xE2, 0xE2, 0xEA),
            accent: LIGHT_ACCENT,
            shadow_alpha: 28,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG 2.x relative luminance of an sRGB color.
    fn luminance(c: Color32) -> f64 {
        let channel = |v: u8| {
            let v = v as f64 / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
    }

    fn contrast(a: Color32, b: Color32) -> f64 {
        let (la, lb) = (luminance(a), luminance(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    #[test]
    fn body_and_weak_text_meet_wcag_aa_in_both_themes() {
        for (label, text, weak, window, panel) in [
            ("dark", DARK_TEXT, DARK_TEXT_WEAK, DARK_WINDOW, DARK_PANEL),
            (
                "light",
                LIGHT_TEXT,
                LIGHT_TEXT_WEAK,
                LIGHT_WINDOW,
                LIGHT_PANEL,
            ),
        ] {
            for (surface_label, surface) in [("window", window), ("panel", panel)] {
                let body = contrast(text, surface);
                let weak_ratio = contrast(weak, surface);
                assert!(body >= 4.5, "{label} body on {surface_label}: {body:.2}");
                assert!(
                    weak_ratio >= 4.5,
                    "{label} weak text on {surface_label}: {weak_ratio:.2}"
                );
            }
        }
    }

    #[test]
    fn accent_surfaces_meet_contrast_requirements() {
        // Text on the accent fill is body-size: 4.5:1. The bare accent is a
        // non-text indicator (focus ring, links get underline affordance):
        // 3:1 against both surfaces.
        for (label, fill, accent, window) in [
            ("dark", DARK_ACCENT_FILL, DARK_ACCENT, DARK_WINDOW),
            ("light", LIGHT_ACCENT_FILL, LIGHT_ACCENT, LIGHT_WINDOW),
        ] {
            let on_fill = contrast(ON_ACCENT, fill);
            let ring = contrast(accent, window);
            assert!(
                on_fill >= 4.5,
                "{label} ON_ACCENT on accent fill: {on_fill:.2}"
            );
            assert!(ring >= 3.0, "{label} accent on window: {ring:.2}");
        }
    }

    #[test]
    fn danger_fill_carries_readable_text() {
        // The destructive confirm button paints ON_ACCENT text on
        // DANGER_FILL; body-size text needs 4.5:1.
        let ratio = contrast(ON_ACCENT, DANGER_FILL);
        assert!(ratio >= 4.5, "ON_ACCENT on DANGER_FILL: {ratio:.2}");
    }

    #[test]
    fn apply_preserves_a_restored_theme_preference() {
        // eframe restores egui memory (with a persisted Light/Dark/System
        // choice) before the app constructs; apply() must not reset it.
        let ctx = Context::default();
        ctx.set_theme(egui::ThemePreference::Dark);
        apply(&ctx);
        assert_eq!(
            ctx.options(|o| o.theme_preference),
            egui::ThemePreference::Dark
        );
    }

    #[test]
    fn type_scale_matches_the_spec() {
        let styles = text_styles();
        assert_eq!(styles.len(), 6);
        assert_eq!(styles[&TextStyle::Heading].size, 22.0);
        assert_eq!(styles[&TextStyle::Heading].family, semibold());
        assert_eq!(styles[&subheading()].size, 16.0);
        assert_eq!(styles[&subheading()].family, medium());
        assert_eq!(styles[&TextStyle::Body].size, 14.0);
        assert_eq!(styles[&TextStyle::Button].family, medium());
        assert_eq!(styles[&TextStyle::Small].size, 11.5);
        assert_eq!(styles[&TextStyle::Monospace].size, 13.0);
        assert_eq!(styles[&TextStyle::Monospace].family, FontFamily::Monospace);
    }

    #[test]
    fn every_family_resolves_and_leads_with_the_intended_font() {
        let fonts = font_definitions();
        for name in [
            "Inter",
            "InterMedium",
            "InterSemiBold",
            "JetBrainsMono",
            "Phosphor",
        ] {
            assert!(fonts.font_data.contains_key(name), "missing font {name}");
        }
        let leads = |family: &FontFamily, expected: &str| {
            let list = &fonts.families[family];
            assert_eq!(list[0], expected, "family {family:?}");
            assert_eq!(list[1], "Phosphor", "icons must fall back in {family:?}");
        };
        leads(&FontFamily::Proportional, "Inter");
        leads(&medium(), "InterMedium");
        leads(&semibold(), "InterSemiBold");
        leads(&FontFamily::Monospace, "JetBrainsMono");
    }

    #[test]
    fn both_visuals_pin_the_spec_hexes() {
        let dark = dark_visuals();
        assert!(dark.dark_mode);
        assert_eq!(dark.window_fill, DARK_WINDOW);
        assert_eq!(dark.panel_fill, DARK_PANEL);
        assert_eq!(dark.window_stroke.color, DARK_HAIRLINE);
        assert_eq!(dark.hyperlink_color, DARK_ACCENT);

        let light = light_visuals();
        assert!(!light.dark_mode);
        assert_eq!(light.window_fill, LIGHT_WINDOW);
        assert_eq!(light.panel_fill, LIGHT_PANEL);
        assert_eq!(light.selection.stroke.width, 2.0);
    }
}

//! Hark's visual identity. Every design token lives here (Phase 4 spec
//! §3.10): embedded fonts, the type scale, both `Visuals`, spacing, and the
//! icon glyphs. `apply` runs once at startup; no panel sets ad-hoc colors,
//! sizes, or spacing inline.

use egui::epaint::{Mesh, Shadow, Vertex, WHITE_UV};
use egui::style::{Selection, WidgetVisuals, Widgets};
use egui::{
    Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Pos2,
    Rect, RichText, Sense, Stroke, TextStyle, Theme, Ui, Vec2, Visuals,
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

    pub const ARROW_UP: &str = "\u{E048}";
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

// --- palette (Nocturne design language; values from nocturne-styles.css) ---
//
// The ground is a near-neutral blue-grey; surfaces (cards, inputs, expanded
// panels, dialogs) sit one step lighter. Hierarchy is size and space, not
// weight; the single blurple accent appears as a line/glow, never a flood —
// buttons are outlined, not filled (see the button helpers below).

/// Ground `--color-bg`: top bar, footer, content — the whole window base.
const DARK_WINDOW: Color32 = Color32::from_rgb(0x16, 0x18, 0x26);
/// Content region; the same ground — panels are separated by hairline rules,
/// not a tonal step.
const DARK_PANEL: Color32 = DARK_WINDOW;
/// `--color-surface`: cards, inputs, expanded detail panels, dialogs.
const DARK_SURFACE: Color32 = Color32::from_rgb(0x23, 0x25, 0x32);
/// Box outlines / control borders (neutral-800, the shadow-sm edge).
const DARK_HAIRLINE: Color32 = Color32::from_rgb(0x3F, 0x42, 0x4D);
/// A brighter hairline on hover/press (neutral-700, the shadow-md edge).
const DARK_HAIRLINE_STRONG: Color32 = Color32::from_rgb(0x59, 0x5D, 0x6C);
/// `--color-text`.
const DARK_TEXT: Color32 = Color32::from_rgb(0xE9, 0xE9, 0xED);
/// neutral-100.
const DARK_TEXT_STRONG: Color32 = Color32::from_rgb(0xF3, 0xF5, 0xFE);
/// Muted text (neutral-500) — the design's ~50% text step, solid so it clears
/// WCAG AA on both ground and surface.
const DARK_TEXT_WEAK: Color32 = Color32::from_rgb(0x93, 0x97, 0xAB);
/// Interactive accent `--color-accent`: links, focus, selection, outlined
/// primary buttons, active nav.
const DARK_ACCENT: Color32 = Color32::from_rgb(0x91, 0x84, 0xD9);
/// A solid accent fill for the rare non-text accent surface (progress fill):
/// accent-700, deep enough that white text keeps >= 4.5:1.
const DARK_ACCENT_FILL: Color32 = Color32::from_rgb(0x5D, 0x52, 0x94);
/// Subtle neutral tint used as the resting hover fill (≈ the surface step).
const DARK_FILL_HOVER: Color32 = Color32::from_rgb(0x23, 0x25, 0x32);
const DARK_FILL_PRESS: Color32 = Color32::from_rgb(0x2B, 0x2E, 0x3C);

// Light theme derives from the same OKLCH ramps (neutral / accent).
const LIGHT_WINDOW: Color32 = Color32::from_rgb(0xF3, 0xF5, 0xFE); // neutral-100
const LIGHT_PANEL: Color32 = LIGHT_WINDOW;
const LIGHT_SURFACE: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
const LIGHT_HAIRLINE: Color32 = Color32::from_rgb(0xCF, 0xD3, 0xE5); // neutral-300
const LIGHT_HAIRLINE_STRONG: Color32 = Color32::from_rgb(0xB2, 0xB6, 0xCA); // neutral-400
const LIGHT_TEXT: Color32 = Color32::from_rgb(0x29, 0x2B, 0x31); // neutral-900
const LIGHT_TEXT_STRONG: Color32 = Color32::from_rgb(0x16, 0x18, 0x26);
const LIGHT_TEXT_WEAK: Color32 = Color32::from_rgb(0x59, 0x5D, 0x6C); // neutral-700
const LIGHT_ACCENT: Color32 = Color32::from_rgb(0x5D, 0x52, 0x94); // accent-700
const LIGHT_ACCENT_FILL: Color32 = LIGHT_ACCENT;
const LIGHT_FILL_HOVER: Color32 = Color32::from_rgb(0xE4, 0xE7, 0xF5); // neutral-200
const LIGHT_FILL_PRESS: Color32 = Color32::from_rgb(0xCF, 0xD3, 0xE5); // neutral-300

/// Accent ramp stops used by name (the update banner, tags).
pub const ACCENT_200: Color32 = Color32::from_rgb(0xE7, 0xE5, 0xFE);
pub const ACCENT_800: Color32 = Color32::from_rgb(0x42, 0x3A, 0x6A);
pub const ACCENT_900: Color32 = Color32::from_rgb(0x2B, 0x27, 0x41);

/// Semantic colors (harmonized in OKLCH), shared by both themes; always
/// paired with an icon or label (guardrails §3), never the sole carrier of a
/// state. danger oklch(0.68 0.15 15), success oklch(0.72 0.12 155),
/// warning oklch(0.78 0.13 75).
pub const DANGER: Color32 = Color32::from_rgb(0xE7, 0x6A, 0x78);
pub const SUCCESS: Color32 = Color32::from_rgb(0x45, 0xB4, 0x87);
pub const WARNING: Color32 = Color32::from_rgb(0xD9, 0xA0, 0x40);

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
/// The dark "pill" capsule behind the pulsing circle: near-black translucent
/// (#101120 @ 92%) so it blends over the desktop through the transparent
/// overlay window. Stored premultiplied.
pub const OVERLAY_PILL_FILL: Color32 = Color32::from_rgba_premultiplied(0x0F, 0x10, 0x1D, 0xEB);
/// A hairline rim on the pill so it stays legible on same-tone backgrounds.
pub const OVERLAY_PILL_STROKE: Color32 = Color32::from_rgba_premultiplied(0x2E, 0x30, 0x3B, 0x80);
/// The pill's "Listening…" label sits in neutral-200.
pub const OVERLAY_TEXT: Color32 = Color32::from_rgb(0xE4, 0xE7, 0xF5);

/// The section-head text style (15 px Inter Medium — Nocturne heads are
/// medium, never bolder; hierarchy is size and space).
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

/// A solid accent fill (progress bars, meter "good" band): the rare place a
/// filled accent surface is wanted. Buttons never use it — they are outlined.
pub fn accent_fill(visuals: &Visuals) -> Color32 {
    if visuals.dark_mode {
        DARK_ACCENT_FILL
    } else {
        LIGHT_ACCENT_FILL
    }
}

/// `--color-surface`: cards, inputs, expanded detail panels, dialogs — one
/// step lighter than the ground.
pub fn surface(visuals: &Visuals) -> Color32 {
    if visuals.dark_mode {
        DARK_SURFACE
    } else {
        LIGHT_SURFACE
    }
}

/// The translucent divider used by the fading rules (text at 16% alpha).
pub fn divider(visuals: &Visuals) -> Color32 {
    let base = visuals.text_color();
    Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), 41)
}

/// A Nocturne signature: a 1px separator that fades to transparent over its
/// last 48px at each end (or a third of its width, whichever is smaller).
/// Used under every list row (history, dictionary, invocations). Allocates a
/// full-width, `gap`-tall strip and paints the rule centered in it.
pub fn fading_rule(ui: &mut Ui, gap: f32) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, gap.max(1.0)), Sense::hover());
    paint_fading_rule(ui, rect);
}

/// Paint the fading rule centered in `rect` (the row strip), without
/// allocating — for callers that already own the strip's geometry.
pub fn paint_fading_rule(ui: &Ui, rect: Rect) {
    let color = divider(ui.visuals());
    let fade = 48.0_f32.min(rect.width() / 3.0);
    let y = rect.center().y.round();
    let (l, r) = (rect.left(), rect.right());
    let clear = Color32::TRANSPARENT;
    let stops = [(l, clear), (l + fade, color), (r - fade, color), (r, clear)];
    let mut mesh = Mesh::default();
    let vert = |x: f32, y: f32, c: Color32| Vertex {
        pos: Pos2::new(x, y),
        uv: WHITE_UV,
        color: c,
    };
    for pair in stops.windows(2) {
        let (x0, c0) = pair[0];
        let (x1, c1) = pair[1];
        let base = mesh.vertices.len() as u32;
        mesh.vertices.push(vert(x0, y - 0.5, c0));
        mesh.vertices.push(vert(x1, y - 0.5, c1));
        mesh.vertices.push(vert(x1, y + 0.5, c1));
        mesh.vertices.push(vert(x0, y + 0.5, c0));
        mesh.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    ui.painter().add(mesh);
}

/// An outlined "primary" button: accent border + accent text, transparent
/// fill (the Nocturne rule — buttons are outlined, never flooded). Hover adds
/// the quiet neutral tint from the widget visuals; focus is the accent ring.
pub fn primary_button(visuals: &Visuals, text: impl Into<String>) -> egui::Button<'static> {
    let accent = accent(visuals);
    egui::Button::new(RichText::new(text.into()).color(accent)).stroke(Stroke::new(1.0, accent))
}

/// An outlined destructive button: danger text, danger-at-~50% border, no
/// fill. Used for confirm actions and "Reset stats" / "Delete".
pub fn danger_button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.into()).color(DANGER))
        .stroke(Stroke::new(1.0, DANGER.gamma_multiply(0.5)))
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

/// The Nocturne type scale. Body 15px Inter Regular; page titles 24px and
/// section heads 15px both in Inter Medium (never bolder). Secondary text
/// uses `weak_text_color`, never an ad-hoc smaller size. Mono (transcripts,
/// latency, terms, triggers) is JetBrains Mono 13px.
fn text_styles() -> BTreeMap<TextStyle, FontId> {
    BTreeMap::from([
        (TextStyle::Heading, FontId::new(24.0, medium())),
        (subheading(), FontId::new(15.0, medium())),
        (TextStyle::Body, FontId::new(15.0, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(14.0, medium())),
        (
            TextStyle::Small,
            FontId::new(12.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(13.0, FontFamily::Monospace),
        ),
    ])
}

/// Compact 0.7× density (Nocturne spacing scale). Buttons are outlined, so a
/// snug button padding keeps the border tight to the label.
fn spacing(spacing: &mut egui::style::Spacing) {
    spacing.item_spacing = Vec2::new(8.0, 8.0);
    spacing.button_padding = Vec2::new(12.0, 6.0);
    spacing.window_margin = Margin::same(14);
    spacing.menu_margin = Margin::same(14);
    spacing.indent = 18.0;
    spacing.interact_size.y = 28.0;
}

struct Palette {
    text: Color32,
    text_strong: Color32,
    text_weak: Color32,
    window: Color32,
    panel: Color32,
    surface: Color32,
    hairline: Color32,
    hairline_strong: Color32,
    /// Hovered / pressed fills (the quiet neutral tint; resting is
    /// transparent so buttons read as outlines on the ground).
    fill_hover: Color32,
    fill_press: Color32,
    accent: Color32,
    shadow_alpha: u8,
}

fn build_visuals(base: Visuals, p: &Palette) -> Visuals {
    let hairline = Stroke::new(1.0, p.hairline);
    let hairline_strong = Stroke::new(1.0, p.hairline_strong);
    let widget = |bg: Color32, fg: Color32, bg_stroke: Stroke| WidgetVisuals {
        bg_fill: bg,
        weak_bg_fill: bg,
        bg_stroke,
        fg_stroke: Stroke::new(1.0, fg),
        corner_radius: CornerRadius::same(8),
        expansion: 0.0,
    };
    Visuals {
        weak_text_color: Some(p.text_weak),
        widgets: Widgets {
            noninteractive: widget(p.panel, p.text, hairline),
            // Resting buttons are outline-only (transparent fill, hairline
            // border) — the Nocturne look.
            inactive: widget(Color32::TRANSPARENT, p.text, hairline),
            hovered: widget(p.fill_hover, p.text_strong, hairline_strong),
            active: widget(p.fill_press, p.text_strong, hairline_strong),
            open: widget(p.fill_hover, p.text, hairline),
        },
        selection: Selection {
            bg_fill: p.accent.gamma_multiply(0.30),
            // Doubles as the visible focus ring (2 px accent, guardrails §3).
            stroke: Stroke::new(2.0, p.accent),
        },
        hyperlink_color: p.accent,
        // Cards / group panels / table stripes pick up the surface step.
        faint_bg_color: p.surface,
        // Text inputs sit on the surface fill.
        extreme_bg_color: p.surface,
        warn_fg_color: WARNING,
        error_fg_color: DANGER,
        window_corner_radius: CornerRadius::same(14),
        window_shadow: Shadow {
            offset: [0, 16],
            blur: 40,
            spread: 0,
            color: Color32::from_black_alpha(p.shadow_alpha),
        },
        window_fill: p.window,
        window_stroke: hairline_strong,
        menu_corner_radius: CornerRadius::same(8),
        panel_fill: p.panel,
        popup_shadow: Shadow {
            offset: [0, 6],
            blur: 18,
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
            surface: DARK_SURFACE,
            hairline: DARK_HAIRLINE,
            hairline_strong: DARK_HAIRLINE_STRONG,
            fill_hover: DARK_FILL_HOVER,
            fill_press: DARK_FILL_PRESS,
            accent: DARK_ACCENT,
            shadow_alpha: 166,
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
            surface: LIGHT_SURFACE,
            hairline: LIGHT_HAIRLINE,
            hairline_strong: LIGHT_HAIRLINE_STRONG,
            fill_hover: LIGHT_FILL_HOVER,
            fill_press: LIGHT_FILL_PRESS,
            accent: LIGHT_ACCENT,
            shadow_alpha: 40,
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
        // The accent is a non-text indicator (focus ring, outlined-button
        // border, active-tab underline; links carry an underline affordance):
        // it must clear 3:1 against the ground so the line is always visible.
        // White on the solid accent_fill (used behind progress fills) keeps a
        // body-text margin in case a label ever lands there.
        for (label, fill, accent, window) in [
            ("dark", DARK_ACCENT_FILL, DARK_ACCENT, DARK_WINDOW),
            ("light", LIGHT_ACCENT_FILL, LIGHT_ACCENT, LIGHT_WINDOW),
        ] {
            let on_fill = contrast(Color32::WHITE, fill);
            let ring = contrast(accent, window);
            assert!(on_fill >= 4.5, "{label} white on accent fill: {on_fill:.2}");
            assert!(ring >= 3.0, "{label} accent on window: {ring:.2}");
        }
    }

    #[test]
    fn danger_reads_on_surface() {
        // The destructive confirm/reset buttons are outlined — danger text on
        // the surface fill. Keep that label legible (body size, 4.5:1).
        let ratio = contrast(DANGER, DARK_SURFACE);
        assert!(ratio >= 4.5, "DANGER on surface: {ratio:.2}");
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
        assert_eq!(styles[&TextStyle::Heading].size, 24.0);
        assert_eq!(styles[&TextStyle::Heading].family, medium());
        assert_eq!(styles[&subheading()].size, 15.0);
        assert_eq!(styles[&subheading()].family, medium());
        assert_eq!(styles[&TextStyle::Body].size, 15.0);
        assert_eq!(styles[&TextStyle::Button].family, medium());
        assert_eq!(styles[&TextStyle::Small].size, 12.0);
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
        assert_eq!(dark.window_stroke.color, DARK_HAIRLINE_STRONG);
        assert_eq!(dark.extreme_bg_color, DARK_SURFACE);
        assert_eq!(dark.hyperlink_color, DARK_ACCENT);

        let light = light_visuals();
        assert!(!light.dark_mode);
        assert_eq!(light.window_fill, LIGHT_WINDOW);
        assert_eq!(light.panel_fill, LIGHT_PANEL);
        assert_eq!(light.selection.stroke.width, 2.0);
    }
}

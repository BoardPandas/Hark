//! Hark's visual identity: the four palettes, embedded fonts, type scale,
//! spacing, and icon glyphs. Panels consume these tokens so the native UI
//! changes appearance consistently without per-panel color overrides.

use egui::epaint::Shadow;
use egui::{
    Color32, Context, FontFamily, FontId, Margin, Rect, RichText, Sense, Stroke, TextStyle, Ui,
    Vec2, Visuals,
};
use std::collections::BTreeMap;
mod appearance;
mod fonts;
mod palette;
pub use appearance::{appearance, appearance_picker, set_appearance, Appearance};
use fonts::font_definitions;
pub use fonts::{icon, icon_label_job, icon_text};
use palette::{palette, DARK, LIGHT};

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
    pub const HOME: &str = "\u{E2C2}";
    pub const KEY: &str = "\u{E2D6}";
    pub const LIGHTNING: &str = "\u{E2DE}";
    pub const MAGNIFYING_GLASS: &str = "\u{E30C}";
    pub const MICROPHONE: &str = "\u{E326}";
    pub const MEETINGS: &str = "\u{E4D6}";
    pub const PLAY: &str = "\u{E3D0}";
    pub const SPINNER: &str = "\u{E66A}";
    pub const TRASH: &str = "\u{E4A6}";
    pub const WARNING: &str = "\u{E4E0}";
    pub const WAVEFORM: &str = "\u{E802}";
    pub const X: &str = "\u{E4F6}";
}

pub const DANGER: Color32 = Color32::from_rgb(0xF1, 0x9C, 0xAB);
pub const SUCCESS: Color32 = Color32::from_rgb(0x8B, 0xD5, 0xAF);
pub const WARNING: Color32 = Color32::from_rgb(0xE5, 0xBC, 0x75);
const LIGHT_DANGER: Color32 = Color32::from_rgb(0xB6, 0x3B, 0x50);
const LIGHT_SUCCESS: Color32 = Color32::from_rgb(0x23, 0x75, 0x4F);
const LIGHT_WARNING: Color32 = Color32::from_rgb(0x91, 0x5C, 0x10);
const ON_DARK_ACCENT: Color32 = Color32::from_rgb(0x18, 0x25, 0x18);

pub const TRAY_MARK: Color32 = ON_DARK_ACCENT;
pub const TRAY_ACCENT: Color32 = DARK.accent;
pub const TRAY_STOPPED: Color32 = Color32::from_rgb(0x8A, 0x8F, 0x98);
pub const OVERLAY_ACCENT: Color32 = DARK.accent;
pub const OVERLAY_PILL_FILL: Color32 = DARK.surface;
pub const OVERLAY_PILL_STROKE: Color32 = DARK.hairline;
pub const OVERLAY_TEXT: Color32 = DARK.text;
pub const OVERLAY_SIZE: Vec2 = Vec2::new(192.0, 46.0);
pub const OVERLAY_SPINNER_STROKE: f32 = 2.0;
pub const OVERLAY_ICON_SIZE: f32 = 20.0;
pub const OVERLAY_HIGHLIGHT: Color32 = Color32::from_rgb(0x46, 0x52, 0x48);
pub const OVERLAY_FONT: f32 = 13.0;
pub const OVERLAY_WAVE_WIDTH: f32 = 3.0;
pub const OVERLAY_WAVE_GAP: f32 = 3.0;
pub const OVERLAY_WAVE_HEIGHT: f32 = 22.0;
pub const OVERLAY_INSET: f32 = 24.0;
pub const OVERLAY_LABEL_OFFSET: f32 = 36.0;
pub const FOOTER_HEIGHT: f32 = 38.0;
pub const CONTENT_WIDTH: f32 = 1020.0;
pub const SIDEBAR_WIDTH: f32 = 184.0;
pub const SIDEBAR_BREAKPOINT: f32 = 760.0;
pub const SIDEBAR_MIN_HEIGHT: f32 = 500.0;
pub const NAV_HEIGHT: f32 = 40.0;
pub const HERO_PADDING: i8 = 28;
pub const HERO_SIZE: f32 = 37.0;
pub const METRIC_MIN_WIDTH: f32 = 155.0;
pub const CHART_HEIGHT: f32 = 134.0;
pub const CHART_AXIS_WIDTH: f32 = 36.0;
pub const HEAT_CELL_GAP: f32 = 4.0;
pub const CHART_BAR_GAP: f32 = 4.0;
pub const SETTINGS_NAV_WIDTH: f32 = 156.0;
pub const SETTINGS_BREAKPOINT: f32 = 670.0;
pub const CONTROL_HEIGHT: f32 = 34.0;
pub const CONTROL_RADIUS: u8 = 7;
pub const SURFACE_RADIUS: u8 = 14;
pub const DIALOG_RADIUS: u8 = 12;
pub const GAP: f32 = 8.0;
pub const SECTION_GAP: f32 = 24.0;
pub const ROW_GAP: f32 = 16.0;
pub const SHADOW_MARGIN: Margin = Margin::same(4);
pub const CARD_PADDING: i8 = 18;
pub const CONTENT_MARGIN: Margin = Margin {
    left: 28,
    right: 28,
    top: 30,
    bottom: 20,
};
pub const TITLE_SIZE: f32 = 29.0;
pub const STAT_SIZE: f32 = 31.0;
pub const BRAND_SIZE: f32 = 31.0;
pub const META_SIZE: f32 = 12.0;
pub const EMPTY_ICON_SIZE: f32 = 30.0;
pub const EMPTY_GAP: f32 = 48.0;
pub const DIALOG_WIDTH: f32 = 400.0;
pub const DIALOG_TITLE_SIZE: f32 = 20.0;
pub const TOOLBAR_SEARCH_WIDTH: f32 = 320.0;
pub const LIST_ACTIONS_WIDTH: f32 = 116.0;
pub const MIN_CARD_WIDTH: f32 = 240.0;

pub fn danger(v: &Visuals) -> Color32 {
    if v.dark_mode {
        DANGER
    } else {
        LIGHT_DANGER
    }
}
pub fn success(v: &Visuals) -> Color32 {
    if v.dark_mode {
        SUCCESS
    } else {
        LIGHT_SUCCESS
    }
}
pub fn warning(v: &Visuals) -> Color32 {
    if v.dark_mode {
        WARNING
    } else {
        LIGHT_WARNING
    }
}
pub fn on_accent(v: &Visuals) -> Color32 {
    palette(v).on_accent
}
pub fn tint(v: &Visuals) -> Color32 {
    palette(v).tint
}
pub fn sidebar(v: &Visuals) -> Color32 {
    palette(v).sidebar
}
pub fn chart(v: &Visuals) -> Color32 {
    palette(v).chart
}
pub fn chart_soft(v: &Visuals) -> Color32 {
    palette(v).chart_soft
}

/// Editorial serif for the Home introduction and brand, embedded for all OSes.
pub fn serif() -> FontFamily {
    FontFamily::Name("Lora".into())
}
pub fn hero_font() -> FontId {
    FontId::new(HERO_SIZE, serif())
}
pub fn edge(v: &Visuals) -> Color32 {
    Color32::from_white_alpha(if v.dark_mode { 18 } else { 220 })
}
pub fn surface_shadow(v: &Visuals) -> Shadow {
    Shadow {
        offset: [0, 3],
        blur: 10,
        spread: 0,
        color: Color32::from_black_alpha(if v.dark_mode { 48 } else { 13 }),
    }
}
pub fn surface_frame(v: &Visuals) -> egui::Frame {
    egui::Frame::new()
        .fill(surface(v))
        .stroke(v.widgets.noninteractive.bg_stroke)
        .corner_radius(SURFACE_RADIUS)
        .inner_margin(Margin::same(CARD_PADDING))
        .shadow(surface_shadow(v))
}
pub fn card<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    let response = surface_frame(ui.visuals()).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        add_contents(ui)
    });
    highlight(ui, response.response.rect);
    response
}
pub fn highlight(ui: &Ui, rect: Rect) {
    ui.painter().hline(
        rect.left() + SURFACE_RADIUS as f32..=rect.right() - SURFACE_RADIUS as f32,
        rect.top() + 1.0,
        Stroke::new(1.0, edge(ui.visuals())),
    );
}
pub fn dialog_frame(v: &Visuals) -> egui::Frame {
    surface_frame(v)
        .inner_margin(Margin::same(24))
        .corner_radius(DIALOG_RADIUS)
        .shadow(v.window_shadow)
}
pub fn nav_button(
    ui: &mut Ui,
    label: impl Into<egui::WidgetText>,
    selected: bool,
) -> egui::Response {
    let mut button = egui::Button::new(label)
        .min_size(Vec2::new(0.0, CONTROL_HEIGHT))
        .selected(selected)
        .frame_when_inactive(selected);
    if selected {
        button = button
            .fill(surface(ui.visuals()))
            .stroke(ui.visuals().widgets.noninteractive.bg_stroke);
    }
    let response = ui.add(button);
    if selected {
        highlight(ui, response.rect);
    }
    focus_ring(ui, &response);
    response
}

/// Section heads use Inter Medium; hierarchy comes from size and spacing.
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
    visuals.hyperlink_color
}

/// Accent fill for primary actions; `on_accent` is its tested text pair.
pub fn accent_fill(visuals: &Visuals) -> Color32 {
    palette(visuals).accent
}

/// Cards, expanded detail panels, and dialogs.
pub fn surface(visuals: &Visuals) -> Color32 {
    visuals.faint_bg_color
}

/// The translucent divider used by the fading rules (text at 16% alpha).
pub fn divider(visuals: &Visuals) -> Color32 {
    let base = visuals.text_color();
    Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), 41)
}

/// Quiet, crisp row divider. The former fading edges made list alignment fuzzy.
pub fn fading_rule(ui: &mut Ui, gap: f32) {
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), gap.max(1.0)),
        Sense::hover(),
    );
    paint_fading_rule(ui, rect);
}
pub fn paint_fading_rule(ui: &Ui, rect: Rect) {
    ui.painter().hline(
        rect.x_range(),
        rect.center().y.round(),
        Stroke::new(1.0, divider(ui.visuals())),
    );
}

/// The single high-emphasis action in a form; text and fill are a tested pair.
pub fn primary_button(visuals: &Visuals, text: impl Into<egui::WidgetText>) -> ActionButton {
    ActionButton(
        egui::Button::new(text.into().color(on_accent(visuals)))
            .fill(accent_fill(visuals))
            .stroke(Stroke::new(1.0, edge(visuals)))
            .min_size(Vec2::new(0.0, CONTROL_HEIGHT)),
    )
}
pub fn danger_button(visuals: &Visuals, text: impl Into<String>) -> ActionButton {
    ActionButton(
        egui::Button::new(RichText::new(text.into()).color(danger(visuals)))
            .stroke(visuals.widgets.noninteractive.bg_stroke),
    )
}

/// Keep native button interaction/accessibility and add a visible focus ring
/// outside the custom fill, where it cannot disappear against the accent.
pub struct ActionButton(egui::Button<'static>);
impl egui::Widget for ActionButton {
    fn ui(self, ui: &mut Ui) -> egui::Response {
        let response = ui.add(self.0);
        focus_ring(ui, &response);
        response
    }
}
pub(crate) fn focus_ring(ui: &Ui, response: &egui::Response) {
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect.expand(3.0),
            CONTROL_RADIUS,
            ui.visuals().selection.stroke,
            egui::StrokeKind::Outside,
        );
    }
}

/// Install fonts and tokens once, restoring the explicit palette or the
/// legacy Light/Dark/System preference already loaded by eframe.
pub fn apply(ctx: &Context) {
    ctx.set_fonts(font_definitions());
    ctx.all_styles_mut(|style| {
        style.text_styles = text_styles();
        spacing(&mut style.spacing);
    });
    let selected: Appearance = appearance(ctx);
    appearance::install(ctx, selected);
}

/// Inter hierarchy for readable text; JetBrains Mono for technical values.
fn text_styles() -> BTreeMap<TextStyle, FontId> {
    BTreeMap::from([
        (TextStyle::Heading, FontId::new(TITLE_SIZE, medium())),
        (subheading(), FontId::new(15.0, medium())),
        (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(13.0, medium())),
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

/// Consistent spacing and comfortable native control targets.
fn spacing(spacing: &mut egui::style::Spacing) {
    spacing.item_spacing = Vec2::new(8.0, 8.0);
    spacing.button_padding = Vec2::new(12.0, 6.0);
    spacing.window_margin = Margin::same(24);
    spacing.menu_margin = Margin::same(8);
    spacing.indent = 18.0;
    spacing.interact_size.y = CONTROL_HEIGHT;
}

#[cfg(test)]
mod tests;

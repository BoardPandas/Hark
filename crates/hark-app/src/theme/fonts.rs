use super::*;
use egui::{FontData, FontDefinitions};
use std::sync::Arc;

/// Inter Regular/Medium/SemiBold each as their own family (egui cannot
/// interpolate variable-font weights, emilk/egui#1862), JetBrains Mono for
/// transcripts and latency figures, Phosphor for icons. egui's default
/// fonts stay appended as emoji/coverage fallback.
pub(super) fn font_definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    for (name, bytes) in [
        (
            "Inter",
            &include_bytes!("../../assets/Inter-Regular.ttf")[..],
        ),
        (
            "InterMedium",
            &include_bytes!("../../assets/Inter-Medium.ttf")[..],
        ),
        (
            "InterSemiBold",
            &include_bytes!("../../assets/Inter-SemiBold.ttf")[..],
        ),
        (
            "JetBrainsMono",
            &include_bytes!("../../assets/JetBrainsMono-Regular.ttf")[..],
        ),
        ("Phosphor", &include_bytes!("../../assets/Phosphor.ttf")[..]),
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

    // Icons get a family that leads with Phosphor, and it is not optional.
    // Inter ships 745 Private-Use-Area glyphs of its own, five of which sit on
    // codepoints Phosphor uses (ARROW_UP, BOOK_OPEN, CHART_BAR, GEAR, KEY), so
    // in any family where Inter comes first egui resolves those five to
    // Inter's glyph and never reaches Phosphor: the Settings tab rendered a
    // stray letter instead of a gear. Phosphor cannot simply lead the shared
    // families in return — it maps a..z and would swallow ordinary lowercase
    // text — so icons need their own family and `icon_text` to reach it.
    let mut icons = vec!["Phosphor".to_string(), "Inter".to_string()];
    icons.extend(fallback.iter().cloned());
    fonts.families.insert(icon(), icons);

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

/// The family icons must render in: Phosphor first, so Inter's Private-Use
/// glyphs cannot shadow one. Only ever use it for glyphs — Phosphor maps the
/// lowercase Latin range, so ordinary text set in this family renders wrong.
pub fn icon() -> FontFamily {
    FontFamily::Name("Icon".into())
}

/// One icon glyph, in the family that actually resolves it. Every icon must go
/// through here; `RichText::new(icons::GEAR)` silently renders Inter's glyph.
pub fn icon_text(glyph: &str) -> RichText {
    RichText::new(glyph).family(icon())
}

/// An icon followed by a label, as one widget. Two sections rather than one
/// string, because the icon and the text need different families and a single
/// `format!` can only have one.
pub fn icon_label_job(style: &egui::Style, glyph: &str, text: &str) -> egui::text::LayoutJob {
    use egui::text::{LayoutJob, TextFormat};
    let size = TextStyle::Button.resolve(style).size;
    // PLACEHOLDER lets a caller recolour the whole job (primary_button tints
    // it accent); a concrete colour here would win and ignore them.
    let color = Color32::PLACEHOLDER;
    let mut job = LayoutJob::default();
    job.append(
        glyph,
        0.0,
        TextFormat::simple(FontId::new(size, icon()), color),
    );
    job.append(
        text,
        6.0,
        TextFormat::simple(FontId::new(size, medium()), color),
    );
    job
}

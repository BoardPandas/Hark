use super::*;
use egui::epaint::text::VariationCoords;
use egui::{FontData, FontDefinitions, FontTweak};
use std::sync::Arc;

const ALEGREYA: &[u8] = include_bytes!("../../assets/Alegreya[wght].ttf");

/// Families are named for their role (`Medium`, `SemiBold`, `Serif`,
/// `DropCap`), not their font, because the lettering decides which font
/// leads them. Every font is registered under both letterings, so a switch
/// changes family lists only. Modern: Inter Regular/Medium/SemiBold as static
/// files, Lora for the hero, brand and drop caps. Wizard: Alegreya at three
/// pinned weights for text, Cinzel Decorative for titles and drop caps.
/// JetBrains Mono (key names, timings, literal aliases) and the Phosphor icon
/// family are the same in both. egui's default fonts stay appended as
/// emoji/coverage fallback.
pub(super) fn font_definitions(lettering: Lettering) -> FontDefinitions {
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
        ("Lora", &include_bytes!("../../assets/Lora-Regular.ttf")[..]),
        (
            "CinzelDecorative",
            &include_bytes!("../../assets/CinzelDecorative-Regular.ttf")[..],
        ),
        (
            "CinzelDecorativeBold",
            &include_bytes!("../../assets/CinzelDecorative-Bold.ttf")[..],
        ),
    ] {
        fonts
            .font_data
            .insert(name.to_string(), Arc::new(FontData::from_static(bytes)));
    }
    // One variable file, registered once per weight with its instance pinned
    // (assets/README.md, "Weights"; the spike test proves the pin holds).
    for (name, weight) in [
        ("Alegreya", 400.0),
        ("AlegreyaMedium", 500.0),
        ("AlegreyaSemiBold", 600.0),
    ] {
        let data = FontData::from_static(ALEGREYA).tweak(FontTweak {
            coords: VariationCoords::new([(b"wght", weight)]),
            ..Default::default()
        });
        fonts.font_data.insert(name.to_string(), Arc::new(data));
    }

    let fallback = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    // [lead, Phosphor, coverage..., egui defaults]. Phosphor stays second in
    // every text family so an icon string that slips into one still resolves.
    let with_fallback = |lead: &str, coverage: &[&str]| {
        let mut list = vec![lead.to_string(), "Phosphor".to_string()];
        list.extend(coverage.iter().map(|name| name.to_string()));
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

    let text_families = match lettering {
        Lettering::Modern => [
            (FontFamily::Proportional, with_fallback("Inter", &[])),
            (medium(), with_fallback("InterMedium", &[])),
            (semibold(), with_fallback("InterSemiBold", &[])),
            (serif(), with_fallback("Lora", &[])),
            (drop_cap(), with_fallback("Lora", &[])),
        ],
        // Inter after Phosphor keeps Modern's symbol coverage (arrows, ⌘)
        // for anything Alegreya lacks.
        // The serif families add Alegreya first, for the two characters
        // Cinzel Decorative does not map (`CINZEL_GAPS` in the tests).
        Lettering::Wizard => [
            (
                FontFamily::Proportional,
                with_fallback("Alegreya", &["Inter"]),
            ),
            (medium(), with_fallback("AlegreyaMedium", &["InterMedium"])),
            (
                semibold(),
                with_fallback("AlegreyaSemiBold", &["InterSemiBold"]),
            ),
            (
                serif(),
                with_fallback("CinzelDecorative", &["Alegreya", "Inter"]),
            ),
            (
                drop_cap(),
                with_fallback("CinzelDecorativeBold", &["Alegreya", "Inter"]),
            ),
        ],
    };
    fonts.families.extend(text_families);
    let mut mono = with_fallback("JetBrainsMono", &[]);
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
    // PLACEHOLDER lets a caller recolour the whole job (primary_button tints
    // it accent); a concrete colour here would win and ignore them.
    hued_icon_label_job(style, glyph, text, Color32::PLACEHOLDER)
}

/// `icon_label_job` with the icon alone in `ink` (a page or section hue); the
/// label keeps the widget's text color, so it reads the same in every hue.
pub fn hued_icon_label_job(
    style: &egui::Style,
    glyph: &str,
    text: &str,
    ink: Color32,
) -> egui::text::LayoutJob {
    use egui::text::{LayoutJob, TextFormat};
    let size = TextStyle::Button.resolve(style).size;
    let mut job = LayoutJob::default();
    job.append(
        glyph,
        0.0,
        TextFormat::simple(FontId::new(size, icon()), ink),
    );
    job.append(
        text,
        6.0,
        TextFormat::simple(FontId::new(size, medium()), Color32::PLACEHOLDER),
    );
    job
}

/// A label followed by an icon ("Next ›"): `icon_label_job` mirrored, for a
/// control whose icon points the way it goes.
pub fn label_icon_job(style: &egui::Style, text: &str, glyph: &str) -> egui::text::LayoutJob {
    use egui::text::{LayoutJob, TextFormat};
    let size = TextStyle::Button.resolve(style).size;
    let color = Color32::PLACEHOLDER;
    let mut job = LayoutJob::default();
    job.append(
        text,
        0.0,
        TextFormat::simple(FontId::new(size, medium()), color),
    );
    job.append(
        glyph,
        6.0,
        TextFormat::simple(FontId::new(size, icon()), color),
    );
    job
}

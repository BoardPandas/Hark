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

fn palettes() -> [(&'static str, Visuals); 4] {
    [
        ("light", palette::visuals(&LIGHT)),
        ("dark", palette::visuals(&DARK)),
        (
            "solarized-light",
            palette::visuals(&palette::SOLARIZED_LIGHT),
        ),
        ("solarized-dark", palette::visuals(&palette::SOLARIZED_DARK)),
    ]
}

#[test]
fn text_meets_wcag_aa_across_all_four_palettes_and_surfaces() {
    for (name, v) in palettes() {
        for background in [
            v.window_fill,
            v.panel_fill,
            surface(&v),
            sidebar(&v),
            tint(&v),
        ] {
            for foreground in [
                v.text_color(),
                v.weak_text_color(),
                danger(&v),
                success(&v),
                warning(&v),
                accent(&v),
            ] {
                let ratio = contrast(foreground, background);
                assert!(
                    ratio >= 4.5,
                    "{name}: {foreground:?} on {background:?}: {ratio:.2}"
                );
            }
        }
    }
}

#[test]
fn primary_button_labels_and_focus_rings_have_readable_contrast() {
    for (name, v) in palettes() {
        assert!(
            contrast(on_accent(&v), accent_fill(&v)) >= 4.5,
            "{name} primary button"
        );
        for background in [v.panel_fill, surface(&v), sidebar(&v), tint(&v)] {
            assert!(
                contrast(v.selection.stroke.color, background) >= 3.0,
                "{name} focus ring"
            );
        }
        assert_eq!(v.selection.stroke.width, 2.0);
    }
    assert!(contrast(OVERLAY_TEXT, OVERLAY_PILL_FILL) >= 4.5);
    for fill in [DANGER, WARNING, TRAY_STOPPED, TRAY_ACCENT] {
        assert!(contrast(TRAY_MARK, fill) >= 4.5);
    }
}

#[test]
fn elevated_cards_reserve_their_padding_without_horizontal_overflow() {
    let ctx = Context::default();
    apply(&ctx);
    for width in [280.0, 480.0, 860.0] {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(width, 480.0),
                )),
                ..Default::default()
            },
            |ui| {
                let response = card(ui, |ui| {
                    ui.label("Connection settings");
                    ui.text_edit_singleline(&mut String::new());
                });
                assert!(
                    response.response.rect.right() <= width,
                    "card overflows {width}"
                );
            },
        );
        output.textures_delta.clear();
    }
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
    assert_eq!(styles[&TextStyle::Heading].size, TITLE_SIZE);
    assert_eq!(styles[&TextStyle::Heading].family, medium());
    assert_eq!(styles[&subheading()].size, 15.0);
    assert_eq!(styles[&subheading()].family, medium());
    assert_eq!(styles[&TextStyle::Body].size, 14.0);
    assert_eq!(styles[&TextStyle::Button].family, medium());
    assert_eq!(styles[&TextStyle::Small].size, 12.0);
    assert_eq!(styles[&TextStyle::Monospace].size, 13.0);
    assert_eq!(styles[&TextStyle::Monospace].family, FontFamily::Monospace);
}

/// The bug this pins: Inter ships 745 Private-Use-Area glyphs, five of
/// which collide with Phosphor's (ARROW_UP, BOOK_OPEN, CHART_BAR, GEAR,
/// KEY). egui resolves a glyph through the family list in order, so in any
/// family led by Inter those five render as Inter's glyph and Phosphor is
/// never consulted — the Settings tab showed a stray letter where the gear
/// should be, and nothing anywhere reported a problem.
#[test]
fn the_icon_family_leads_with_phosphor() {
    let fonts = font_definitions();
    let list = &fonts.families[&icon()];
    assert_eq!(
        list[0], "Phosphor",
        "icons must resolve to Phosphor BEFORE any text font, or Inter's \
         private-use glyphs shadow them"
    );
    // ...and Inter stays reachable, so an icon string that also contains
    // text does not fall all the way through to egui's defaults.
    assert!(list.contains(&"Inter".to_string()));
}

/// Phosphor maps the lowercase Latin range, which is why it cannot simply
/// lead the shared families and why `icon_text` exists at all. If this ever
/// stops being true the whole icon family could be retired.
#[test]
fn phosphor_would_swallow_lowercase_text_if_it_led_a_text_family() {
    let fonts = font_definitions();
    for family in [FontFamily::Proportional, medium(), semibold()] {
        assert_ne!(
            fonts.families[&family][0], "Phosphor",
            "{family:?} must lead with a text font"
        );
    }
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
        "Lora",
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
    leads(&serif(), "Lora");
    leads(&FontFamily::Monospace, "JetBrainsMono");
}

#[test]
fn solarized_uses_official_base_fills_and_cyan_charts() {
    let dark = palette::visuals(&palette::SOLARIZED_DARK);
    let light = palette::visuals(&palette::SOLARIZED_LIGHT);
    assert_eq!(dark.panel_fill, Color32::from_rgb(0x00, 0x2b, 0x36));
    assert_eq!(surface(&dark), Color32::from_rgb(0x07, 0x36, 0x42));
    assert_eq!(light.panel_fill, Color32::from_rgb(0xfd, 0xf6, 0xe3));
    assert_eq!(sidebar(&light), Color32::from_rgb(0xee, 0xe8, 0xd5));
    for v in [dark, light] {
        assert_eq!(chart(&v), Color32::from_rgb(0x2a, 0xa1, 0x98));
        assert_ne!(chart(&v), chart_soft(&v));
    }
}

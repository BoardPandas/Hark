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

/// Built from `palette::ALL`, so a new palette cannot skip these tests.
fn palettes() -> Vec<(String, Visuals)> {
    palette::ALL
        .into_iter()
        .enumerate()
        .map(|(i, p)| {
            (
                format!("palette {i} (window {:?})", p.window),
                palette::visuals(p),
            )
        })
        .collect()
}

#[test]
fn every_palette_is_found_again_from_its_own_visuals() {
    for (i, p) in palette::ALL.into_iter().enumerate() {
        assert_eq!(palette(&palette::visuals(p)), p, "palette {i}");
        for other in &palette::ALL[i + 1..] {
            assert_ne!(
                p.window, other.window,
                "palette {i} shares its window color, so one theme would render as another"
            );
        }
    }
}

#[test]
fn every_appearance_installs_palettes_from_the_tested_list() {
    let ctx = Context::default();
    apply(&ctx);
    for selected in Appearance::ALL {
        set_appearance(&ctx, selected);
        for variant in [egui::Theme::Light, egui::Theme::Dark] {
            let fill = ctx.style_of(variant).visuals.panel_fill;
            assert!(
                palette::ALL.iter().any(|p| p.window == fill),
                "{selected:?} / {variant:?} installs a palette outside palette::ALL"
            );
        }
    }
}

#[test]
fn text_meets_wcag_aa_across_every_palette_and_surface() {
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
            ]
            .into_iter()
            .chain(Hue::ALL.map(|h| hue(&v, h)))
            {
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
fn every_hue_ink_reads_on_its_own_wash() {
    for (name, v) in palettes() {
        for h in Hue::ALL {
            let ratio = contrast(hue(&v, h), hue_wash(&v, h));
            assert!(ratio >= 4.5, "{name}: {h:?} ink on wash: {ratio:.2}");
        }
    }
}

/// sRGB to OKLab (Björn Ottosson, 2020), for perceptual distance.
fn oklab(c: Color32) -> [f64; 3] {
    let linear = |v: u8| {
        let v = v as f64 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let (r, g, b) = (linear(c.r()), linear(c.g()), linear(c.b()));
    let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
    let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
    let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
    [
        0.210_454_255_3 * l + 0.793_617_785_0 * m - 0.004_072_046_8 * s,
        1.977_998_495_1 * l - 2.428_592_205_0 * m + 0.450_593_709_9 * s,
        0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766_0 * s,
    ]
}

#[test]
fn oklab_matches_its_reference_points() {
    // White is L=1 with no chroma; pure red is (0.628, 0.225, 0.126).
    let white = oklab(Color32::WHITE);
    assert!((white[0] - 1.0).abs() < 1e-3 && white[1].abs() < 1e-3 && white[2].abs() < 1e-3);
    let red = oklab(Color32::from_rgb(255, 0, 0));
    for (got, want) in red.iter().zip([0.628, 0.225, 0.126]) {
        assert!((got - want).abs() < 1e-3, "red {red:?}");
    }
}

/// Speakers and chart series are told apart by hue, so two inks of one
/// palette that look alike would silently merge two categories.
#[test]
fn the_six_inks_of_each_palette_are_perceptually_distinct() {
    for (name, v) in palettes() {
        for (i, a) in Hue::ALL.into_iter().enumerate() {
            for b in &Hue::ALL[i + 1..] {
                let (pa, pb) = (oklab(hue(&v, a)), oklab(hue(&v, *b)));
                let gap = pa
                    .iter()
                    .zip(pb)
                    .map(|(x, y)| (x - y).powi(2))
                    .sum::<f64>()
                    .sqrt();
                assert!(gap >= 0.06, "{name}: {a:?} and {b:?} are {gap:.3} apart");
            }
        }
    }
}

#[test]
fn me_is_always_blue_and_no_one_else_ever_is() {
    assert_eq!(speaker_hue(0), Hue::Blue);
    for slot in 1..64 {
        assert_ne!(speaker_hue(slot), Hue::Blue, "slot {slot}");
    }
}

#[test]
fn the_first_six_speakers_get_six_different_hues() {
    let first: Vec<Hue> = (0..6).map(speaker_hue).collect();
    for h in Hue::ALL {
        assert_eq!(first.iter().filter(|x| **x == h).count(), 1, "{h:?}");
    }
    assert_eq!(
        first,
        [
            Hue::Blue,
            Hue::Gold,
            Hue::Violet,
            Hue::Teal,
            Hue::Rose,
            Hue::Green
        ]
    );
}

#[test]
fn speaker_hues_wrap_over_slots_one_to_five() {
    assert_eq!(speaker_hue(6), speaker_hue(1));
    assert_eq!(speaker_hue(10), speaker_hue(5));
    assert_eq!(speaker_hue(11), speaker_hue(1));
}

#[test]
fn document_speaker_inks_read_on_white_paper() {
    for h in Hue::ALL {
        let ratio = contrast(document_hue(h), Color32::WHITE);
        assert!(ratio >= 4.5, "{h:?} on white: {ratio:.2}");
    }
    // The same table the palette tests cover, never a copied hex.
    let light = palette::visuals(&palette::LIGHT);
    for h in Hue::ALL {
        assert_eq!(document_hue(h), hue(&light, h));
    }
}

#[test]
fn a_chip_paints_its_label_in_ink_on_its_wash() {
    let ctx = Context::default();
    apply(&ctx);
    let mut rect = Rect::NOTHING;
    let mut v = Visuals::default();
    let mut output = ctx.run_ui(Default::default(), |ui| {
        v = ui.visuals().clone();
        rect = chip(ui, Hue::Violet, Some(icons::BOOK_OPEN), "New").rect;
    });
    output.textures_delta.clear();
    assert!(rect.height() < CONTROL_HEIGHT, "a chip is not a control");
    let mut wash = false;
    let mut label = None;
    for clipped in &output.shapes {
        match &clipped.shape {
            egui::Shape::Rect(r) if r.fill == hue_wash(&v, Hue::Violet) => wash = true,
            egui::Shape::Text(t) => label = Some(t.galley.text().to_owned()),
            _ => {}
        }
    }
    assert!(wash, "no wash-filled pill was painted");
    let label = label.expect("no text was painted");
    assert!(label.contains("New") && label.contains(icons::BOOK_OPEN));
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

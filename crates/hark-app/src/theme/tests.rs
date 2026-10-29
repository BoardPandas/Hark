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
    // (lettering, heading family, subheading, body, button, small)
    for (lettering, heading, sub, body, button, small) in [
        (Lettering::Modern, medium(), 15.0, 14.0, 13.0, 12.0),
        (Lettering::Wizard, serif(), 16.0, 15.0, 14.0, 13.0),
    ] {
        let styles = text_styles(lettering);
        assert_eq!(styles.len(), 6);
        assert_eq!(styles[&TextStyle::Heading].size, TITLE_SIZE);
        assert_eq!(styles[&TextStyle::Heading].family, heading);
        assert_eq!(styles[&subheading()].size, sub);
        assert_eq!(styles[&subheading()].family, medium());
        assert_eq!(styles[&TextStyle::Body].size, body);
        assert_eq!(styles[&TextStyle::Body].family, FontFamily::Proportional);
        assert_eq!(styles[&TextStyle::Button].size, button);
        assert_eq!(styles[&TextStyle::Button].family, medium());
        assert_eq!(styles[&TextStyle::Small].size, small);
        assert_eq!(styles[&TextStyle::Monospace].size, 13.0);
        assert_eq!(styles[&TextStyle::Monospace].family, FontFamily::Monospace);
    }
}

/// The bug this pins: Inter ships 745 Private-Use-Area glyphs, five of
/// which collide with Phosphor's (ARROW_UP, BOOK_OPEN, CHART_BAR, GEAR,
/// KEY). egui resolves a glyph through the family list in order, so in any
/// family led by Inter those five render as Inter's glyph and Phosphor is
/// never consulted — the Settings tab showed a stray letter where the gear
/// should be, and nothing anywhere reported a problem.
#[test]
fn the_icon_family_leads_with_phosphor() {
    for lettering in Lettering::ALL {
        let fonts = font_definitions(lettering);
        let list = &fonts.families[&icon()];
        assert_eq!(
            list[0], "Phosphor",
            "{lettering:?}: icons must resolve to Phosphor BEFORE any text font, \
             or Inter's private-use glyphs shadow them"
        );
        // ...and Inter stays reachable, so an icon string that also contains
        // text does not fall all the way through to egui's defaults.
        assert!(list.contains(&"Inter".to_string()));
    }
}

/// Phosphor maps the lowercase Latin range, which is why it cannot simply
/// lead the shared families and why `icon_text` exists at all. If this ever
/// stops being true the whole icon family could be retired.
#[test]
fn phosphor_would_swallow_lowercase_text_if_it_led_a_text_family() {
    for lettering in Lettering::ALL {
        let fonts = font_definitions(lettering);
        for family in [FontFamily::Proportional, medium(), semibold(), serif()] {
            assert_ne!(
                fonts.families[&family][0], "Phosphor",
                "{lettering:?}: {family:?} must lead with a text font"
            );
        }
    }
}

#[test]
fn every_family_resolves_and_leads_with_the_intended_font() {
    for (lettering, text, med, semi, title, cap) in [
        (
            Lettering::Modern,
            "Inter",
            "InterMedium",
            "InterSemiBold",
            "Lora",
            "Lora",
        ),
        (
            Lettering::Wizard,
            "Alegreya",
            "AlegreyaMedium",
            "AlegreyaSemiBold",
            "CinzelDecorative",
            "CinzelDecorativeBold",
        ),
    ] {
        let fonts = font_definitions(lettering);
        // Every font is registered under both letterings, so a switch only
        // changes family lists.
        for name in [
            "Inter",
            "InterMedium",
            "InterSemiBold",
            "JetBrainsMono",
            "Phosphor",
            "Lora",
            "Alegreya",
            "AlegreyaMedium",
            "AlegreyaSemiBold",
            "CinzelDecorative",
            "CinzelDecorativeBold",
        ] {
            assert!(fonts.font_data.contains_key(name), "missing font {name}");
        }
        let leads = |family: &FontFamily, expected: &str| {
            let list = &fonts.families[family];
            assert_eq!(list[0], expected, "{lettering:?} family {family:?}");
            assert_eq!(
                list[1], "Phosphor",
                "{lettering:?}: icons must fall back in {family:?}"
            );
            for name in list {
                assert!(
                    fonts.font_data.contains_key(name)
                        || egui::FontDefinitions::default()
                            .font_data
                            .contains_key(name),
                    "{lettering:?} {family:?} names an unregistered font {name}"
                );
            }
        };
        leads(&FontFamily::Proportional, text);
        leads(&medium(), med);
        leads(&semibold(), semi);
        leads(&serif(), title);
        leads(&drop_cap(), cap);
        leads(&FontFamily::Monospace, "JetBrainsMono");
    }
}

/// Modern lettering is today's lettering, list for list: the switch must not
/// move a single glyph for anyone who never turns it on.
#[test]
fn modern_lettering_keeps_the_existing_family_lists() {
    let fonts = font_definitions(Lettering::Modern);
    let defaults = egui::FontDefinitions::default();
    let fallback = &defaults.families[&FontFamily::Proportional];
    let chain = |lead: &str| {
        let mut list = vec![lead.to_string(), "Phosphor".to_string()];
        list.extend(fallback.iter().cloned());
        list
    };
    assert_eq!(fonts.families[&FontFamily::Proportional], chain("Inter"));
    assert_eq!(fonts.families[&medium()], chain("InterMedium"));
    assert_eq!(fonts.families[&semibold()], chain("InterSemiBold"));
    assert_eq!(fonts.families[&serif()], chain("Lora"));
    let mut mono = chain("JetBrainsMono");
    mono.extend(defaults.families[&FontFamily::Monospace].iter().cloned());
    assert_eq!(fonts.families[&FontFamily::Monospace], mono);
    let mut icons = vec!["Phosphor".to_string(), "Inter".to_string()];
    icons.extend(fallback.iter().cloned());
    assert_eq!(fonts.families[&icon()], icons);
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

const ALEGREYA: &[u8] = include_bytes!("../../assets/Alegreya[wght].ttf");

/// Spike (plan F4): egui 0.36 pins a variable font's instance through
/// `FontTweak::coords`. If it ignored them, all three widths would match and
/// Wizard's Medium and SemiBold would silently render as Regular.
#[test]
fn alegreya_weights_are_pinned_by_font_tweak_coords() {
    use egui::epaint::text::VariationCoords;
    use std::sync::Arc;
    const WEIGHTS: [f32; 3] = [400.0, 500.0, 600.0];
    let family = |w: f32| FontFamily::Name(format!("probe-{w}").into());
    let mut fonts = egui::FontDefinitions::default();
    for w in WEIGHTS {
        let name = format!("probe-{w}");
        let data = egui::FontData::from_static(ALEGREYA).tweak(egui::FontTweak {
            coords: VariationCoords::new([(b"wght", w)]),
            ..Default::default()
        });
        fonts.font_data.insert(name.clone(), Arc::new(data));
        fonts.families.insert(family(w), vec![name]);
    }
    let ctx = Context::default();
    ctx.set_fonts(fonts);
    let mut widths = [0.0; 3];
    let mut output = ctx.run_ui(Default::default(), |ui| {
        widths = WEIGHTS.map(|w| {
            ui.fonts_mut(|f| {
                f.layout_no_wrap(
                    "The quick brown fox jumps over the lazy dog".into(),
                    FontId::new(15.0, family(w)),
                    Color32::BLACK,
                )
                .size()
                .x
            })
        });
    });
    output.textures_delta.clear();
    eprintln!("Alegreya widths at wght 400/500/600: {widths:?}");
    assert!(
        widths[0] < widths[1] && widths[1] < widths[2],
        "coords not applied: {widths:?}"
    );
}

/// Spec AC7: ASCII, Latin-1, and the typographic punctuation the UI writes.
fn coverage_string() -> String {
    (' '..='~')
        .chain('\u{A0}'..='\u{FF}')
        .chain("“”‘’–—…•·×".chars())
        .collect()
}

/// Characters the Cinzel Decorative files (google/fonts 6157dfaa) do not map:
/// the no-break space and the micro sign. Phosphor lacks both too, so the
/// Wizard serif family puts Alegreya right after Phosphor to catch them in a
/// book face. The font cannot be patched (its Reserved Font Name), so this
/// gap is pinned instead: a font update that closes it fails this test and
/// the workaround can go.
const CINZEL_GAPS: &str = "\u{A0}\u{B5}";

/// Which coverage-string characters each face lacks on its own. A probe
/// family holds only the face under test, because in a real family a missing
/// glyph falls through to the next font and renders in the wrong face with
/// no error. (`has_glyph` cannot answer this: in a one-face family that face
/// is also the replacement face, so it reports `false` for every char. The
/// charmap from `characters()` is the honest answer.)
fn missing_from_each_face(faces: &[(&str, &'static [u8])]) -> Vec<String> {
    use std::sync::Arc;
    let mut fonts = egui::FontDefinitions::default();
    for (name, bytes) in faces {
        let name = format!("probe-{name}");
        fonts
            .font_data
            .insert(name.clone(), Arc::new(egui::FontData::from_static(bytes)));
        fonts
            .families
            .insert(FontFamily::Name(name.clone().into()), vec![name]);
    }
    let ctx = Context::default();
    ctx.set_fonts(fonts);
    let mut missing = Vec::new();
    let mut output = ctx.run_ui(Default::default(), |ui| {
        missing = faces
            .iter()
            .map(|(name, _)| {
                ui.fonts_mut(|f| {
                    let family = FontFamily::Name(format!("probe-{name}").into());
                    let charmap = f.fonts.font(&family).characters().clone();
                    coverage_string()
                        .chars()
                        .filter(|c| !charmap.contains_key(c))
                        .collect()
                })
            })
            .collect();
    });
    output.textures_delta.clear();
    missing
}

#[test]
fn the_wizard_faces_cover_the_coverage_string_alone() {
    let faces: [(&str, &'static [u8]); 3] = [
        ("Alegreya", ALEGREYA),
        (
            "CinzelDecorative",
            include_bytes!("../../assets/CinzelDecorative-Regular.ttf"),
        ),
        (
            "CinzelDecorativeBold",
            include_bytes!("../../assets/CinzelDecorative-Bold.ttf"),
        ),
    ];
    let missing = missing_from_each_face(&faces);
    assert_eq!(missing[0], "", "the Wizard body face");
    for (face, gaps) in faces[1..].iter().zip(&missing[1..]) {
        assert_eq!(gaps, CINZEL_GAPS, "{} changed coverage", face.0);
    }
}

/// Which font each coverage-string character resolves to in each family of
/// the shipped definitions. `characters()` lists, per char, the faces that
/// map it in family order, so the first is the one egui draws.
fn first_faces(lettering: Lettering, families: &[FontFamily]) -> Vec<Vec<(char, String)>> {
    let ctx = Context::default();
    ctx.set_fonts(font_definitions(lettering));
    let mut faces = Vec::new();
    let mut output = ctx.run_ui(Default::default(), |ui| {
        faces = families
            .iter()
            .map(|family| {
                ui.fonts_mut(|f| {
                    let charmap = f.fonts.font(family).characters().clone();
                    coverage_string()
                        .chars()
                        .map(|c| (c, charmap.get(&c).map_or("none", |v| &v[0]).to_owned()))
                        .collect()
                })
            })
            .collect();
    });
    output.textures_delta.clear();
    faces
}

/// AC6/AC7 against what ships: under Wizard every coverage char is drawn by
/// the Wizard face itself, never by Phosphor (which maps a..z) or Inter, and
/// the serif families reach Alegreya — not Phosphor — for Cinzel's gaps.
#[test]
fn wizard_families_draw_the_coverage_string_in_their_own_faces() {
    let families = [
        (FontFamily::Proportional, "Alegreya"),
        (medium(), "AlegreyaMedium"),
        (semibold(), "AlegreyaSemiBold"),
        (serif(), "CinzelDecorative"),
        (drop_cap(), "CinzelDecorativeBold"),
    ];
    let resolved = first_faces(
        Lettering::Wizard,
        &families.clone().map(|(family, _)| family),
    );
    for ((family, lead), chars) in families.iter().zip(resolved) {
        for (c, face) in chars {
            let expected = if lead.starts_with("Cinzel") && CINZEL_GAPS.contains(c) {
                "Alegreya"
            } else {
                lead
            };
            assert_eq!(face, expected, "{family:?}: {c:?} (U+{:04X})", c as u32);
        }
    }
}

/// The spike, repeated on the shipped families: Wizard's Regular, Medium and
/// SemiBold roles really are three different weights.
#[test]
fn wizard_text_roles_render_at_three_weights() {
    let ctx = Context::default();
    ctx.set_fonts(font_definitions(Lettering::Wizard));
    let mut widths = [0.0; 3];
    let mut output = ctx.run_ui(Default::default(), |ui| {
        widths = [FontFamily::Proportional, medium(), semibold()].map(|family| {
            ui.fonts_mut(|f| {
                f.layout_no_wrap(
                    "The quick brown fox jumps over the lazy dog".into(),
                    FontId::new(15.0, family),
                    Color32::BLACK,
                )
                .size()
                .x
            })
        });
    });
    output.textures_delta.clear();
    assert!(
        widths[0] < widths[1] && widths[1] < widths[2],
        "weights did not pin: {widths:?}"
    );
}

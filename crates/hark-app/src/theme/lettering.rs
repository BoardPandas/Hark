//! Wizard lettering: a book face for headings and text, independent of the
//! theme. Until the user chooses, it follows the theme (Wizard on Parchment,
//! Modern elsewhere); once chosen, the choice holds on every theme. Like the
//! appearance, it lives in egui memory, which eframe persists.
use super::*;
use egui::Id;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Lettering {
    /// Inter, with Lora for the hero and brand.
    Modern,
    /// Alegreya for text, Cinzel Decorative for titles and drop caps.
    Wizard,
}

impl Lettering {
    pub const ALL: [Self; 2] = [Self::Modern, Self::Wizard];

    fn key(self) -> &'static str {
        match self {
            Self::Modern => "modern",
            Self::Wizard => "wizard",
        }
    }

    fn derived(appearance: Appearance) -> Self {
        if appearance == Appearance::Parchment {
            Self::Wizard
        } else {
            Self::Modern
        }
    }
}

fn preference_id() -> Id {
    Id::new("hark-lettering-v1")
}

/// The effective lettering: the saved choice, or the one the theme implies.
/// Nothing is ever written here at startup (LL-G
/// `eframe-set-theme-clobbers-persisted-preference`): a default written on
/// launch would turn "follows the theme" into a fixed choice. An unknown
/// saved key (a newer build's value) also falls back to the derived one.
pub fn lettering(ctx: &Context) -> Lettering {
    let saved = ctx.data_mut(|data| data.get_persisted::<String>(preference_id()));
    saved
        .and_then(|saved| Lettering::ALL.into_iter().find(|l| l.key() == saved))
        .unwrap_or_else(|| Lettering::derived(appearance(ctx)))
}

pub fn set_lettering(ctx: &Context, value: Lettering) {
    ctx.data_mut(|data| data.insert_persisted(preference_id(), value.key().to_owned()));
    install(ctx, value);
    ctx.request_repaint();
}

/// `set_fonts` rebuilds the font atlas, so callers install only on a change.
/// Both take effect on the next pass; nothing else in the style is reset.
pub(super) fn install(ctx: &Context, value: Lettering) {
    ctx.set_fonts(font_definitions(value));
    ctx.all_styles_mut(|style| style.text_styles = text_styles(value));
}

/// The Settings → Appearance checkbox. The top-bar picker stays theme-only.
pub fn lettering_toggle(ui: &mut Ui) {
    let mut wizard = lettering(ui.ctx()) == Lettering::Wizard;
    let response = ui.checkbox(&mut wizard, "Wizard lettering");
    focus_ring(ui, &response);
    if response.changed() {
        let value = if wizard {
            Lettering::Wizard
        } else {
            Lettering::Modern
        };
        set_lettering(ui.ctx(), value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::appearance::tests::MemoryStorage;

    fn proportional_lead(ctx: &Context) -> String {
        let mut output = ctx.run_ui(Default::default(), |_| {});
        output.textures_delta.clear();
        ctx.fonts(|f| f.definitions().families[&FontFamily::Proportional][0].clone())
    }

    #[test]
    fn with_no_choice_parchment_derives_wizard_and_every_other_theme_modern() {
        for value in Appearance::ALL {
            let ctx = Context::default();
            apply(&ctx);
            set_appearance(&ctx, value);
            let expected = if value == Appearance::Parchment {
                Lettering::Wizard
            } else {
                Lettering::Modern
            };
            assert_eq!(lettering(&ctx), expected, "{value:?}");
            assert_eq!(
                ctx.data_mut(|d| d.get_persisted::<String>(preference_id())),
                None,
                "{value:?}: a derived lettering must never be written"
            );
        }
    }

    #[test]
    fn choosing_parchment_with_no_saved_choice_swaps_the_fonts() {
        let ctx = Context::default();
        apply(&ctx);
        assert_eq!(proportional_lead(&ctx), "Inter");
        set_appearance(&ctx, Appearance::Parchment);
        assert_eq!(proportional_lead(&ctx), "Alegreya");
        assert_eq!(
            ctx.style_of(egui::Theme::Light).text_styles[&TextStyle::Heading].family,
            serif()
        );
        set_appearance(&ctx, Appearance::Light);
        assert_eq!(proportional_lead(&ctx), "Inter");
    }

    #[test]
    fn an_explicit_choice_survives_restart_and_theme_changes() {
        for (appearance, chosen) in [
            (Appearance::Parchment, Lettering::Modern),
            (Appearance::Dark, Lettering::Wizard),
        ] {
            let first = Context::default();
            apply(&first);
            set_appearance(&first, appearance);
            set_lettering(&first, chosen);
            let mut storage = MemoryStorage::default();
            first.memory(|memory| eframe::set_value(&mut storage, "egui", memory));

            let restored = Context::default();
            let memory = eframe::get_value(&storage, "egui").expect("serialized egui memory");
            restored.memory_mut(|current| *current = memory);
            apply(&restored);
            assert_eq!(lettering(&restored), chosen, "{appearance:?}");
            let lead = if chosen == Lettering::Wizard {
                "Alegreya"
            } else {
                "Inter"
            };
            assert_eq!(proportional_lead(&restored), lead, "{appearance:?}");

            for other in Appearance::ALL {
                set_appearance(&restored, other);
                assert_eq!(lettering(&restored), chosen, "{appearance:?} -> {other:?}");
            }
        }
    }

    #[test]
    fn an_unknown_saved_lettering_falls_back_to_the_derived_one() {
        for (value, expected) in [
            (Appearance::Parchment, Lettering::Wizard),
            (Appearance::Light, Lettering::Modern),
        ] {
            let ctx = Context::default();
            apply(&ctx);
            set_appearance(&ctx, value);
            ctx.data_mut(|d| d.insert_persisted(preference_id(), "future-lettering".to_owned()));
            apply(&ctx);
            assert_eq!(lettering(&ctx), expected, "{value:?}");
            assert_eq!(appearance(&ctx), value, "the theme must not be reset");
        }
    }
}

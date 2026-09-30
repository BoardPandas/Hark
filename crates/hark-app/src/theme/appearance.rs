//! egui persists its built-in Light/Dark/System choice. Store the palette
//! separately so Solarized survives restarts and old preferences still work.
use super::*;
use egui::{Id, Theme, ThemePreference};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Appearance {
    System,
    Light,
    Dark,
    SolarizedLight,
    SolarizedDark,
}

impl Appearance {
    pub const ALL: [Self; 5] = [
        Self::System,
        Self::Light,
        Self::Dark,
        Self::SolarizedLight,
        Self::SolarizedDark,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
            Self::SolarizedLight => "Solarized Light",
            Self::SolarizedDark => "Solarized Dark",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
            Self::SolarizedLight => "solarized-light",
            Self::SolarizedDark => "solarized-dark",
        }
    }

    fn preference(self) -> ThemePreference {
        match self {
            Self::System => ThemePreference::System,
            Self::Light | Self::SolarizedLight => ThemePreference::Light,
            Self::Dark | Self::SolarizedDark => ThemePreference::Dark,
        }
    }
}

fn preference_id() -> Id {
    Id::new("hark-appearance-v1")
}

pub fn appearance(ctx: &Context) -> Appearance {
    let saved = ctx.data_mut(|data| data.get_persisted::<String>(preference_id()));
    if let Some(value) = saved.and_then(|saved| {
        Appearance::ALL
            .into_iter()
            .find(|value| value.key() == saved)
    }) {
        return value;
    }
    // Older Hark versions only saved this built-in preference. Also handles
    // an unknown future palette key without silently resetting to System.
    match ctx.options(|o| o.theme_preference) {
        ThemePreference::System => Appearance::System,
        ThemePreference::Light => Appearance::Light,
        ThemePreference::Dark => Appearance::Dark,
    }
}

pub fn set_appearance(ctx: &Context, value: Appearance) {
    ctx.data_mut(|data| data.insert_persisted(preference_id(), value.key().to_owned()));
    install(ctx, value);
    ctx.request_repaint();
}

pub(super) fn install(ctx: &Context, value: Appearance) {
    let solarized = matches!(
        value,
        Appearance::SolarizedLight | Appearance::SolarizedDark
    );
    // Reset both variants, so returning from Solarized to System cannot
    // leave a Solarized palette behind after the OS changes appearance.
    let (light, dark) = if solarized {
        (&palette::SOLARIZED_LIGHT, &palette::SOLARIZED_DARK)
    } else {
        (&LIGHT, &DARK)
    };
    ctx.set_visuals_of(Theme::Light, palette::visuals(light));
    ctx.set_visuals_of(Theme::Dark, palette::visuals(dark));
    ctx.set_theme(value.preference());
}

/// Compact native, keyboard-accessible picker shared by shell and Settings.
pub fn appearance_picker(ui: &mut Ui) {
    let mut selected = appearance(ui.ctx());
    let previous = selected;
    egui::ComboBox::from_id_salt("appearance-picker")
        .selected_text(selected.label())
        .width(SETTINGS_NAV_WIDTH)
        .show_ui(ui, |ui| {
            for value in Appearance::ALL {
                ui.selectable_value(&mut selected, value, value.label());
            }
        });
    if selected != previous {
        super::set_appearance(ui.ctx(), selected);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct MemoryStorage(HashMap<String, String>);

    impl eframe::Storage for MemoryStorage {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.to_owned(), value);
        }
        fn remove_string(&mut self, key: &str) {
            self.0.remove(key);
        }
        fn flush(&mut self) {}
    }

    #[test]
    fn every_appearance_survives_eframe_memory_serialization_and_startup() {
        for selected in Appearance::ALL {
            let first = Context::default();
            apply(&first);
            set_appearance(&first, selected);
            let mut storage = MemoryStorage::default();
            first.memory(|memory| eframe::set_value(&mut storage, "egui", memory));

            let restored = Context::default();
            let memory = eframe::get_value(&storage, "egui").expect("serialized egui memory");
            restored.memory_mut(|current| *current = memory);
            apply(&restored);

            assert_eq!(appearance(&restored), selected);
            assert_eq!(
                restored.options(|o| o.theme_preference),
                selected.preference()
            );
            for variant in [Theme::Light, Theme::Dark] {
                assert_eq!(
                    restored.style_of(variant).visuals.panel_fill,
                    first.style_of(variant).visuals.panel_fill,
                    "{selected:?} / {variant:?}"
                );
            }
        }
    }

    #[test]
    fn legacy_and_unknown_palette_preferences_keep_the_existing_choice() {
        for preference in [
            ThemePreference::System,
            ThemePreference::Light,
            ThemePreference::Dark,
        ] {
            for saved in [None, Some("future-unknown-palette")] {
                let ctx = Context::default();
                ctx.set_theme(preference);
                if let Some(saved) = saved {
                    ctx.data_mut(|data| data.insert_persisted(preference_id(), saved.to_owned()));
                }
                apply(&ctx);
                assert_eq!(ctx.options(|o| o.theme_preference), preference);
            }
        }
    }

    #[test]
    fn switching_back_to_system_restores_both_neutral_palettes() {
        let ctx = Context::default();
        apply(&ctx);
        for solarized in [Appearance::SolarizedDark, Appearance::SolarizedLight] {
            set_appearance(&ctx, solarized);
            set_appearance(&ctx, Appearance::System);
            assert_eq!(ctx.style_of(Theme::Light).visuals.panel_fill, LIGHT.window);
            assert_eq!(ctx.style_of(Theme::Dark).visuals.panel_fill, DARK.window);
            assert_eq!(ctx.options(|o| o.theme_preference), ThemePreference::System);
        }
    }
}

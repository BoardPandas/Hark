//! Behavior, appearance, and history/privacy preferences.
use super::form::subhead;
use egui::{DragValue, RichText, Ui};
use hark_config::Settings;

pub fn behavior_section(ui: &mut Ui, draft: &mut Settings) {
    ui.vertical(|ui| {
        subhead(ui, "Behavior");
        ui.horizontal(|ui| {
            ui.label("Skip cleanup below");
            ui.add(DragValue::new(&mut draft.voice.skip_below_words).range(0..=50));
            ui.label("words");
        });
        ui.label(
            RichText::new("Short dictations stay verbatim; 0 sends everything to cleanup.")
                .small()
                .weak(),
        );

        ui.add_space(4.0);
        // The range starts at 1.0, so fully disabling the guard stays a
        // config-file edit (`max_expansion_ratio = 0`); the slider cannot
        // reach a value config validation would reject.
        ui.horizontal(|ui| {
            ui.label("Reject cleanup longer than");
            ui.add(
                DragValue::new(&mut draft.voice.max_expansion_ratio)
                    .range(1.0..=5.0)
                    .speed(0.05)
                    .fixed_decimals(2),
            );
            ui.label("x what you said");
        });
        ui.label(
            RichText::new(
                "Keeps a voice from turning a short remark into a paragraph: over the \
                     limit, your uncleaned words are injected instead.",
            )
            .small()
            .weak(),
        );

        ui.add_space(4.0);
        ui.checkbox(
            &mut draft.output.strip_single_word_period,
            "Drop the trailing period on single words",
        );
        ui.label(
            RichText::new(
                "When you dictate just one word, inject it without the trailing period a \
                     provider or cleanup voice adds.",
            )
            .small()
            .weak(),
        );

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Theme");
            let mut preference = ui.ctx().options(|o| o.theme_preference);
            let mut changed = false;
            for (value, label) in [
                (egui::ThemePreference::System, "System"),
                (egui::ThemePreference::Light, "Light"),
                (egui::ThemePreference::Dark, "Dark"),
            ] {
                changed |= ui.radio_value(&mut preference, value, label).changed();
            }
            if changed {
                // Persists via egui memory (eframe `persistence`), not
                // config.toml; theme::apply preserves it on relaunch.
                ui.ctx().set_theme(preference);
            }
        });

        ui.add_space(4.0);
        // The registry reconcile happens on Save (settings::mod::save), so
        // the checkbox only edits the draft here, like every other field.
        ui.checkbox(&mut draft.startup.launch_at_login, "Launch Hark at login");
        ui.label(
            RichText::new(if cfg!(target_os = "linux") {
                // An XDG autostart entry runs when the desktop session
                // starts, which is not the same moment as signing in on
                // Windows and is worth being accurate about.
                "Starts hidden in the system tray when your desktop session starts."
            } else {
                "Starts hidden in the system tray when you sign in."
            })
            .small()
            .weak(),
        );
    });
}

pub fn privacy_section(ui: &mut Ui, draft: &mut Settings) {
    ui.vertical(|ui| {
        subhead(ui, "History & privacy");
        ui.checkbox(
            &mut draft.history.capture,
            "Save dictation history on this device",
        );
        ui.label(
            RichText::new(
                "Off: no transcript content is stored; the lifetime counters still tick.",
            )
            .small()
            .weak(),
        );
        ui.horizontal(|ui| {
            ui.label("Keep at most");
            ui.add(DragValue::new(&mut draft.history.max_entries).range(1..=100_000));
            ui.label("entries, for");
            ui.add(DragValue::new(&mut draft.history.max_age_days).range(1..=3_650));
            ui.label("days");
        });
        ui.add_space(6.0);
        ui.label(
            RichText::new(
                "Cloud transcription sends audio to your chosen provider; primary on-device \
                     transcription keeps audio here. Non-Verbatim cleanup sends text to your \
                     cleanup provider. History and stats stay here. Spellbook terms may be sent \
                     to your provider as accuracy hints.",
            )
            .small()
            .weak(),
        );
    });
}

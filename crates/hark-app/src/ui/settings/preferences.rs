//! General, dictation behavior, and history/privacy preferences.
use super::form::subhead;
use egui::{DragValue, RichText, Ui};
use hark_config::Settings;

/// Returns true for an explicit exit, independently of the draft preferences.
pub fn general_section(ui: &mut Ui, draft: &mut Settings) -> bool {
    subhead(ui, "General");
    ui.checkbox(&mut draft.startup.launch_at_login, "Launch Hark at startup");
    ui.label(
        RichText::new(if cfg!(target_os = "linux") {
            "Starts hidden in the system tray when your desktop session starts."
        } else {
            "Starts hidden in the system tray when you sign in."
        })
        .small()
        .weak(),
    );

    ui.add_space(crate::theme::GAP);
    ui.checkbox(&mut draft.general.always_on_top, "Always on top");
    ui.label(
        RichText::new(if cfg!(target_os = "linux") {
            "Keep the Hark window above other windows (unavailable on Wayland)."
        } else {
            "Keep the Hark window above other windows."
        })
        .small()
        .weak(),
    );

    ui.add_space(crate::theme::GAP);
    ui.checkbox(
        &mut draft.general.exit_on_close,
        "Exit when the window is closed",
    );
    ui.label(
        RichText::new("On: the X exits Hark. Off: the X hides Hark in the system tray.")
            .small()
            .weak(),
    );

    ui.add_space(crate::theme::SECTION_GAP);
    appearance_section(ui);

    ui.add_space(crate::theme::SECTION_GAP);
    ui.separator();
    ui.add_space(crate::theme::GAP);
    let close = ui.button("Close Program").clicked();
    ui.label(
        RichText::new("Fully exit Hark and stop dictation. Unsaved settings will be discarded.")
            .small()
            .weak(),
    );
    close
}

fn appearance_section(ui: &mut Ui) {
    subhead(ui, "Appearance");
    ui.horizontal(|ui| {
        ui.label("Theme");
        crate::theme::appearance_picker(ui);
    });
    ui.label(
        RichText::new("Applies immediately and is remembered on this device. System follows your desktop's light or dark appearance.")
            .small()
            .weak(),
    );
}

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
                "Off: no transcript content is stored. Numeric Insights keep up to 366 days; \
                 lifetime totals continue. Clear history keeps these stats; Reset stats removes them.",
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
        ui.add_space(crate::theme::ROW_GAP);
        ui.checkbox(
            &mut draft.insights.track_apps,
            "Include app names in local Insights",
        );
        ui.label(RichText::new(
            "Only the app name when a dictation starts. No window or document titles, \
             no continuous tracking. Best effort on Windows, macOS and X11; unavailable on Wayland. \
             Turning this off stops future collection; Reset stats removes stored app counts."
        ).small().weak());
        ui.checkbox(
            &mut draft.insights.analyze_text,
            "Analyze retained history for words and phrases",
        );
        ui.label(RichText::new(
            "Opt-in analysis runs only on this device and uses retained transcripts. \
             Nothing is sent to a provider and no extra copy of the text is stored."
        ).small().weak());
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

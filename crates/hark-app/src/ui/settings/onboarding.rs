//! Guided setup uses the same real controls as Settings.
use super::{
    get_started::{self, Step},
    local, Section, SettingsPage,
};
use crate::{pipeline::PipelineController, theme};
use egui::{RichText, Ui};
use hark_config::{LocalMode, Settings, VoiceName};

impl SettingsPage {
    pub(super) fn onboarding(
        &mut self,
        ui: &mut Ui,
        saved: &mut Settings,
        pipeline: &mut PipelineController,
    ) -> bool {
        if !self.get_started.visible(pipeline.injected_count() > 0) {
            return false;
        }
        egui::ScrollArea::vertical()
            .id_salt("first-run")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(theme::SHADOW_MARGIN)
                    .show(ui, |ui| {
                        theme::card(ui, |ui| {
                            self.setup_progress(ui);
                            ui.add_space(theme::SECTION_GAP);
                            match self.get_started.step {
                                Step::Choose => self.choose_engine(ui),
                                Step::Configure => self.configure_first_run(ui, saved, pipeline),
                                Step::Permissions => self.permissions_step(ui),
                                Step::Try => self.try_step(ui, saved, pipeline),
                            }
                            ui.add_space(theme::SECTION_GAP);
                            if ui
                                .link(RichText::new("Skip for now").small().weak())
                                .clicked()
                            {
                                self.get_started.dismissed = true;
                            }
                        });
                    });
            });
        true
    }

    fn setup_progress(&self, ui: &mut Ui) {
        let active = match self.get_started.step {
            Step::Choose | Step::Configure => 0,
            Step::Permissions => 1,
            Step::Try => 2,
        };
        ui.horizontal_wrapped(|ui| {
            for (index, label) in ["Transcription", "Permissions", "Try it"]
                .into_iter()
                .enumerate()
            {
                let color = if index == active {
                    theme::accent(ui.visuals())
                } else {
                    ui.visuals().weak_text_color()
                };
                ui.label(
                    RichText::new(format!("{}  {label}", index + 1))
                        .small()
                        .color(color),
                );
                ui.add_space(theme::GAP);
            }
        });
    }

    fn permissions_step(&mut self, ui: &mut Ui) {
        ui.heading("Let Hark hear you.");
        ui.label(
            RichText::new(
                "Microphone and keyboard access let you dictate into the apps you already use.",
            )
            .weak(),
        );
        ui.add_space(theme::ROW_GAP);
        permission_guidance(ui);
        ui.add_space(theme::SECTION_GAP);
        ui.horizontal(|ui| {
            if ui.button("Back").clicked() {
                self.get_started.step = Step::Configure;
            }
            if ui
                .add(theme::primary_button(ui.visuals(), "Try a dictation"))
                .clicked()
            {
                self.get_started.step = Step::Try;
            }
        });
    }

    fn try_step(&mut self, ui: &mut Ui, saved: &Settings, pipeline: &mut PipelineController) {
        ui.heading("You've got something to say.");
        ui.label(format!(
            "Hold {}, say a sentence, then release.",
            hark_hotkey::pretty_chord(&saved.hotkey.ptt_key)
        ));
        ui.add_space(theme::SECTION_GAP);
        ui.label(RichText::new("Open a text field in another app to try it. Hark inserts your words at your cursor; this guide closes after your first successful dictation.").weak());
        ui.add_space(theme::ROW_GAP);
        if !pipeline.is_running() {
            ui.label(RichText::new("Dictation isn't ready yet. Check the status below, or return to configuration.").color(theme::warning(ui.visuals())));
            if ui.button("Retry after granting access").clicked() {
                pipeline.start(saved, ui.ctx());
            }
        }
        ui.horizontal(|ui| {
            if ui.button("Back to setup").clicked() {
                self.get_started.step = Step::Configure;
            }
            if ui.button("Change shortcut").clicked() {
                self.open(Section::Audio);
            }
        });
    }

    fn choose_engine(&mut self, ui: &mut Ui) {
        ui.heading("A good place to start.");
        ui.label(
            RichText::new("Choose how your words become text. You can change this anytime.").weak(),
        );
        ui.add_space(theme::SECTION_GAP);
        let local_available = hark_local_stt::LocalEngine::is_available();
        for (local, icon, title, detail) in [
            (
                false,
                theme::icons::KEY,
                "Use a cloud provider",
                "Bring your own API key. Audio is sent to your chosen provider.",
            ),
            (
                true,
                theme::icons::WAVEFORM,
                "Keep transcription on-device",
                "No key needed. Download Parakeet once, then dictate offline.",
            ),
        ] {
            ui.add_enabled_ui(!local || local_available, |ui| {
                let label = theme::icon_label_job(ui.style(), icon, title);
                if theme::nav_button(ui, label, false).clicked() {
                    self.get_started.local = local;
                    self.get_started.step = Step::Configure;
                    self.draft.local_stt.mode = if local {
                        LocalMode::Primary
                    } else {
                        LocalMode::Off
                    };
                    if local {
                        self.draft.voice.default = VoiceName::Verbatim;
                    }
                }
                ui.label(RichText::new(detail).small().weak());
            });
            ui.add_space(theme::ROW_GAP);
        }
        if !local_available {
            ui.label(
                RichText::new("This build does not include the on-device engine.")
                    .small()
                    .weak(),
            );
        }
        ui.label(
            RichText::new("Your dictation history stays on this device. Hark operates no servers.")
                .small()
                .weak(),
        );
    }

    fn configure_first_run(
        &mut self,
        ui: &mut Ui,
        saved: &mut Settings,
        pipeline: &mut PipelineController,
    ) {
        if self.get_started.local {
            ui.heading("A little download. A lot of freedom.");
            ui.label(RichText::new("Your audio stays on this computer. This setup uses Verbatim voice for offline dictation.").weak());
            ui.add_space(theme::ROW_GAP);
            local::card(ui, &mut self.draft, &mut self.download);
        } else {
            ui.heading("Connect your speech provider.");
            self.dictation_section(ui, saved, pipeline);
        }
        ui.add_space(theme::SECTION_GAP);
        let ready = get_started::ready(
            self.get_started.local,
            self.download.status().is_ready(),
            self.test.passed_for(&self.draft),
        );
        ui.horizontal(|ui| {
            if ui.button("Back").clicked() {
                self.get_started.step = Step::Choose;
            }
            if ui
                .add_enabled(
                    ready,
                    theme::primary_button(ui.visuals(), "Save & continue"),
                )
                .clicked()
            {
                self.save(saved, pipeline, ui.ctx());
                // Saving can succeed even when the OS refuses mic/hook access.
                // The next step explains permissions without a false success badge.
                if self.draft == *saved {
                    self.get_started.step = Step::Permissions;
                }
            }
        });
        if !ready {
            ui.label(
                RichText::new(if self.get_started.local {
                    "Finish downloading the model to continue."
                } else {
                    "Store your key and run Test connection to continue."
                })
                .small()
                .weak(),
            );
        }
    }
}

fn permission_guidance(ui: &mut Ui) {
    ui.label(RichText::new("Microphone").text_style(theme::subheading()));
    ui.label(
        RichText::new(
            "Allow Hark to capture audio. Your shortcut controls when speech is transcribed.",
        )
        .small()
        .weak(),
    );
    #[cfg(windows)]
    if ui.button("Open microphone settings").clicked() {
        ui.ctx()
            .open_url(egui::OpenUrl::new_tab("ms-settings:privacy-microphone"));
    }
    #[cfg(target_os = "macos")]
    {
        if ui.button("Open microphone settings").clicked() {
            ui.ctx().open_url(egui::OpenUrl::new_tab(
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone",
            ));
        }
        ui.add_space(theme::ROW_GAP);
        ui.label(RichText::new("Keyboard & text insertion").text_style(theme::subheading()));
        if ui.button("Open Accessibility settings").clicked() {
            ui.ctx().open_url(egui::OpenUrl::new_tab(
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
            ));
        }
    }
    #[cfg(target_os = "linux")]
    {
        ui.label(
            RichText::new("Choose your microphone in your desktop's sound settings.")
                .small()
                .weak(),
        );
        ui.add_space(theme::ROW_GAP);
        ui.label(RichText::new("Keyboard & text insertion").text_style(theme::subheading()));
        ui.label("Hark needs membership in the input group to read your shortcut and insert text. Run this in a terminal, then sign out and back in:");
        ui.code("sudo usermod -aG input $USER");
    }
    #[cfg(windows)]
    {
        ui.add_space(theme::ROW_GAP);
        ui.label(RichText::new("Keyboard & text insertion").text_style(theme::subheading()));
        ui.label(RichText::new("Your shortcut works across normal desktop apps. Windows prevents insertion into apps running with higher privileges.").small().weak());
    }
}

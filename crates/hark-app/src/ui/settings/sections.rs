//! Settings navigation and grouped surfaces. The draft belongs to SettingsPage,
//! so moving between sections never discards a field or re-seeds its buffer.

use super::{capture, cleanup, form, hotkey, local, preferences, updates, SettingsPage};
use crate::{pipeline::PipelineController, theme, update::Updater};
use egui::{RichText, Ui};
use hark_config::Settings;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Section {
    #[default]
    General,
    Dictation,
    Audio,
    OnDevice,
    Behavior,
    Privacy,
    Updates,
}

impl Section {
    pub const ALL: [Self; 7] = [
        Self::General,
        Self::Dictation,
        Self::Audio,
        Self::OnDevice,
        Self::Behavior,
        Self::Privacy,
        Self::Updates,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Dictation => "Dictation",
            Self::Audio => "Audio & shortcut",
            Self::OnDevice => "On-device",
            Self::Behavior => "Behavior",
            Self::Privacy => "Privacy",
            Self::Updates => "Updates",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::General => theme::icons::GEAR,
            Self::Dictation => theme::icons::WAVEFORM,
            Self::Audio => theme::icons::MICROPHONE,
            Self::OnDevice => theme::icons::WAVEFORM,
            Self::Behavior => theme::icons::GEAR,
            Self::Privacy => theme::icons::KEY,
            Self::Updates => theme::icons::ARROW_UP,
        }
    }
}

impl SettingsPage {
    pub fn open(&mut self, section: Section) {
        self.section = section;
        self.get_started.dismissed = true;
    }

    pub fn leave(&mut self, pipeline: &mut PipelineController) {
        self.hotkey.cancel(pipeline);
    }

    pub(super) fn navigation(&mut self, ui: &mut Ui, pipeline: &mut PipelineController) {
        let previous = self.section;
        for section in Section::ALL {
            let label = theme::icon_label_job(ui.style(), section.icon(), section.label());
            if theme::nav_button(ui, label, self.section == section).clicked() {
                self.section = section;
            }
        }
        if previous == Section::Audio && self.section != previous {
            self.leave(pipeline);
        }
    }

    pub(super) fn section_body(
        &mut self,
        ui: &mut Ui,
        saved: &mut Settings,
        pipeline: &mut PipelineController,
        updater: &mut Updater,
    ) {
        egui::ScrollArea::vertical()
            .id_salt(("settings-section", self.section))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Shadows need a little room inside the scroller's clip rect.
                egui::Frame::new()
                    .inner_margin(theme::SHADOW_MARGIN)
                    .show(ui, |ui| match self.section {
                        Section::General => {
                            theme::card(ui, |ui| {
                                self.close_requested |=
                                    preferences::general_section(ui, &mut self.draft);
                            });
                        }
                        Section::Dictation => self.dictation_section(ui, saved, pipeline),
                        Section::Audio => self.audio_section(ui, pipeline),
                        Section::OnDevice => {
                            local::section(ui, &mut self.draft, &mut self.download)
                        }
                        Section::Behavior => {
                            theme::card(ui, |ui| {
                                preferences::behavior_section(ui, &mut self.draft)
                            });
                        }
                        Section::Privacy => {
                            theme::card(ui, |ui| preferences::privacy_section(ui, &mut self.draft));
                        }
                        Section::Updates => {
                            theme::card(ui, |ui| updates::section(ui, updater, &mut self.draft));
                        }
                    });
            });
    }

    pub(super) fn dictation_section(
        &mut self,
        ui: &mut Ui,
        saved: &mut Settings,
        pipeline: &mut PipelineController,
    ) {
        theme::card(ui, |ui| {
            if form::provider_section(ui, &mut self.draft, &mut self.bufs) {
                self.stt_keys.sync_account(self.draft.provider.kind.label());
                self.test.reset();
            }
            ui.add_space(theme::GAP);
            if self.stt_keys.show(ui) {
                self.test.reset();
                if self.draft.provider.kind == saved.provider.kind {
                    pipeline.start(saved, ui.ctx());
                }
            }
            ui.add_space(theme::GAP);
            self.test.show(ui, &self.draft);
            ui.add_space(theme::GAP);
            form::model_endpoint_section(ui, &mut self.draft, &mut self.bufs);
        });
        ui.add_space(theme::SECTION_GAP);
        theme::card(ui, |ui| {
            form::voice_section(ui, &mut self.draft, &mut self.voice_secret_clicks);
            ui.add_space(theme::GAP);
            cleanup::section(
                ui,
                &mut self.draft,
                &mut self.bufs,
                &mut self.cleanup_keys,
                &mut self.cleanup_test,
            );
        });
        ui.add_space(theme::GAP);
        ui.label(RichText::new("Cloud transcription sends audio to your chosen provider. Non-Verbatim voices may also send text to your cleanup provider.").small().weak());
    }

    pub(super) fn audio_section(&mut self, ui: &mut Ui, pipeline: &mut PipelineController) {
        theme::card(ui, |ui| {
            if matches!(
                hotkey::section(ui, &mut self.draft, &mut self.hotkey, pipeline),
                capture::HotkeyAction::StartRequested
            ) {
                self.hotkey.begin(ui.ctx(), pipeline);
            }
        });
        ui.add_space(theme::SECTION_GAP);
        theme::card(ui, |ui| {
            let level = pipeline.level_meter().map(|m| m.level());
            if form::mic_section(
                ui,
                &mut self.draft,
                &self.mic_devices,
                level,
                self.comms_default.as_deref(),
            ) {
                self.mic_devices = hark_audio::list_input_devices();
                self.comms_default = hark_audio::communications_default_device();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_settings_section_has_a_unique_scroll_identity() {
        let ids: std::collections::HashSet<_> = Section::ALL
            .into_iter()
            .map(|section| egui::Id::new(("settings-section", section)))
            .collect();
        assert_eq!(ids.len(), Section::ALL.len());
    }
}

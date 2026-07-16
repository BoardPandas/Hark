//! The settings form sections (spec §3.6, §3.11). Progressive disclosure:
//! provider row, key section, hotkey, and voice stay visible; everything
//! else lives in `CollapsingHeader`s. Buffers sync into the draft at
//! widget-render time, so the draft is always current when Save reads it.

use crate::theme;
use egui::{CollapsingHeader, DragValue, RichText, TextEdit, Ui};
use hark_config::{Provider, ProviderKind, Settings, VoiceName};

/// String buffers behind optional config fields (empty = unset) plus the
/// one-shot flag that force-opens Model & endpoint for custom endpoints.
pub struct FormBufs {
    pub model: String,
    pub base_url: String,
    pub cleanup_model: String,
    pub cleanup_base_url: String,
    pub cleanup_key_account: String,
    force_endpoint_open: bool,
}

impl FormBufs {
    pub fn from_settings(settings: &Settings) -> Self {
        let voice_provider = settings.voice.provider.as_ref();
        FormBufs {
            model: settings.provider.model.clone().unwrap_or_default(),
            base_url: settings.provider.base_url.clone().unwrap_or_default(),
            cleanup_model: voice_provider
                .and_then(|p| p.model.clone())
                .unwrap_or_default(),
            cleanup_base_url: voice_provider
                .and_then(|p| p.base_url.clone())
                .unwrap_or_default(),
            cleanup_key_account: voice_provider
                .and_then(|p| p.key_account.clone())
                .unwrap_or_default(),
            force_endpoint_open: false,
        }
    }
}

/// Empty or whitespace input means "unset, use the default".
pub fn none_if_empty(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub(super) fn inline_error(ui: &mut Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(theme::icons::WARNING).color(theme::DANGER));
        ui.label(RichText::new(text).small());
    });
}

fn subhead(ui: &mut Ui, text: &str) {
    ui.add_space(8.0);
    ui.label(RichText::new(text).text_style(theme::subheading()));
}

/// Provider kind radio row. Returns true when the kind changed.
pub fn provider_section(ui: &mut Ui, draft: &mut Settings, bufs: &mut FormBufs) -> bool {
    subhead(ui, "Speech to text");
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        for (kind, label) in [
            (ProviderKind::Deepgram, "Deepgram"),
            (ProviderKind::Openai, "OpenAI"),
            (ProviderKind::Groq, "Groq"),
            (ProviderKind::OpenaiCompatible, "OpenAI-compatible"),
        ] {
            if ui
                .radio_value(&mut draft.provider.kind, kind, label)
                .changed()
            {
                changed = true;
            }
        }
    });
    if changed {
        // A model or URL typed for the previous provider almost certainly
        // does not exist on the new one; fall back to the per-kind default.
        bufs.model.clear();
        bufs.base_url.clear();
        draft.provider.model = None;
        draft.provider.base_url = None;
        if draft.provider.kind == ProviderKind::OpenaiCompatible {
            bufs.force_endpoint_open = true;
        }
    }
    changed
}

/// Collapsed by default; auto-opened when a custom endpoint needs its URL.
pub fn model_endpoint_section(ui: &mut Ui, draft: &mut Settings, bufs: &mut FormBufs) {
    let force_open = bufs.force_endpoint_open.then_some(true);
    bufs.force_endpoint_open = false;
    let custom = draft.provider.kind == ProviderKind::OpenaiCompatible;
    CollapsingHeader::new("Model & endpoint")
        .default_open(custom)
        .open(force_open)
        .show(ui, |ui| {
            let default_model = Provider {
                kind: draft.provider.kind,
                base_url: None,
                model: None,
            }
            .resolved_model();
            ui.label(RichText::new("Model").weak());
            ui.add(
                TextEdit::singleline(&mut bufs.model)
                    .hint_text(format!("{default_model} (default)")),
            );
            draft.provider.model = none_if_empty(&bufs.model);

            ui.label(RichText::new("Base URL").weak());
            let hint = if custom {
                "https://host/v1 (required)".to_string()
            } else {
                Provider {
                    kind: draft.provider.kind,
                    base_url: None,
                    model: None,
                }
                .resolved_base_url()
                .map(|url| format!("{url} (default)"))
                .unwrap_or_default()
            };
            ui.add(TextEdit::singleline(&mut bufs.base_url).hint_text(hint));
            draft.provider.base_url = none_if_empty(&bufs.base_url);
            if custom && draft.provider.base_url.is_none() {
                inline_error(ui, "OpenAI-compatible endpoints need a base URL.");
            }
        });
}

pub fn hotkey_section(ui: &mut Ui, draft: &mut Settings) {
    subhead(ui, "Push-to-talk");
    ui.add(
        TextEdit::singleline(&mut draft.hotkey.ptt_key)
            .hint_text("LCtrl+LWin")
            .desired_width(200.0),
    );
    match hark_hotkey::PttChord::parse(&draft.hotkey.ptt_key) {
        Ok(_) => {
            ui.label(
                RichText::new("Hold these keys together to dictate; release to inject.")
                    .small()
                    .weak(),
            );
        }
        Err(e) => inline_error(ui, &e.to_string()),
    }
}

pub fn voice_section(ui: &mut Ui, draft: &mut Settings) {
    subhead(ui, "Voice");
    egui::ComboBox::from_id_salt("voice-picker")
        .selected_text(voice_display(draft.voice.default))
        .show_ui(ui, |ui| {
            for name in [
                VoiceName::Verbatim,
                VoiceName::Clean,
                VoiceName::Professional,
                VoiceName::Casual,
                VoiceName::Custom,
            ] {
                ui.selectable_value(&mut draft.voice.default, name, voice_display(name));
            }
        });
    if draft.voice.default == VoiceName::Custom {
        ui.label(RichText::new("Custom prompt").weak());
        ui.add(
            TextEdit::multiline(&mut draft.voice.custom_prompt)
                .desired_rows(3)
                .hint_text("Rewrite the transcript as..."),
        );
        if draft.voice.custom_prompt.trim().is_empty() {
            inline_error(ui, "The custom voice needs a prompt.");
        }
    }
}

fn voice_display(name: VoiceName) -> &'static str {
    match name {
        VoiceName::Verbatim => "Verbatim",
        VoiceName::Clean => "Clean",
        VoiceName::Professional => "Professional",
        VoiceName::Casual => "Casual",
        VoiceName::Custom => "Custom",
    }
}

pub fn behavior_section(ui: &mut Ui, draft: &mut Settings) {
    CollapsingHeader::new("Behavior")
        .default_open(false)
        .show(ui, |ui| {
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
        });
}

pub fn privacy_section(ui: &mut Ui, draft: &mut Settings) {
    CollapsingHeader::new("History & privacy")
        .default_open(false)
        .show(ui, |ui| {
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
                    "Audio goes to your STT provider on every dictation. Text goes to \
                     your cleanup provider on non-Verbatim voices. History, stats, and \
                     the dictionary never leave this device.",
                )
                .small()
                .weak(),
            );
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_whitespace_inputs_mean_unset() {
        assert_eq!(none_if_empty(""), None);
        assert_eq!(none_if_empty("   "), None);
        assert_eq!(none_if_empty(" nova-3 "), Some("nova-3".to_string()));
    }

    #[test]
    fn buffers_seed_from_existing_settings() {
        let settings = Settings::from_toml(
            "[provider]\nkind = \"openai-compatible\"\nbase_url = \"https://llm.local/v1\"\n\
             model = \"whisper-1\"",
        )
        .unwrap();
        let bufs = FormBufs::from_settings(&settings);
        assert_eq!(bufs.model, "whisper-1");
        assert_eq!(bufs.base_url, "https://llm.local/v1");
        assert_eq!(bufs.cleanup_model, "");
    }

    #[test]
    fn every_voice_has_a_display_name_matching_its_config_label() {
        for name in [
            VoiceName::Verbatim,
            VoiceName::Clean,
            VoiceName::Professional,
            VoiceName::Casual,
            VoiceName::Custom,
        ] {
            assert_eq!(voice_display(name).to_lowercase(), name.label());
        }
    }
}

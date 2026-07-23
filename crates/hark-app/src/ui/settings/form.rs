//! The settings form sections (spec §3.6, §3.11). Progressive disclosure:
//! provider row, key section, hotkey, and voice stay visible; everything
//! else lives in `CollapsingHeader`s. Buffers sync into the draft at
//! widget-render time, so the draft is always current when Save reads it.

use super::capture::{CaptureTransition, HotkeyCapture};
use crate::theme;
use egui::{CollapsingHeader, Context, DragValue, RichText, TextEdit, Ui};
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

/// Shown in the picker for "use whatever the OS picks".
const DEFAULT_MIC_LABEL: &str = "System default";

/// Microphone picker. `devices` is the list enumerated off the UI thread and
/// cached by the page; `on_refresh` is set true when the user asks to re-scan
/// (a mic plugged in after the page opened). The chosen device applies on the
/// next Save, like every other field here.
pub fn mic_section(
    ui: &mut Ui,
    draft: &mut Settings,
    devices: &[String],
    level: Option<f32>,
    comms_default: Option<&str>,
) -> bool {
    subhead(ui, "Microphone");
    let selected = draft.audio.input_device.clone();
    let selected_text = selected.as_deref().unwrap_or(DEFAULT_MIC_LABEL);
    let mut refresh = false;
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("mic-picker")
            .selected_text(selected_text)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut draft.audio.input_device, None, DEFAULT_MIC_LABEL);
                for name in devices {
                    // Windows keeps a second capture default for communications
                    // apps, and cpal never asks for it. If the user set their
                    // headset there — every headset guide says to — Teams gets
                    // the headset and Hark gets the built-in array mic. Naming
                    // it turns an invisible mismatch into an obvious one.
                    let label = if comms_default == Some(name.as_str()) {
                        format!("{name}  — used by Teams/Zoom")
                    } else {
                        name.clone()
                    };
                    ui.selectable_value(&mut draft.audio.input_device, Some(name.clone()), label);
                }
                // A configured device that is not currently enumerated (mic
                // unplugged) stays selectable so opening the picker cannot
                // silently reset the choice; capture falls back to the
                // default until it returns.
                if let Some(name) = &selected {
                    if !devices.iter().any(|d| d == name) {
                        ui.selectable_value(
                            &mut draft.audio.input_device,
                            Some(name.clone()),
                            format!("{name} (not connected)"),
                        );
                    }
                }
            });
        if ui
            .button("Rescan")
            .on_hover_text("Re-scan for microphones plugged in since this page opened")
            .clicked()
        {
            refresh = true;
        }
    });
    ui.label(
        RichText::new("Falls back to the system default if the chosen microphone is unavailable.")
            .small()
            .weak(),
    );
    if let Some(peak) = level {
        input_meter(ui, peak);
    }
    refresh
}

/// How loud the live input is, in the terms a user can act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputLevel {
    /// Nothing arriving at all: wrong device, or muted.
    Silent,
    /// Arriving, but too quiet to transcribe reliably.
    TooQuiet,
    Good,
    /// Loud enough to clip, which destroys consonants.
    Hot,
}

/// Classify a peak amplitude (0..=1) into a band. Pure: the thresholds are the
/// whole content of the meter, so they are worth testing directly.
pub fn classify_level(peak: f32) -> InputLevel {
    match peak {
        p if p < 0.005 => InputLevel::Silent,
        p if p < 0.05 => InputLevel::TooQuiet,
        p if p < 0.95 => InputLevel::Good,
        _ => InputLevel::Hot,
    }
}

/// A live input meter under the picker.
///
/// This exists so "Hark can't hear me" becomes something the user can see
/// rather than guess at: a bar that does not move names the wrong device, and
/// one that barely moves names a level problem. Without it the only feedback is
/// dictation silently doing nothing, which is indistinguishable from a bug.
fn input_meter(ui: &mut Ui, peak: f32) {
    let band = classify_level(peak);
    let (color, note) = match band {
        InputLevel::Silent => (
            ui.visuals().weak_text_color(),
            "No input — is this the right microphone?",
        ),
        InputLevel::TooQuiet => (
            theme::WARNING,
            "Very quiet — move closer, or raise the level in Windows sound settings.",
        ),
        InputLevel::Good => (theme::SUCCESS, "Good level."),
        InputLevel::Hot => (
            theme::DANGER,
            "Too loud — lower the level in Windows sound settings.",
        ),
    };
    ui.add_space(6.0);
    // Amplitude is linear but hearing is not; a square root gives the quiet end
    // enough of the bar to be readable, which is the end that matters here.
    ui.add(
        egui::ProgressBar::new(peak.clamp(0.0, 1.0).sqrt())
            .desired_width(240.0)
            .desired_height(6.0)
            .fill(color),
    );
    ui.label(RichText::new(note).small().weak());
    // Drive the meter while this page is open. 20 fps is plenty for a level
    // bar and an order of magnitude cheaper than the overlay's 60; the cost
    // stops the moment the user leaves Settings.
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(50));
}

/// Push-to-talk shortcut: a "Record" button captures held keys via the same
/// low-level hook the pipeline uses (egui's input can't tell L/R modifiers
/// apart or see the Win key), with the text field kept as a manual fallback.
/// Returns whether recording started or ended so the page can pause/resume the
/// pipeline (only one keyboard hook may run at a time).
pub fn hotkey_section(
    ui: &mut Ui,
    draft: &mut Settings,
    capture: &mut HotkeyCapture,
    ctx: &Context,
) -> CaptureTransition {
    subhead(ui, "Push-to-talk");
    let mut transition = CaptureTransition::None;

    // A completed chord lands straight in the draft and ends recording; the
    // idle field below then renders with the new value in the same frame.
    if capture.is_recording() {
        if let CaptureTransition::Ended = capture.poll_into(&mut draft.hotkey.ptt_key) {
            transition = CaptureTransition::Ended;
        }
    }

    if capture.is_recording() {
        egui::Frame::default()
            .fill(theme::surface(ui.visuals()))
            .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
            .corner_radius(8)
            .inner_margin(egui::Margin::symmetric(14, 11))
            .show(ui, |ui| {
                let held = capture.held_display();
                if held.is_empty() {
                    ui.label(
                        RichText::new("Press and hold your shortcut keys...")
                            .text_style(theme::subheading()),
                    );
                } else {
                    ui.label(RichText::new(held).text_style(theme::subheading()));
                }
                ui.label(
                    RichText::new(
                        "Release to set. Modifier keys (Ctrl, Shift, Alt, Win), CapsLock, \
                         and F1..F24 only.",
                    )
                    .small()
                    .weak(),
                );
            });
        if ui.button("Cancel").clicked() {
            if let CaptureTransition::Ended = capture.cancel() {
                transition = CaptureTransition::Ended;
            }
        }
        return transition;
    }

    ui.horizontal(|ui| {
        ui.add(
            TextEdit::singleline(&mut draft.hotkey.ptt_key)
                .hint_text("LCtrl+LWin")
                .desired_width(160.0),
        );
        if ui
            .button("Record")
            .on_hover_text("Press the keys you want to hold; Hark captures them")
            .clicked()
        {
            if let CaptureTransition::Started = capture.begin(ctx) {
                transition = CaptureTransition::Started;
            }
        }
    });
    if let Some(notice) = capture.notice() {
        inline_error(ui, notice);
    }
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
    transition
}

/// Every selectable voice, in display order. Shared with the tray menu's
/// radio group, so the two pickers can never drift apart.
pub(crate) const VOICES: [VoiceName; 5] = [
    VoiceName::Verbatim,
    VoiceName::Clean,
    VoiceName::Professional,
    VoiceName::Casual,
    VoiceName::Custom,
];

pub fn voice_section(ui: &mut Ui, draft: &mut Settings) {
    subhead(ui, "Voice");
    egui::ComboBox::from_id_salt("voice-picker")
        .selected_text(voice_display(draft.voice.default))
        .show_ui(ui, |ui| {
            for name in VOICES {
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

pub(crate) fn voice_display(name: VoiceName) -> &'static str {
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
                RichText::new("Starts hidden in the system tray when you sign in to Windows.")
                    .small()
                    .weak(),
            );
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
        for name in VOICES {
            assert_eq!(voice_display(name).to_lowercase(), name.label());
        }
    }

    #[test]
    fn input_level_bands_cover_the_range_in_order() {
        assert_eq!(classify_level(0.0), InputLevel::Silent);
        assert_eq!(classify_level(0.004), InputLevel::Silent);
        assert_eq!(classify_level(0.02), InputLevel::TooQuiet);
        assert_eq!(classify_level(0.3), InputLevel::Good);
        assert_eq!(classify_level(0.99), InputLevel::Hot);
        assert_eq!(classify_level(1.0), InputLevel::Hot);
    }

    /// The band a user sits in when they say "it only works if I lean in":
    /// audible, but under the level a recognizer wants.
    #[test]
    fn a_quiet_but_present_signal_reads_as_too_quiet_not_silent() {
        assert_eq!(classify_level(0.01), InputLevel::TooQuiet);
        assert_eq!(classify_level(0.049), InputLevel::TooQuiet);
    }
}

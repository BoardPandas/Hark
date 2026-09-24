//! The status footer (spec §3.11): a persistent full-width strip that is
//! the always-visible truth about the pipeline. Left: state icon + label
//! (icon always paired with text, guardrails §3). Right: the active
//! provider and models, so a disappointing result has an obvious cause.

use crate::pipeline::PipelineStatus;
use crate::theme;
use crate::ui::settings::Section;
use hark_config::{LocalMode, Settings};

use egui::{Frame, Margin, Panel, RichText, Sense, Sides, Ui, Vec2};

/// Render the footer. Returns the Settings section to open when the user
/// clicked an "Open Settings" jump. `intercepted` is a push-to-talk key
/// another program is intercepting (config name), shown while idle.
pub fn show(
    ui: &mut Ui,
    status: &PipelineStatus,
    settings: &Settings,
    intercepted: Option<&str>,
) -> Option<Section> {
    let window_fill = ui.visuals().panel_fill;
    let mut jump = None;
    Panel::bottom("status_footer")
        .min_size(theme::FOOTER_HEIGHT)
        .resizable(false)
        .show_separator_line(true)
        .frame(
            Frame::default()
                .fill(window_fill)
                .inner_margin(Margin::symmetric(20, 8)),
        )
        .show(ui, |ui| {
            Sides::new().height(20.0).show(
                ui,
                |ui| jump = state_side(ui, status, settings, intercepted),
                |ui| {
                    ui.add(
                        egui::Label::new(RichText::new(provider_line(settings)).small().weak())
                            .truncate(),
                    )
                    .on_hover_text(provider_line(settings));
                },
            );
        });
    jump
}

/// The left side: icon + label per state, plus the Open Settings jump when
/// there is something to fix there. Returns the section to open on click.
fn state_side(
    ui: &mut Ui,
    status: &PipelineStatus,
    settings: &Settings,
    intercepted: Option<&str>,
) -> Option<Section> {
    let accent = theme::accent(ui.visuals());
    let weak = ui.visuals().weak_text_color();
    let mut jump = false;
    match status {
        // Idle is where the conflict is worth saying: mid-dictation the footer
        // is busy reporting the dictation, and the report arrives mid-hold.
        PipelineStatus::Idle => match intercepted {
            Some(key) => {
                let key = hark_hotkey::pretty_chord(key);
                icon_label(
                    ui,
                    theme::icons::WARNING,
                    theme::warning(ui.visuals()),
                    &format!("Another app is intercepting {key}"),
                )
                .on_hover_text(intercepted_explanation(&key));
                // The shortcut editor lives in the Audio section.
                return settings_jump(ui).then_some(Section::Audio);
            }
            None => {
                icon_label(
                    ui,
                    theme::icons::MICROPHONE,
                    weak,
                    &format!(
                        "Ready · Hold {} to dictate",
                        hark_hotkey::pretty_chord(&settings.hotkey.ptt_key)
                    ),
                );
            }
        },
        PipelineStatus::Recording => {
            pulsing_dot(ui, theme::danger(ui.visuals()));
            ui.add(egui::Label::new(RichText::new("Recording").small()).truncate());
        }
        PipelineStatus::Processing => {
            spinner(ui, accent);
            ui.add(egui::Label::new(RichText::new("Processing").small()).truncate());
        }
        PipelineStatus::LoadingModel => {
            spinner(ui, accent);
            ui.add(
                egui::Label::new(RichText::new("Loading the on-device model\u{2026}").small())
                    .truncate(),
            );
        }
        PipelineStatus::Errored {
            detail,
            key_related,
        } => {
            icon_label(
                ui,
                theme::icons::WARNING,
                theme::danger(ui.visuals()),
                detail,
            );
            jump = *key_related && settings_jump(ui);
        }
        PipelineStatus::Hint { detail } => {
            icon_label(
                ui,
                theme::icons::MICROPHONE,
                theme::warning(ui.visuals()),
                detail,
            );
            jump = settings_jump(ui);
        }
        PipelineStatus::Stopped {
            detail,
            key_related: true,
        } => {
            // Short, honest copy up front; the full cause on hover.
            icon_label(
                ui,
                theme::icons::KEY,
                theme::warning(ui.visuals()),
                "No STT key yet.",
            )
            .on_hover_text(detail);
            jump = settings_jump(ui);
        }
        PipelineStatus::Stopped {
            detail,
            key_related: false,
        } => {
            icon_label(
                ui,
                theme::icons::WARNING,
                theme::danger(ui.visuals()),
                &format!("Pipeline stopped: {detail}"),
            );
        }
    }
    jump.then_some(Section::Dictation)
}

/// The hover text for an intercepted key (pure; the testable seam).
fn intercepted_explanation(key: &str) -> String {
    format!(
        "You were holding {key}, but Windows reported it as not pressed. Another \
         program is taking the key before Windows sees it: usually a key remapper \
         such as PowerToys Keyboard Manager, or a macro tool. That program also acts \
         on every push-to-talk press, and depending on the order you press the keys, \
         dictation can cut out.\n\nRemove that program's mapping for {key}, or choose a \
         shortcut that does not use it. Hark ignores keys other programs type, so the \
         key it remaps {key} to will not work as a shortcut either."
    )
}

fn icon_label(ui: &mut Ui, icon: &str, icon_color: egui::Color32, text: &str) -> egui::Response {
    ui.label(theme::icon_text(icon).color(icon_color));
    ui.add(egui::Label::new(RichText::new(text).small()).truncate())
}

/// A small filled dot that breathes, for the Recording state (the design's
/// pulsing 10px danger mark; the label still carries the state for AA).
fn pulsing_dot(ui: &mut Ui, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
    let t = ui.input(|i| i.time) as f32;
    let pulse = 0.55 + 0.45 * (t * std::f32::consts::TAU * 0.8).sin().abs();
    ui.painter()
        .circle_filled(rect.center(), 4.0, color.gamma_multiply(pulse));
    // Keep the breath alive even if nothing else asks for a repaint.
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(33));
}

/// A spinning circle-notch for the Processing / model-loading states.
fn spinner(ui: &mut Ui, color: egui::Color32) {
    ui.add(egui::Spinner::new().size(13.0).color(color));
}

fn settings_jump(ui: &mut Ui) -> bool {
    ui.link(RichText::new("Open Settings").small()).clicked()
}

/// "provider · model", plus the cleanup model when a non-Verbatim voice
/// would actually run one (pure; the testable seam).
///
/// The engine shown is the one that actually transcribes: in `Primary` local
/// mode the cloud provider is never called, so naming it here would be a
/// plain lie about where the words come from.
fn provider_line(settings: &Settings) -> String {
    let mut line = match settings.local_stt.mode {
        LocalMode::Primary => format!("on-device · {}", settings.local_stt.model),
        LocalMode::Fallback => format!(
            "{} · {} · on-device fallback",
            settings.provider.kind.label(),
            settings.provider.resolved_model()
        ),
        LocalMode::Off => format!(
            "{} · {}",
            settings.provider.kind.label(),
            settings.provider.resolved_model()
        ),
    };
    // A fused provider does its own cleanup, so the separate cleanup model is
    // never called and must not be advertised as if it were.
    if settings.provider.kind == hark_config::ProviderKind::Gemini
        && settings.provider.live_mode == hark_config::LiveMode::Smart
        && settings.local_stt.mode != LocalMode::Primary
    {
        line.push_str(" · smart");
        return line;
    }
    if let hark_config::CleanupResolution::Resolved(r) = hark_config::resolve_cleanup_provider(
        &settings.provider,
        &settings.voice,
        settings.voice.default,
    ) {
        line.push_str(&format!(" · cleanup {}", r.model));
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_interception_explanation_names_the_key_and_both_ways_out() {
        let text = intercepted_explanation("F12");
        assert!(text.contains("F12"));
        assert!(text.contains("PowerToys"), "name the usual culprit");
        assert!(text.contains("Remove"), "fix one: remove the mapping");
        assert!(text.contains("choose a shortcut"), "fix two: change Hark's");
        // The tempting wrong fix: binding Hark to the key the remapper sends.
        assert!(text.contains("will not work as a shortcut"));
    }

    #[test]
    fn default_config_shows_provider_and_model_without_cleanup() {
        // Defaults: Deepgram STT + Clean voice, but Deepgram cannot host a
        // chat cleanup, so the resolution degrades and no cleanup label may
        // appear (it would blame a model that never runs).
        let settings = Settings::default();
        assert_eq!(provider_line(&settings), "deepgram · nova-3");
    }

    #[test]
    fn openai_with_clean_voice_shows_the_cleanup_model() {
        let settings = hark_config::Settings::from_toml("[provider]\nkind = \"openai\"").unwrap();
        assert_eq!(
            provider_line(&settings),
            "openai · gpt-transcribe · cleanup gpt-5-nano"
        );
    }

    #[test]
    fn verbatim_voice_never_shows_a_cleanup_model() {
        let settings = hark_config::Settings::from_toml(
            "[provider]\nkind = \"openai\"\n[voice]\ndefault = \"verbatim\"",
        )
        .unwrap();
        assert_eq!(provider_line(&settings), "openai · gpt-transcribe");
    }

    #[test]
    fn local_primary_names_the_engine_that_actually_runs() {
        // The Deepgram default is still in config but is never called; the
        // footer must not imply otherwise.
        let settings = Settings::from_toml("[local_stt]\nmode = \"primary\"").unwrap();
        assert_eq!(
            provider_line(&settings),
            "on-device · parakeet-tdt-0.6b-v3-int8"
        );
    }

    #[test]
    fn local_fallback_still_names_the_cloud_provider_first() {
        let settings = Settings::from_toml("[local_stt]\nmode = \"fallback\"").unwrap();
        assert_eq!(
            provider_line(&settings),
            "deepgram · nova-3 · on-device fallback"
        );
    }

    #[test]
    fn local_primary_still_shows_an_overridden_cleanup_model() {
        // Cleanup is a separate call and still runs on-device-primary.
        let settings = Settings::from_toml(
            "[local_stt]\nmode = \"primary\"\n\
             [voice]\ndefault = \"clean\"\n\
             [voice.provider]\nkind = \"openai\"\nmodel = \"gpt-5-nano\"",
        )
        .unwrap();
        assert_eq!(
            provider_line(&settings),
            "on-device · parakeet-tdt-0.6b-v3-int8 · cleanup gpt-5-nano"
        );
    }
}

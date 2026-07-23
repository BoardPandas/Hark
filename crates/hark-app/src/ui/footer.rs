//! The status footer (spec §3.11): a persistent full-width strip that is
//! the always-visible truth about the pipeline. Left: state icon + label
//! (icon always paired with text, guardrails §3). Right: the active
//! provider and models, so a disappointing result has an obvious cause.

use crate::pipeline::PipelineStatus;
use crate::theme;
use hark_config::{LocalMode, Settings};

use egui::{Frame, Margin, Panel, RichText, Sense, Sides, Ui, Vec2};

/// Render the footer. Returns true when the user clicked the "Open
/// Settings" jump on a key-related problem.
pub fn show(ui: &mut Ui, status: &PipelineStatus, settings: &Settings) -> bool {
    let window_fill = ui.visuals().window_fill;
    let mut jump = false;
    Panel::bottom("status_footer")
        .exact_size(28.0)
        .resizable(false)
        .show_separator_line(true)
        .frame(
            Frame::default()
                .fill(window_fill)
                .inner_margin(Margin::symmetric(12, 4)),
        )
        .show(ui, |ui| {
            Sides::new().height(20.0).show(
                ui,
                |ui| jump = state_side(ui, status, settings),
                |ui| {
                    ui.label(RichText::new(provider_line(settings)).small().weak());
                },
            );
        });
    jump
}

/// The left side: icon + label per state, plus the Open Settings jump when
/// the cause is key-related. Returns true on jump click.
fn state_side(ui: &mut Ui, status: &PipelineStatus, settings: &Settings) -> bool {
    let accent = theme::accent(ui.visuals());
    let weak = ui.visuals().weak_text_color();
    let mut jump = false;
    match status {
        PipelineStatus::Idle => {
            icon_label(
                ui,
                theme::icons::MICROPHONE,
                weak,
                &format!("Listening for {}", settings.hotkey.ptt_key),
            );
        }
        PipelineStatus::Recording => {
            pulsing_dot(ui, theme::DANGER);
            ui.add(egui::Label::new(RichText::new("Recording").small()).truncate());
        }
        PipelineStatus::Processing => {
            spinner(ui, accent);
            ui.add(egui::Label::new(RichText::new("Processing").small()).truncate());
        }
        PipelineStatus::LoadingModel => {
            spinner(ui, accent);
            ui.add(
                egui::Label::new(
                    RichText::new("Loading the on-device model\u{2026}").small(),
                )
                .truncate(),
            );
        }
        PipelineStatus::Errored {
            detail,
            key_related,
        } => {
            icon_label(ui, theme::icons::WARNING, theme::DANGER, detail);
            jump = *key_related && settings_jump(ui);
        }
        PipelineStatus::Hint { detail } => {
            icon_label(ui, theme::icons::MICROPHONE, theme::WARNING, detail);
            jump = settings_jump(ui);
        }
        PipelineStatus::Stopped {
            detail,
            key_related: true,
        } => {
            // Short, honest copy up front; the full cause on hover.
            icon_label(ui, theme::icons::KEY, theme::WARNING, "No STT key yet.")
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
                theme::DANGER,
                &format!("Pipeline stopped: {detail}"),
            );
        }
    }
    jump
}

fn icon_label(ui: &mut Ui, icon: &str, icon_color: egui::Color32, text: &str) -> egui::Response {
    ui.label(RichText::new(icon).color(icon_color));
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
            "openai · gpt-4o-mini-transcribe · cleanup gpt-5-nano"
        );
    }

    #[test]
    fn verbatim_voice_never_shows_a_cleanup_model() {
        let settings = hark_config::Settings::from_toml(
            "[provider]\nkind = \"openai\"\n[voice]\ndefault = \"verbatim\"",
        )
        .unwrap();
        assert_eq!(provider_line(&settings), "openai · gpt-4o-mini-transcribe");
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

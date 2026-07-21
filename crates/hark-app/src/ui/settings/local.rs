//! The Settings "On-device model" section: the off/fallback/primary choice,
//! the model card with its download state, and the download/cancel/delete
//! controls.
//!
//! The mode radio persists through the settings draft on Save (like every
//! other field here); the download itself is immediate, because a 670 MB
//! transfer behind a Save button would be a surprising thing to trigger.

use crate::model_download::{ModelDownload, Phase};
use crate::theme;
use egui::{RichText, Ui};
use hark_config::{LocalMode, Settings};
use hark_local_stt::{format_bytes, ModelStatus};

pub fn section(ui: &mut Ui, draft: &mut Settings, download: &mut ModelDownload) {
    download.poll();

    ui.add_space(8.0);
    ui.label(RichText::new("On-device model").text_style(theme::subheading()));
    ui.label(
        RichText::new(
            "Transcribe without the internet. Runs entirely on this computer; \
             nothing is sent to a provider.",
        )
        .small()
        .weak(),
    );

    if !hark_local_stt::LocalEngine::is_available() {
        unavailable(ui);
        return;
    }

    ui.add_space(4.0);
    let before = draft.local_stt.mode;
    ui.radio_value(&mut draft.local_stt.mode, LocalMode::Off, "Off");
    ui.radio_value(
        &mut draft.local_stt.mode,
        LocalMode::Fallback,
        "Use as a backup when the cloud fails or times out",
    );
    ui.radio_value(
        &mut draft.local_stt.mode,
        LocalMode::Primary,
        "Use as my primary engine (no cloud, no API key needed)",
    );
    if draft.local_stt.mode != before {
        download.refresh_status();
    }

    if draft.local_stt.mode.uses_local() {
        ui.add_space(6.0);
        card(ui, draft, download);
    }
}

/// Built without the `engine` feature: say so plainly rather than offering a
/// toggle that could never do anything.
fn unavailable(ui: &mut Ui) {
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(theme::icons::WARNING).color(theme::WARNING));
        ui.label(
            RichText::new(
                "This build of Hark does not include the on-device engine. \
                 Install the \"Hark + Local\" build to use it.",
            )
            .small(),
        );
    });
}

fn card(ui: &mut Ui, draft: &mut Settings, download: &mut ModelDownload) {
    let spec = download.spec();
    let status = download.status();

    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(spec.display_name).strong());
            ui.label(
                RichText::new(format!("· {}", format_bytes(spec.total_bytes())))
                    .small()
                    .weak(),
            );
        });

        state_line(ui, download, status);
        controls(ui, draft, download, status);

        ui.add_space(2.0);
        ui.label(
            RichText::new(format!("Model weights: {}", spec.licence))
                .small()
                .weak(),
        );
    });
}

/// One status line, always icon + text so state is never carried by color
/// alone (design guardrails §3).
fn state_line(ui: &mut Ui, download: &ModelDownload, status: ModelStatus) {
    ui.add_space(4.0);
    match download.phase() {
        Phase::Downloading {
            done_bytes,
            total_bytes,
        } => {
            let fraction = download.fraction().unwrap_or(0.0);
            ui.add(
                egui::ProgressBar::new(fraction)
                    .show_percentage()
                    .desired_width(320.0),
            );
            ui.label(
                RichText::new(format!(
                    "{} of {}",
                    format_bytes(*done_bytes),
                    format_bytes(*total_bytes)
                ))
                .small()
                .weak(),
            );
        }
        Phase::Failed(msg) => line(ui, theme::icons::X, theme::DANGER, msg),
        Phase::Cancelled => line(
            ui,
            theme::icons::WARNING,
            theme::WARNING,
            "Download cancelled. Starting again resumes where it stopped.",
        ),
        Phase::Idle | Phase::Done => match status {
            ModelStatus::Ready => line(
                ui,
                theme::icons::CHECK,
                theme::SUCCESS,
                "Downloaded and ready.",
            ),
            ModelStatus::Partial { have_bytes } => line(
                ui,
                theme::icons::WARNING,
                theme::WARNING,
                &format!(
                    "Partly downloaded ({} so far). Download resumes from here.",
                    format_bytes(have_bytes)
                ),
            ),
            ModelStatus::NotDownloaded => line(
                ui,
                theme::icons::CIRCLE_NOTCH,
                theme::accent(ui.visuals()),
                "Not downloaded yet.",
            ),
        },
    }
}

fn controls(ui: &mut Ui, draft: &mut Settings, download: &mut ModelDownload, status: ModelStatus) {
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        if download.is_busy() {
            if ui.button("Cancel").clicked() {
                download.cancel();
            }
        } else {
            let label = match status {
                ModelStatus::Ready => "Re-download",
                ModelStatus::Partial { .. } => "Resume download",
                ModelStatus::NotDownloaded => "Download",
            };
            let button = egui::Button::new(RichText::new(label).color(theme::ON_ACCENT))
                .fill(theme::accent_fill(ui.visuals()));
            if ui.add(button).clicked() {
                download.start(ui.ctx());
            }
            if status != ModelStatus::NotDownloaded
                && ui
                    .button(format!("{} Delete", theme::icons::TRASH))
                    .clicked()
            {
                if let Err(e) = download.delete() {
                    log::warn!("could not delete the local model: {e}");
                }
            }
        }
    });

    // The one combination that silently does nothing useful: primary mode
    // with no weights on disk means every dictation fails until the download
    // finishes. Say so here rather than letting the user discover it by
    // holding the chord and getting an error.
    if draft.local_stt.mode == LocalMode::Primary && status != ModelStatus::Ready {
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(theme::icons::WARNING).color(theme::WARNING));
            ui.label(
                RichText::new(
                    "Dictation will not work until this finishes downloading, \
                     because no cloud provider is used in this mode.",
                )
                .small(),
            );
        });
    }
}

fn line(ui: &mut Ui, icon: &str, color: egui::Color32, text: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(icon).color(color));
        ui.label(RichText::new(text).small());
    });
}

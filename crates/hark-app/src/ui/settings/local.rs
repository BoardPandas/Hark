//! The on-device half of the Speech to text card: the toggle, Primary or
//! Backup, the model card with its download state, and the
//! download/cancel/delete controls.
//!
//! The mode persists through the settings draft on Save (like every other
//! field here); the download itself is immediate, because a 670 MB transfer
//! behind a Save button would be a surprising thing to trigger. Nothing here
//! starts it except the card's own button.

use super::stt_card;
use crate::model_download::{ModelDownload, Phase};
use crate::theme;
use egui::{RichText, Ui};
use hark_config::{LocalMode, Settings};
use hark_local_stt::{format_bytes, ModelStatus};

/// R4-R8. `key_stored` decides what turning the toggle on means.
pub(super) fn on_device_block(
    ui: &mut Ui,
    draft: &mut Settings,
    download: &mut ModelDownload,
    key_stored: bool,
) {
    if !hark_local_stt::LocalEngine::is_available() {
        unavailable(ui);
        return;
    }

    let before = draft.local_stt.mode;
    // Derived from the draft every frame: a stored bool would drift from it
    // the moment the callout or Discard changed the mode.
    let mut enabled = before.uses_local();
    if ui
        .checkbox(&mut enabled, "Use on-device transcription")
        .changed()
    {
        draft.local_stt.mode = if enabled {
            stt_card::mode_when_enabled(key_stored)
        } else {
            LocalMode::Off
        };
    }

    ui.indent("on-device-mode", |ui| {
        if draft.local_stt.mode.uses_local() {
            // The engine by name, in the on-device hue History uses for it.
            theme::chip(ui, theme::Hue::Violet, None, download.spec().display_name);
            ui.radio_value(
                &mut draft.local_stt.mode,
                LocalMode::Primary,
                "Primary: offline, no key needed",
            )
            .on_hover_text(stt_card::PRIMARY_HOVER);
            ui.radio_value(
                &mut draft.local_stt.mode,
                LocalMode::Fallback,
                "Backup: when the cloud fails or is slow",
            );
            if let Some(line) =
                stt_card::mode_line(draft.local_stt.mode, draft.local_stt.fallback_after_ms)
            {
                ui.label(RichText::new(line).small().weak());
            }
            ui.add_space(6.0);
            card(ui, draft, download);
        } else {
            ui.label(
                RichText::new(format!(
                    "Runs on this computer, so audio never leaves it. One-time {} download.",
                    format_bytes(download.spec().total_bytes())
                ))
                .small()
                .weak(),
            );
            // Turning the toggle off does not stop a transfer; a hidden
            // 670 MB download would be a silent behavior (R8).
            if download.is_busy() {
                state_line(ui, download, download.status());
                if ui.button("Cancel").clicked() {
                    download.cancel();
                }
            }
        }
    });

    if draft.local_stt.mode != before {
        download.refresh_status();
    }
}

/// Built without the `engine` feature: say so plainly rather than offering a
/// toggle that could never do anything.
fn unavailable(ui: &mut Ui) {
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.label(theme::icon_text(theme::icons::WARNING).color(theme::warning(ui.visuals())));
        ui.label(
            RichText::new(
                "This build of Hark does not include the on-device engine. \
                 Install a build with on-device transcription to use it.",
            )
            .small(),
        );
    });
}

pub(super) fn card(ui: &mut Ui, draft: &mut Settings, download: &mut ModelDownload) {
    let spec = download.spec();
    let status = download.status();

    theme::card(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(spec.display_name).text_style(theme::subheading()));
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
                    .desired_width(ui.available_width()),
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
        Phase::Failed(msg) => line(ui, theme::icons::X, theme::danger(ui.visuals()), msg),
        Phase::Cancelled => line(
            ui,
            theme::icons::WARNING,
            theme::warning(ui.visuals()),
            "Download cancelled. Starting again resumes where it stopped.",
        ),
        Phase::Idle | Phase::Done => match status {
            ModelStatus::Ready => line(
                ui,
                theme::icons::CHECK,
                theme::success(ui.visuals()),
                "Downloaded and ready.",
            ),
            ModelStatus::Partial { have_bytes } => line(
                ui,
                theme::icons::WARNING,
                theme::warning(ui.visuals()),
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
            if ui.add(theme::primary_button(ui.visuals(), label)).clicked() {
                download.start(ui.ctx());
            }
            if status != ModelStatus::NotDownloaded
                && ui
                    .button(theme::icon_label_job(
                        ui.style(),
                        theme::icons::TRASH,
                        "Delete",
                    ))
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
            ui.label(theme::icon_text(theme::icons::WARNING).color(theme::warning(ui.visuals())));
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
        ui.label(theme::icon_text(icon).color(color));
        ui.label(RichText::new(text).small());
    });
}

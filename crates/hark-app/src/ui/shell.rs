//! The window shell (Nocturne): a slim top navigation bar (wordmark + page
//! tabs left, Settings + version right), the update banner below it, the
//! status footer across the bottom, and content in a centered column.

use crate::pipeline::PipelineController;
use crate::storage::StorageHandle;
use crate::theme;
use crate::ui::{footer, pages};
use crate::update::{Phase, Updater};
use hark_config::Settings;

use egui::{Frame, Layout, Margin, Panel, RichText, Stroke, Ui};

#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut Ui,
    page: &mut pages::Page,
    settings: &mut Settings,
    pipeline: &mut PipelineController,
    views: &mut pages::Views,
    updater: &mut Updater,
    storage: Option<&StorageHandle>,
    storage_error: Option<&str>,
) {
    // The footer claims the full window width first; it is the always-
    // visible truth about the pipeline.
    let status = pipeline.status().clone();
    let intercepted = pipeline.shortcut_warning().map(str::to_owned);
    if let Some(section) = footer::show(ui, &status, settings, intercepted.as_deref()) {
        *page = pages::Page::Settings;
        views.settings.open(section);
    }

    // Stacked directly above the footer, outside the settings scroll area:
    // a Save that scrolls away is a Save the user does not know they owe.
    if *page == pages::Page::Settings {
        views.settings.unsaved_bar(ui, settings, pipeline);
    }

    // The top navigation bar owns the outermost top strip; the update banner
    // (when visible) stacks directly beneath it.
    let before = *page;
    topbar(ui, page);
    if before == pages::Page::Settings && *page != before {
        views.settings.leave(pipeline);
    }
    if updater.banner_visible() {
        banner(ui, updater, page, &mut views.settings);
    }

    let panel_fill = ui.visuals().window_fill;
    egui::CentralPanel::default()
        .frame(
            Frame::default()
                .fill(panel_fill)
                .inner_margin(theme::CONTENT_MARGIN),
        )
        .show(ui, |ui| {
            pages::show(
                ui,
                page,
                settings,
                pipeline,
                views,
                updater,
                storage,
                storage_error,
            )
        });
}

fn topbar(ui: &mut Ui, page: &mut pages::Page) {
    let fill = ui.visuals().panel_fill;
    Panel::top("topbar")
        .exact_size(theme::TOPBAR_HEIGHT)
        .resizable(false)
        .frame(
            Frame::new()
                .fill(fill)
                .inner_margin(Margin::symmetric(20, 12)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    theme::icon_text(theme::icons::WAVEFORM)
                        .size(theme::BRAND_SIZE)
                        .color(theme::accent(ui.visuals())),
                );
                ui.label(
                    RichText::new("Hark")
                        .text_style(theme::subheading())
                        .size(theme::BRAND_SIZE),
                );
                ui.add_space(theme::GAP);
                for target in [
                    pages::Page::History,
                    pages::Page::Spellbook,
                    pages::Page::Invocations,
                    pages::Page::Stats,
                ] {
                    if theme::nav_button(ui, target.label(), *page == target).clicked() {
                        *page = target;
                    }
                }
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = theme::icon_label_job(ui.style(), theme::icons::GEAR, "Settings");
                    if theme::nav_button(ui, label, *page == pages::Page::Settings).clicked() {
                        *page = pages::Page::Settings;
                    }
                    if ui.available_width() > theme::CONTROL_HEIGHT * 2.0 {
                        ui.label(
                            RichText::new(concat!("v", env!("CARGO_PKG_VERSION")))
                                .small()
                                .weak(),
                        );
                    }
                });
            });
        });
}

/// The update strip beneath the top bar: an accent-900 ground with accent-800
/// bottom edge, accent-200 message, a ghost "Details" jump, an outlined
/// primary action, and a dismiss. Status is icon + label, never color alone.
fn banner(
    ui: &mut Ui,
    updater: &mut Updater,
    page: &mut pages::Page,
    settings: &mut crate::ui::settings::SettingsPage,
) {
    Panel::top("update-banner")
        .resizable(false)
        .show_separator_line(false)
        .frame(
            Frame::default()
                .fill(theme::tint(ui.visuals()))
                .stroke(Stroke::new(1.0, theme::divider(ui.visuals())))
                .inner_margin(Margin::symmetric(20, 9)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let version = updater
                    .release()
                    .map(|r| r.version.clone())
                    .unwrap_or_default();
                let tint = theme::accent(ui.visuals());

                match updater.phase() {
                    Phase::Installing(_) => {
                        ui.add(egui::Spinner::new().size(15.0).color(tint));
                        ui.label(
                            RichText::new(format!("Downloading Hark {version}\u{2026}"))
                                .color(tint),
                        );
                    }
                    Phase::Ready { .. } => {
                        ui.label(theme::icon_text(theme::icons::CHECK).color(tint));
                        ui.label(
                            RichText::new(format!("Hark {version} is ready to install"))
                                .color(tint),
                        );
                    }
                    _ => {
                        ui.label(theme::icon_text(theme::icons::ARROW_UP).color(tint));
                        ui.label(
                            RichText::new(format!("Hark {version} is available.")).color(tint),
                        );
                    }
                }

                // Actions pin to the right edge.
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(theme::icon_text(theme::icons::X).color(tint))
                                .frame(false),
                        )
                        .on_hover_text("Dismiss")
                        .clicked()
                    {
                        updater.dismiss_banner();
                    }
                    banner_action(ui, updater, page, settings);
                });
            });
        });
}

/// The banner's primary action, matched to the current phase. Outlined
/// primary on the accent ground; "Details" is a ghost jump to Settings.
fn banner_action(
    ui: &mut Ui,
    updater: &mut Updater,
    page: &mut pages::Page,
    settings: &mut crate::ui::settings::SettingsPage,
) {
    let visuals = ui.visuals().clone();
    match updater.phase() {
        Phase::Installing(_) => {}
        Phase::Ready { .. } => {
            if ui
                .add(theme::primary_button(&visuals, "Restart now"))
                .clicked()
            {
                updater.restart();
            }
        }
        _ => {
            if updater.can_self_install() {
                if ui.add(theme::primary_button(&visuals, "Install")).clicked() {
                    updater.start_install(ui.ctx());
                }
            } else if let Some(url) = updater.release().map(|r| r.html_url.clone()) {
                if ui
                    .add(theme::primary_button(&visuals, "View release"))
                    .clicked()
                {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                }
            }
            // "Details" jumps to the Settings section with the release notes.
            if ui
                .add(
                    egui::Button::new(RichText::new("Details").color(theme::accent(ui.visuals())))
                        .frame(false),
                )
                .clicked()
            {
                *page = pages::Page::Settings;
                settings.open(crate::ui::settings::Section::Updates);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_labels_fit_the_minimum_window_in_both_themes() {
        for preference in [egui::ThemePreference::Dark, egui::ThemePreference::Light] {
            let ctx = egui::Context::default();
            theme::apply(&ctx);
            ctx.set_theme(preference);
            let mut page = pages::Page::Settings;
            for _ in 0..2 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(720.0, 480.0),
                        )),
                        ..Default::default()
                    },
                    |ui| topbar(ui, &mut page),
                );
                output.textures_delta.clear();
                let mut labels = Vec::new();
                for shape in output.shapes {
                    if let egui::Shape::Text(text) = shape.shape {
                        let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
                        assert!(
                            rect.left() >= 0.0 && rect.right() <= 720.0,
                            "label {:?} overflows: {rect:?}",
                            text.galley.text()
                        );
                        labels.push(text.galley.text().to_owned());
                    }
                }
                for page in ["History", "Spellbook", "Invocations", "Stats"] {
                    assert!(labels.iter().any(|label| label == page), "missing {page}");
                }
                assert!(labels.iter().any(|label| label.contains("Settings")));
            }
        }
    }
}

//! Native window shell: responsive sidebar, appearance picker, update banner,
//! persistent pipeline status, and the existing page editors.

use crate::meeting::MeetingController;
use crate::pipeline::PipelineController;
use crate::storage::StorageHandle;
use crate::theme;
use crate::ui::{footer, navigation, pages};
use crate::update::{Phase, Updater};
use hark_config::Settings;

use egui::{Frame, Layout, Margin, Panel, RichText, Stroke, Ui};

#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut Ui,
    page: &mut pages::Page,
    settings: &mut Settings,
    pipeline: &mut PipelineController,
    meetings: &mut MeetingController,
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

    // Page navigation keeps the Settings draft and cancels shortcut capture on leave.
    let before = *page;
    navigation::show(ui, page);
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
                meetings,
                views,
                updater,
                storage,
                storage_error,
            )
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
                updater.restart(ui.ctx());
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

    fn text_shapes(shape: &egui::Shape, visit: &mut impl FnMut(&egui::epaint::TextShape)) {
        match shape {
            egui::Shape::Text(text) => visit(text),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    text_shapes(shape, visit);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn navigation_labels_fit_small_windows_in_all_themes() {
        for preference in theme::Appearance::ALL {
            for (width, height) in [
                (720.0, 480.0),
                (760.0, 480.0),
                (900.0, 480.0),
                (960.0, 640.0),
            ] {
                let ctx = egui::Context::default();
                theme::apply(&ctx);
                theme::set_appearance(&ctx, preference);
                let mut page = pages::Page::Settings;
                for frame in 0..3 {
                    let mut output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, height),
                            )),
                            ..Default::default()
                        },
                        |ui| {
                            // Reserve both the persistent status and dirty Settings bars.
                            Panel::bottom("test-footers")
                                .exact_size(90.0)
                                .show(ui, |_| {});
                            navigation::show(ui, &mut page);
                        },
                    );
                    output.textures_delta.clear();
                    // egui's first sizing pass may not paint all new widgets.
                    if frame == 0 {
                        continue;
                    }
                    let mut labels = Vec::new();
                    for clipped in &output.shapes {
                        text_shapes(&clipped.shape, &mut |text| {
                            let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
                            assert!(
                                rect.left() >= 0.0 && rect.right() <= width,
                                "{preference:?} {width}x{height}: {:?} overflows: {rect:?}",
                                text.galley.text()
                            );
                            let is_page = pages::Page::ALL
                                .iter()
                                .any(|page| text.galley.text().contains(page.label()));
                            if is_page {
                                assert!(clipped.clip_rect.expand(1.0).contains_rect(rect),
                                    "{preference:?} {width}x{height}: {:?} clipped: {rect:?} by {:?}", text.galley.text(), clipped.clip_rect);
                            }
                            labels.push(text.galley.text().to_owned());
                        });
                    }
                    for page in [
                        "Home",
                        "History",
                        "Spellbook",
                        "Invocations",
                        "Insights",
                        "Settings",
                    ] {
                        assert!(
                            labels.iter().any(|label| label.contains(page)),
                            "{preference:?} {width}x{height}: missing {page}: {labels:?}"
                        );
                    }
                }
            }
        }
    }
}

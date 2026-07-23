//! The window shell (Nocturne): a slim top navigation bar (wordmark + page
//! tabs left, Settings + version right), the update banner below it, the
//! status footer across the bottom, and content in a centered column.

use crate::pipeline::PipelineController;
use crate::storage::StorageHandle;
use crate::theme;
use crate::ui::{footer, pages};
use crate::update::{Phase, Updater};
use hark_config::Settings;

use egui::{
    Color32, CursorIcon, Frame, Layout, Margin, Panel, Rangef, RichText, Sense, Stroke, TextStyle,
    Ui, Vec2,
};

const TOPBAR_HEIGHT: f32 = 40.0;

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
    if footer::show(ui, &status, settings) {
        *page = pages::Page::Settings;
    }

    // Stacked directly above the footer, outside the settings scroll area:
    // a Save that scrolls away is a Save the user does not know they owe.
    if *page == pages::Page::Settings {
        views.settings.unsaved_bar(ui, settings, pipeline);
    }

    // The top navigation bar owns the outermost top strip; the update banner
    // (when visible) stacks directly beneath it.
    topbar(ui, page);
    if updater.banner_visible() {
        banner(ui, updater, page);
    }

    let panel_fill = ui.visuals().panel_fill;
    egui::CentralPanel::default()
        .frame(Frame::default().fill(panel_fill).inner_margin(Margin {
            left: 24,
            right: 24,
            top: 22,
            bottom: 14,
        }))
        .show(ui, |ui| {
            pages::show(
                ui,
                *page,
                settings,
                pipeline,
                views,
                updater,
                storage,
                storage_error,
            )
        });
}

/// The slim top bar: accent mic glyph + "Hark" wordmark, then the page tabs,
/// then the Settings button and version caption pinned to the right.
fn topbar(ui: &mut Ui, page: &mut pages::Page) {
    let window_fill = ui.visuals().window_fill;
    Panel::top("topbar")
        .exact_size(TOPBAR_HEIGHT)
        .resizable(false)
        .show_separator_line(true)
        .frame(
            Frame::default()
                .fill(window_fill)
                .inner_margin(Margin::symmetric(20, 0)),
        )
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let accent = theme::accent(ui.visuals());
                ui.label(
                    RichText::new(theme::icons::MICROPHONE)
                        .size(17.0)
                        .color(accent),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new("Hark")
                        .text_style(theme::subheading())
                        .size(17.0),
                );
                ui.add_space(14.0);

                for target in [
                    pages::Page::History,
                    pages::Page::Dictionary,
                    pages::Page::Invocations,
                    pages::Page::Stats,
                ] {
                    tab(ui, page, target);
                }

                // Settings + version hug the right edge.
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(concat!("v", env!("CARGO_PKG_VERSION")))
                            .small()
                            .color(ui.visuals().weak_text_color().gamma_multiply(0.85)),
                    );
                    ui.add_space(8.0);
                    settings_tab(ui, page);
                });
            });
        });
}

/// One page tab: label text, accent when active or hovered, with a 2px accent
/// underline under the active tab. No pill — hierarchy is the accent line.
fn tab(ui: &mut Ui, page: &mut pages::Page, target: pages::Page) {
    let selected = *page == target;
    let font = TextStyle::Body.resolve(ui.style());
    let galley =
        ui.painter()
            .layout_no_wrap(target.label().to_string(), font, Color32::PLACEHOLDER);
    let pad = Vec2::new(11.0, 6.0);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + pad * 2.0, Sense::click());

    let accent = theme::accent(ui.visuals());
    let color = if selected || resp.hovered() {
        accent
    } else {
        ui.visuals().text_color()
    };
    let text_pos = rect.center() - galley.size() / 2.0;
    ui.painter().galley(text_pos, galley, color);
    if selected {
        let y = rect.bottom() - 1.0;
        ui.painter().hline(
            Rangef::new(rect.left() + pad.x, rect.right() - pad.x),
            y,
            Stroke::new(2.0, accent),
        );
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    if resp.clicked() {
        *page = target;
    }
}

/// The Settings tab is an outlined button (gear + label); on the Settings
/// page it takes the accent border + accent text.
fn settings_tab(ui: &mut Ui, page: &mut pages::Page) {
    let on_settings = *page == pages::Page::Settings;
    let label = format!("{}  Settings", theme::icons::GEAR);
    let clicked = if on_settings {
        ui.add(theme::primary_button(ui.visuals(), label)).clicked()
    } else {
        ui.button(label).clicked()
    };
    if clicked {
        *page = pages::Page::Settings;
    }
}

/// The update strip beneath the top bar: an accent-900 ground with accent-800
/// bottom edge, accent-200 message, a ghost "Details" jump, an outlined
/// primary action, and a dismiss. Status is icon + label, never color alone.
fn banner(ui: &mut Ui, updater: &mut Updater, page: &mut pages::Page) {
    Panel::top("update-banner")
        .resizable(false)
        .show_separator_line(false)
        .frame(
            Frame::default()
                .fill(theme::ACCENT_900)
                .stroke(Stroke::new(1.0, theme::ACCENT_800))
                .inner_margin(Margin::symmetric(20, 9)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let version = updater
                    .release()
                    .map(|r| r.version.clone())
                    .unwrap_or_default();
                let tint = theme::ACCENT_200;

                match updater.phase() {
                    Phase::Installing(_) => {
                        ui.add(egui::Spinner::new().size(15.0).color(tint));
                        ui.label(
                            RichText::new(format!("Downloading Hark {version}\u{2026}"))
                                .color(tint),
                        );
                    }
                    Phase::Ready { .. } => {
                        ui.label(RichText::new(theme::icons::CHECK).color(tint));
                        ui.label(
                            RichText::new(format!("Hark {version} is ready to install"))
                                .color(tint),
                        );
                    }
                    _ => {
                        ui.label(RichText::new(theme::icons::ARROW_UP).color(tint));
                        ui.label(
                            RichText::new(format!("Hark {version} is available.")).color(tint),
                        );
                    }
                }

                // Actions pin to the right edge.
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(RichText::new(theme::icons::X).color(tint))
                                .frame(false),
                        )
                        .on_hover_text("Dismiss")
                        .clicked()
                    {
                        updater.dismiss_banner();
                    }
                    banner_action(ui, updater, page);
                });
            });
        });
}

/// The banner's primary action, matched to the current phase. Outlined
/// primary on the accent ground; "Details" is a ghost jump to Settings.
fn banner_action(ui: &mut Ui, updater: &mut Updater, page: &mut pages::Page) {
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
                    egui::Button::new(RichText::new("Details").color(theme::ACCENT_200))
                        .frame(false),
                )
                .clicked()
            {
                *page = pages::Page::Settings;
            }
        }
    }
}

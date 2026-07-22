//! The window shell (spec §3.11): status footer across the bottom, fixed
//! left sidebar with the nav, content in a centered column.

use crate::pipeline::PipelineController;
use crate::storage::StorageHandle;
use crate::theme;
use crate::ui::{footer, pages};
use crate::update::{Phase, Updater};
use hark_config::Settings;

use egui::{Color32, CornerRadius, Frame, Margin, Panel, RichText, Stroke, Ui, Vec2};

const SIDEBAR_WIDTH: f32 = 184.0;

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

    // The update banner claims the top strip, below the title bar, above the
    // sidebar and content. Only shown when an update is pending and undismissed.
    if updater.banner_visible() {
        banner(ui, updater, page);
    }

    let window_fill = ui.visuals().window_fill;
    let panel_fill = ui.visuals().panel_fill;

    Panel::left("sidebar")
        .exact_size(SIDEBAR_WIDTH)
        .resizable(false)
        .show_separator_line(true)
        .frame(
            Frame::default()
                .fill(window_fill)
                .inner_margin(Margin::symmetric(10, 14)),
        )
        .show(ui, |ui| sidebar(ui, page));

    egui::CentralPanel::default()
        .frame(
            Frame::default()
                .fill(panel_fill)
                .inner_margin(Margin::same(24)),
        )
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

/// The accent-filled update strip across the top. Primary action (install /
/// view / restart), a shortcut to the Settings section for the full notes, and
/// a dismiss. Colors from `theme.rs`; status is icon + label, never color alone.
fn banner(ui: &mut Ui, updater: &mut Updater, page: &mut pages::Page) {
    let accent = theme::accent_fill(ui.visuals());
    Panel::top("update-banner")
        .resizable(false)
        .show_separator_line(false)
        .frame(
            Frame::default()
                .fill(accent)
                .inner_margin(Margin::symmetric(16, 8)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let version = updater
                    .release()
                    .map(|r| r.version.clone())
                    .unwrap_or_default();

                match updater.phase() {
                    Phase::Installing(_) => {
                        ui.add(egui::Spinner::new().size(16.0).color(theme::ON_ACCENT));
                        ui.label(
                            RichText::new(format!("Downloading Hark {version}\u{2026}"))
                                .color(theme::ON_ACCENT),
                        );
                    }
                    Phase::Ready { .. } => {
                        ui.label(RichText::new(theme::icons::CHECK).color(theme::ON_ACCENT));
                        ui.label(
                            RichText::new(format!("Hark {version} is ready to install"))
                                .color(theme::ON_ACCENT),
                        );
                    }
                    _ => {
                        ui.label(RichText::new(theme::icons::CIRCLE_NOTCH).color(theme::ON_ACCENT));
                        ui.label(
                            RichText::new(format!("Update available: Hark {version}"))
                                .color(theme::ON_ACCENT),
                        );
                    }
                }

                // Actions pin to the right edge.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(egui::Button::new(
                            RichText::new(theme::icons::X).color(theme::ON_ACCENT),
                        ))
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

/// The banner's primary button, matched to the current phase.
fn banner_action(ui: &mut Ui, updater: &mut Updater, page: &mut pages::Page) {
    let light = |text: &str| {
        egui::Button::new(RichText::new(text.to_string()).color(theme::ON_ACCENT))
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::new(1.0, theme::ON_ACCENT))
    };
    match updater.phase() {
        Phase::Installing(_) => {}
        Phase::Ready { .. } => {
            if ui.add(light("Restart now")).clicked() {
                updater.restart();
            }
        }
        _ => {
            // "Details" jumps to the Settings section with the release notes.
            if ui.add(light("Details")).clicked() {
                *page = pages::Page::Settings;
            }
            if updater.can_self_install() {
                if ui.add(light("Install")).clicked() {
                    updater.start_install(ui.ctx());
                }
            } else if let Some(url) = updater.release().map(|r| r.html_url.clone()) {
                if ui.add(light("View release")).clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                }
            }
        }
    }
}

fn sidebar(ui: &mut Ui, page: &mut pages::Page) {
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        ui.label(
            RichText::new(theme::icons::MICROPHONE)
                .text_style(theme::subheading())
                .color(theme::accent(ui.visuals())),
        );
        ui.label(RichText::new("Hark").text_style(theme::subheading()));
    });
    ui.add_space(14.0);

    for target in [
        pages::Page::History,
        pages::Page::Dictionary,
        pages::Page::Invocations,
        pages::Page::Stats,
    ] {
        nav_item(ui, page, target);
    }

    // Settings pins to the bottom, above the version caption.
    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
        ui.add_space(2.0);
        ui.label(
            RichText::new(concat!("v", env!("CARGO_PKG_VERSION")))
                .small()
                .weak(),
        );
        ui.add_space(4.0);
        nav_item(ui, page, pages::Page::Settings);
    });
}

/// One nav row: icon + label; the selected item gets the accent pill.
fn nav_item(ui: &mut Ui, page: &mut pages::Page, target: pages::Page) {
    let selected = *page == target;
    let (fill, text_color) = if selected {
        (theme::accent_fill(ui.visuals()), theme::ON_ACCENT)
    } else {
        (Color32::TRANSPARENT, ui.visuals().text_color())
    };
    let label = RichText::new(format!("{}   {}", target.icon(), target.label())).color(text_color);
    let button = egui::Button::new(label)
        .fill(fill)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(6))
        .min_size(Vec2::new(ui.available_width(), 32.0));
    if ui.add(button).clicked() {
        *page = target;
    }
}

//! The window shell (spec §3.11): status footer across the bottom, fixed
//! left sidebar with the nav, content in a centered column.

use crate::pipeline::PipelineStatus;
use crate::theme;
use crate::ui::{footer, pages};
use hark_config::Settings;
use hark_keychain::KeyStatus;

use egui::{Color32, CornerRadius, Frame, Margin, Panel, RichText, Stroke, Ui, Vec2};

const SIDEBAR_WIDTH: f32 = 184.0;

pub fn show(
    ui: &mut Ui,
    page: &mut pages::Page,
    status: &PipelineStatus,
    settings: &Settings,
    key_status: &KeyStatus,
) {
    // The footer claims the full window width first; it is the always-
    // visible truth about the pipeline.
    if footer::show(ui, status, settings) {
        *page = pages::Page::Settings;
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
        .show(ui, |ui| pages::show(ui, *page, settings, key_status));
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

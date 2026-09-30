//! Responsive native navigation. Existing page editors keep their own state.
use crate::theme;
use crate::ui::pages::Page;
use egui::{Align, Frame, Layout, Margin, Panel, RichText, Ui};

pub fn show(ui: &mut Ui, page: &mut Page) {
    // Leave enough vertical room for every destination, including a dirty
    // Settings footer. Short windows use the same wrapped navigation as narrow ones.
    let wide = ui.available_width() >= theme::SIDEBAR_BREAKPOINT
        && ui.available_height() >= theme::SIDEBAR_MIN_HEIGHT;
    Panel::top("appearance-bar")
        .resizable(false)
        .frame(
            Frame::new()
                .fill(theme::sidebar(ui.visuals()))
                .inner_margin(Margin::symmetric(theme::CARD_PADDING, theme::GAP as i8)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if !wide {
                    brand(ui);
                } else {
                    ui.label(
                        RichText::new("Your personal dictation workspace")
                            .small()
                            .weak(),
                    );
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    theme::appearance_picker(ui);
                });
            });
            if !wide {
                ui.add_space(theme::GAP);
                ui.horizontal_wrapped(|ui| {
                    for target in Page::ALL {
                        if visible(target) {
                            button(ui, page, target, false);
                        }
                    }
                });
            }
        });
    if !wide {
        return;
    }
    Panel::left("workspace-navigation")
        .exact_size(theme::SIDEBAR_WIDTH)
        .resizable(false)
        .frame(
            Frame::new()
                .fill(theme::sidebar(ui.visuals()))
                .inner_margin(Margin::same(theme::GAP as i8)),
        )
        .show(ui, |ui| {
            ui.add_space(theme::ROW_GAP);
            brand(ui);
            ui.add_space(theme::SECTION_GAP);
            for target in [Page::Home, Page::Stats, Page::History, Page::Meetings] {
                if visible(target) {
                    button(ui, page, target, true);
                }
            }
            ui.add_space(theme::SECTION_GAP);
            ui.label(RichText::new("MAKE IT YOURS").small().weak());
            ui.add_space(theme::GAP);
            for target in [Page::Spellbook, Page::Invocations] {
                button(ui, page, target, true);
            }
            ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                ui.label(
                    RichText::new(concat!("Hark v", env!("CARGO_PKG_VERSION")))
                        .small()
                        .weak(),
                );
                button(ui, page, Page::Settings, true);
            });
        });
}

fn visible(page: Page) -> bool {
    page != Page::Meetings || hark_pipeline::meeting::meetings_supported()
}

fn brand(ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.label(
            theme::icon_text(theme::icons::WAVEFORM)
                .size(theme::BRAND_SIZE)
                .color(theme::accent(ui.visuals())),
        );
        ui.label(
            RichText::new("hark.")
                .font(theme::hero_font())
                .size(theme::TITLE_SIZE),
        );
    });
}

fn button(ui: &mut Ui, page: &mut Page, target: Page, full_width: bool) {
    let label = theme::icon_label_job(ui.style(), target.icon(), target.label());
    let mut button = egui::Button::new(label)
        .selected(*page == target)
        .frame_when_inactive(*page == target)
        .min_size(egui::vec2(
            if full_width {
                ui.available_width()
            } else {
                0.0
            },
            theme::NAV_HEIGHT,
        ));
    if *page == target {
        button = button.fill(theme::surface(ui.visuals()));
    }
    let response = ui.add(button);
    theme::focus_ring(ui, &response);
    if response.clicked() {
        *page = target;
    }
}

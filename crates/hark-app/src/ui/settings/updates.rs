//! The Settings "Updates" section: the running version, a manual "Check for
//! updates" button with a live result, the install/restart controls, and the
//! auto-check-on-startup toggle. It reads and drives the shared [`Updater`]
//! (the same instance the startup banner uses); the toggle persists through the
//! settings draft on Save.

use crate::theme;
use crate::update::{Phase, Updater};
use egui::{RichText, Ui};
use hark_config::Settings;

pub fn section(ui: &mut Ui, updater: &mut Updater, draft: &mut Settings) {
    ui.add_space(8.0);
    ui.label(RichText::new("Updates").text_style(theme::subheading()));
    ui.label(
        RichText::new(format!("You're on Hark v{}", updater.current_version()))
            .small()
            .weak(),
    );

    let busy = updater.is_busy();
    ui.horizontal(|ui| {
        if ui
            .add_enabled(!busy, egui::Button::new("Check for updates"))
            .clicked()
        {
            updater.start_check(ui.ctx());
        }
        if matches!(updater.phase(), Phase::Checking) {
            ui.add(egui::Spinner::new().size(16.0));
            ui.label(RichText::new("Checking GitHub\u{2026}").small().weak());
        }
    });

    result(ui, updater);

    ui.add_space(6.0);
    ui.checkbox(
        &mut draft.updates.check_on_startup,
        "Check for updates automatically at startup",
    );
}

/// One flat description of the current phase, snapshotted (owned strings) so the
/// action buttons below can borrow `updater` mutably without a borrow clash.
enum Kind {
    Idle,
    Checking,
    UpToDate,
    Failed(String),
    Available,
    Installing,
    Ready,
}

fn result(ui: &mut Ui, updater: &mut Updater) {
    let (kind, version, notes, html_url, can_install) = {
        let kind = match updater.phase() {
            Phase::Idle => Kind::Idle,
            Phase::Checking => Kind::Checking,
            Phase::UpToDate => Kind::UpToDate,
            Phase::Failed(m) => Kind::Failed(m.clone()),
            Phase::Available(_) => Kind::Available,
            Phase::Installing(_) => Kind::Installing,
            Phase::Ready { .. } => Kind::Ready,
        };
        let release = updater.release();
        (
            kind,
            release.map(|r| r.version.clone()).unwrap_or_default(),
            release.map(|r| r.notes.clone()).unwrap_or_default(),
            release.map(|r| r.html_url.clone()).unwrap_or_default(),
            updater.can_self_install(),
        )
    };

    match kind {
        Kind::Idle | Kind::Checking => {}
        Kind::UpToDate => status(
            ui,
            theme::icons::CHECK,
            theme::SUCCESS,
            "You're on the latest version.",
        ),
        Kind::Failed(msg) => {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(theme::icons::X).color(theme::DANGER));
                ui.label(&msg);
            });
        }
        Kind::Installing => {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(16.0));
                ui.label(format!("Downloading and verifying Hark {version}\u{2026}"));
            });
        }
        Kind::Available => {
            status(
                ui,
                theme::icons::CIRCLE_NOTCH,
                theme::accent(ui.visuals()),
                &format!("Hark {version} is available."),
            );
            notes_view(ui, &notes);
            if can_install {
                if ui.add(accent_button(ui, "Download & install")).clicked() {
                    updater.start_install(ui.ctx());
                }
            } else if ui.button("View release").clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(html_url));
            }
        }
        Kind::Ready => {
            status(
                ui,
                theme::icons::CHECK,
                theme::SUCCESS,
                &format!("Hark {version} downloaded and verified."),
            );
            notes_view(ui, &notes);
            if ui.add(accent_button(ui, "Restart to finish")).clicked() {
                updater.restart();
            }
        }
    }
}

fn status(ui: &mut Ui, icon: &str, color: egui::Color32, text: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(icon).color(color));
        ui.label(text);
    });
}

fn accent_button(ui: &Ui, text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_string()).color(theme::ON_ACCENT))
        .fill(theme::accent_fill(ui.visuals()))
}

/// Release notes as plain text in a bounded scroller (GitHub bodies are
/// Markdown; egui has no Markdown renderer, so they show verbatim).
fn notes_view(ui: &mut Ui, notes: &str) {
    let notes = notes.trim();
    if notes.is_empty() {
        return;
    }
    egui::ScrollArea::vertical()
        .id_salt("update-notes")
        .max_height(140.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.label(RichText::new(notes).small());
        });
}

//! Shared widgets: the destructive-confirm modal (spec §3.11) and the
//! centered empty/error state every panel ships (state coverage rule: a
//! blank region is a bug). Destructive actions never fire on first click;
//! the confirm button carries the verb, and Cancel is the focused default.

use crate::theme;
use egui::{Id, Key, Modal, RichText, Ui};

/// Centered panel state: icon, one-line title, weak caption. Serves empty,
/// gated, and error states alike (the icon and copy carry the difference).
pub fn empty_state(ui: &mut Ui, icon: &str, title: &str, caption: &str) {
    ui.add_space(56.0);
    ui.vertical_centered(|ui| {
        ui.label(
            RichText::new(icon)
                .size(40.0)
                .color(ui.visuals().weak_text_color()),
        );
        ui.add_space(6.0);
        ui.label(RichText::new(title).text_style(theme::subheading()));
        ui.label(RichText::new(caption).weak());
    });
}

/// A pending destructive confirmation. Callers hold at most one in state
/// and render it every frame until it resolves.
pub struct Confirm {
    title: String,
    body: String,
    /// The destructive verb ("Remove key", "Delete 214 entries").
    action: String,
    /// Focus lands on Cancel the first frame (safe default under Enter).
    focus_cancel: bool,
}

impl Confirm {
    pub fn new(
        title: impl Into<String>,
        body: impl Into<String>,
        action: impl Into<String>,
    ) -> Self {
        Confirm {
            title: title.into(),
            body: body.into(),
            action: action.into(),
            focus_cancel: true,
        }
    }

    /// Render the modal. `Some(true)` = confirmed, `Some(false)` = declined
    /// (Cancel, Esc, or backdrop click), `None` = still open.
    pub fn show(&mut self, ui: &Ui, id_salt: &str) -> Option<bool> {
        let mut outcome = None;
        let modal = Modal::new(Id::new(("confirm", id_salt))).show(ui.ctx(), |ui| {
            ui.set_max_width(360.0);
            ui.label(RichText::new(&self.title).text_style(theme::subheading()));
            ui.label(&self.body);
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let cancel = ui.button("Cancel");
                if self.focus_cancel {
                    cancel.request_focus();
                    self.focus_cancel = false;
                }
                if cancel.clicked() {
                    outcome = Some(false);
                }
                let confirm =
                    egui::Button::new(RichText::new(&self.action).color(theme::ON_ACCENT))
                        .fill(theme::DANGER_FILL);
                if ui.add(confirm).clicked() {
                    outcome = Some(true);
                }
            });
            if ui.input(|i| i.key_pressed(Key::Escape)) {
                outcome = Some(false);
            }
        });
        // Backdrop click (and Esc, when the Modal consumed it first).
        if outcome.is_none() && modal.should_close() {
            outcome = Some(false);
        }
        outcome
    }
}

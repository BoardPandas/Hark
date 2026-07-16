//! The key section (spec §3.6): masked paste straight to the OS keychain,
//! a status line from a cached `key_status`, and Remove behind a confirm.
//! A stored key is never displayed back, in full or in part; the paste
//! buffer is cleared the moment the key is handed to the keychain.

use crate::theme;
use crate::ui::widgets::Confirm;
use egui::{Color32, Key, RichText, TextEdit, Ui};
use hark_keychain::KeyStatus;

pub struct KeySection {
    /// Stable modal/widget salt ("stt" / "cleanup"), not the account.
    id_salt: &'static str,
    account: String,
    /// Cached: reading the OS keychain per frame is forbidden (crate rules).
    status: KeyStatus,
    paste: String,
    /// Outcome of the last store/remove; persists until the next action.
    notice: Option<Result<String, String>>,
    confirm: Option<Confirm>,
}

impl KeySection {
    pub fn new(id_salt: &'static str, account: &str) -> Self {
        KeySection {
            id_salt,
            account: account.to_string(),
            status: hark_keychain::key_status(account),
            paste: String::new(),
            notice: None,
            confirm: None,
        }
    }

    /// Point the section at a (possibly different) keychain account.
    /// Re-reads status only on an actual change, never per frame.
    pub fn sync_account(&mut self, account: &str) {
        if self.account != account {
            self.account = account.to_string();
            self.status = hark_keychain::key_status(account);
            self.paste.clear();
            self.notice = None;
            self.confirm = None;
        }
    }

    /// Re-read the cached status (call after saves or external changes).
    pub fn refresh(&mut self) {
        self.status = hark_keychain::key_status(&self.account);
    }

    pub fn stored(&self) -> bool {
        matches!(self.status, KeyStatus::Stored)
    }

    /// Render. Returns true when a store or remove changed the keychain.
    pub fn show(&mut self, ui: &mut Ui) -> bool {
        let mut changed = false;

        let (icon, color, text) = status_line(&self.status, &self.account);
        ui.horizontal(|ui| {
            ui.label(RichText::new(icon).color(color));
            ui.label(text);
        });

        ui.horizontal(|ui| {
            let response = ui.add(
                TextEdit::singleline(&mut self.paste)
                    .password(true)
                    .hint_text("Paste API key")
                    .desired_width(280.0),
            );
            let entered = response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            let has_input = !self.paste.trim().is_empty();
            let store = ui
                .add_enabled(has_input, egui::Button::new("Store"))
                .clicked();
            if (store || entered) && has_input {
                match hark_keychain::store_key(&self.account, &self.paste) {
                    Ok(()) => {
                        self.notice = Some(Ok("Key stored".to_string()));
                        changed = true;
                    }
                    Err(e) => self.notice = Some(Err(e.to_string())),
                }
                // The paste buffer never outlives the store attempt.
                self.paste.clear();
                self.refresh();
            }
            if self.stored() && ui.button("Remove").clicked() {
                self.confirm = Some(Confirm::new(
                    "Remove key?",
                    format!(
                        "Hark forgets the \"{}\" key stored in the OS keychain. \
                         Dictation stops until a new key is stored.",
                        self.account
                    ),
                    "Remove key",
                ));
            }
        });

        if let Some(notice) = &self.notice {
            let (icon, color, text) = match notice {
                Ok(t) => (theme::icons::CHECK, theme::SUCCESS, t.as_str()),
                Err(t) => (theme::icons::WARNING, theme::DANGER, t.as_str()),
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon).color(color));
                ui.label(RichText::new(text).small());
            });
        }

        if let Some(confirm) = &mut self.confirm {
            match confirm.show(ui, self.id_salt) {
                Some(true) => {
                    match hark_keychain::delete_key(&self.account) {
                        Ok(()) => {
                            self.notice = Some(Ok("Key removed".to_string()));
                            changed = true;
                        }
                        Err(e) => self.notice = Some(Err(e.to_string())),
                    }
                    self.refresh();
                    self.confirm = None;
                }
                Some(false) => self.confirm = None,
                None => {}
            }
        }
        changed
    }
}

/// Pure status -> (icon, color, copy) mapping (the testable seam). Status is
/// never conveyed by color alone; the icon and copy carry it too.
fn status_line(status: &KeyStatus, account: &str) -> (&'static str, Color32, String) {
    match status {
        KeyStatus::Stored => (
            theme::icons::CHECK,
            theme::SUCCESS,
            format!("Key stored for {account}"),
        ),
        KeyStatus::Missing => (
            theme::icons::KEY,
            theme::WARNING,
            format!("No key for {account} yet"),
        ),
        KeyStatus::Backend(detail) => (
            theme::icons::WARNING,
            theme::DANGER,
            format!("Keychain unavailable: {detail}"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_lines_name_the_account_and_never_leak_key_material() {
        let (icon, _, text) = status_line(&KeyStatus::Stored, "deepgram");
        assert_eq!(icon, theme::icons::CHECK);
        assert_eq!(text, "Key stored for deepgram");

        let (icon, _, text) = status_line(&KeyStatus::Missing, "groq");
        assert_eq!(icon, theme::icons::KEY);
        assert_eq!(text, "No key for groq yet");

        let (icon, _, text) =
            status_line(&KeyStatus::Backend("locked vault".to_string()), "openai");
        assert_eq!(icon, theme::icons::WARNING);
        assert_eq!(text, "Keychain unavailable: locked vault");
    }
}

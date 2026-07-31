//! The push-to-talk shortcut control. Lives beside `capture.rs` (which owns
//! the recording state) rather than in `form.rs`, because it is the one
//! section of the settings form that is a small interaction of its own rather
//! than a row of fields.

use super::capture::{HotkeyAction, HotkeyCapture};
use super::form::{inline_error, subhead};
use crate::theme;
use egui::{RichText, TextEdit, Ui};
use hark_config::Settings;

/// The shortcut a fresh install ships with, and what "Reset" restores.
fn default_ptt_key() -> String {
    hark_config::Hotkey::default().ptt_key
}

/// Push-to-talk shortcut. The chord is *recorded*, not typed: clicking the
/// field captures held keys through the same low-level hook the pipeline uses
/// (egui's own input can't tell left from right modifiers or reliably see the
/// Win key), and the combo is committed on the first release — the rule every
/// push-to-talk app uses, because a PTT chord is usually modifier-only and a
/// recorder that waited for a "real" key could never record Ctrl+Win.
///
/// Typing the chord as text stays available behind a link, for the platforms
/// where recording is not wired up yet and for the day a hook refuses to
/// install; it is not the default because a hand-typed string is one nobody
/// has ever pressed.
///
/// Returns what the page must do about the pipeline: its hook has to stand
/// down before the recorder installs one, and come back afterwards.
pub fn section(ui: &mut Ui, draft: &mut Settings, capture: &mut HotkeyCapture) -> HotkeyAction {
    subhead(ui, "Push-to-talk");

    if capture.is_recording() {
        return recording_box(ui, draft, capture);
    }

    let mut action = HotkeyAction::None;
    ui.horizontal(|ui| {
        if capture.typing() {
            ui.add(
                TextEdit::singleline(&mut draft.hotkey.ptt_key)
                    .hint_text(default_ptt_key())
                    .desired_width(160.0),
            );
        } else {
            // The chord display is the record button: clicking the thing you
            // want to change is how every shortcut editor works, and it leaves
            // no way to read the field as an editable text box.
            let shortcut = hark_hotkey::pretty_chord(&draft.hotkey.ptt_key);
            if chord_display(ui, &shortcut).clicked() {
                action = HotkeyAction::StartRequested;
            }
        }
        // No button where recording cannot work: one that can only ever report
        // an error is worse than none at all.
        if hark_hotkey::capture_supported()
            && ui
                .button(if capture.typing() { "Record" } else { "Change" })
                .on_hover_text("Press and hold the keys you want; Hark captures them")
                .clicked()
        {
            action = HotkeyAction::StartRequested;
        }
        if draft.hotkey.ptt_key != default_ptt_key()
            && ui
                .button("Reset")
                .on_hover_text(format!(
                    "Back to {}",
                    hark_hotkey::pretty_chord(&default_ptt_key())
                ))
                .clicked()
        {
            draft.hotkey.ptt_key = default_ptt_key();
        }
    });

    if let Some(notice) = capture.notice() {
        inline_error(ui, notice);
    }
    match hark_hotkey::PttChord::parse(&draft.hotkey.ptt_key) {
        Ok(_) => {
            ui.label(
                RichText::new("Hold these keys together to dictate; release to inject.")
                    .small()
                    .weak(),
            );
        }
        Err(e) => inline_error(ui, &e.to_string()),
    }

    // Only offered where recording works; where it does not, the text field is
    // already the only path and a "type it instead" link would be nonsense.
    if hark_hotkey::capture_supported() {
        let link = if capture.typing() {
            "Record it instead"
        } else {
            "Type it instead"
        };
        if ui.link(RichText::new(link).small()).clicked() {
            capture.toggle_typing();
        }
    }
    action
}

/// The live prompt shown while the hook is capturing. Escape cancels, matching
/// every other shortcut recorder; the button is there for anyone who does not
/// know that.
fn recording_box(ui: &mut Ui, draft: &mut Settings, capture: &mut HotkeyCapture) -> HotkeyAction {
    // A completed chord lands straight in the draft and ends recording; the
    // idle field renders with the new value on the next pass.
    let mut action = capture.poll_into(&mut draft.hotkey.ptt_key);

    egui::Frame::default()
        .fill(theme::surface(ui.visuals()))
        .stroke(egui::Stroke::new(1.0, theme::accent(ui.visuals())))
        .corner_radius(8)
        .inner_margin(egui::Margin::symmetric(14, 11))
        .show(ui, |ui| {
            let held = capture.held_display();
            if held.is_empty() {
                ui.label(
                    RichText::new("Press and hold your shortcut keys...")
                        .text_style(theme::subheading()),
                );
            } else {
                ui.label(
                    RichText::new(held)
                        .text_style(theme::subheading())
                        .color(theme::accent(ui.visuals())),
                );
            }
            ui.label(
                RichText::new(
                    "Let go to set it. Modifier keys (Ctrl, Shift, Alt, Win), Caps Lock, \
                     and F1..F24 only — Esc to cancel.",
                )
                .small()
                .weak(),
            );
        });

    // Escape comes from egui, not the hook: it is not a chord-capable key, so
    // the recorder never sees it and it can never end up in a shortcut.
    let escaped = ui.input(|i| i.key_pressed(egui::Key::Escape));
    if (ui.button("Cancel").clicked() || escaped) && !matches!(action, HotkeyAction::Ended) {
        action = capture.cancel();
    }
    // Key edges wake the UI on their own (the capture pump). This slow tick
    // covers the case where they stop arriving: a hook Windows has dropped
    // sends nothing at all, and without a pass to notice, the prompt would ask
    // for keys forever. Runs only while this box is on screen.
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(250));
    action
}

/// The current chord rendered as a clickable field. Looks like the read-only
/// value it is, but senses clicks so the whole control is the record affordance.
fn chord_display(ui: &mut Ui, shortcut: &str) -> egui::Response {
    let response = ui.add_sized(
        [160.0, 24.0],
        egui::Button::new(RichText::new(shortcut))
            .fill(theme::surface(ui.visuals()))
            .stroke(ui.visuals().widgets.inactive.bg_stroke),
    );
    response.on_hover_text("Click to record a new shortcut")
}

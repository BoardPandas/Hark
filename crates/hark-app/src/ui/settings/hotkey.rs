//! The push-to-talk shortcut control. Lives beside `capture.rs` (which owns
//! the recording state) rather than in `form.rs`, because it is the one
//! section of the settings form that is a small interaction of its own rather
//! than a row of fields.

use super::capture::{HotkeyAction, HotkeyCapture};
use super::form::{inline_error, subhead};
use crate::pipeline::PipelineController;
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
/// Win key), and the combination is committed once every key has been let go.
/// Release-to-commit rather than the commit-on-first-non-modifier rule most
/// shortcut recorders use, because a PTT chord is usually modifier-only and
/// that rule could never record Ctrl+Win; all-released rather than
/// first-released, because a hand that lets go a few milliseconds apart would
/// otherwise record half the combination.
///
/// Typing the chord as text stays available behind a link, for the platforms
/// where recording is not wired up yet and for the day a hook refuses to
/// install; it is not the default because a hand-typed string is one nobody
/// has ever pressed.
///
/// Recording taps the running listener in place rather than stopping the
/// pipeline and installing a second hook, so dictation is never torn down to
/// change a shortcut and the recorder rides the code path dictation proves.
pub fn section(
    ui: &mut Ui,
    draft: &mut Settings,
    capture: &mut HotkeyCapture,
    pipeline: &mut PipelineController,
) -> HotkeyAction {
    subhead(ui, "Push-to-talk");

    if capture.is_recording() {
        return recording_box(ui, draft, capture, pipeline);
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
    // The recorder refuses a lone modifier outright, but the typed field has to
    // allow anything that parses -- it is the escape hatch. It must still say
    // what a bare Ctrl will do, because the consequence lands in every other app
    // and the fix has to be made in the one it just broke.
    match hark_hotkey::PttChord::parse(&draft.hotkey.ptt_key) {
        Ok(chord) => {
            let (text, color) = match chord.lone_modifier() {
                Some(key) => (
                    format!(
                        "{} on its own starts a dictation every time you press it, in \
                         every app. Add another key unless you really mean that.",
                        key.label()
                    ),
                    Some(theme::WARNING),
                ),
                None => (
                    "Hold these keys together to dictate; release to inject.".to_string(),
                    None,
                ),
            };
            let text = RichText::new(text).small();
            ui.label(match color {
                Some(c) => text.color(c),
                None => text.weak(),
            });
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
fn recording_box(
    ui: &mut Ui,
    draft: &mut Settings,
    capture: &mut HotkeyCapture,
    pipeline: &mut PipelineController,
) -> HotkeyAction {
    // A completed chord lands straight in the draft and ends recording; the
    // idle field renders with the new value on the next pass.
    let mut action = capture.poll_into(&mut draft.hotkey.ptt_key, pipeline);

    egui::Frame::default()
        .fill(theme::surface(ui.visuals()))
        .stroke(egui::Stroke::new(1.0, theme::accent(ui.visuals())))
        .corner_radius(8)
        .inner_margin(egui::Margin::symmetric(14, 11))
        .show(ui, |ui| {
            let held = capture.held_display();
            let nothing_held = held.is_empty();
            if nothing_held {
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
                    "Let go of everything to set it. Modifier keys (Ctrl, Shift, Alt, \
                     Win), Caps Lock, and F1..F24 only — Esc to cancel.",
                )
                .small()
                .weak(),
            );
            // A combination that cannot be a shortcut says why and lets the
            // user try again, rather than silently setting something else or
            // -- as it used to -- silently setting a bare modifier.
            if let Some(rejected) = capture.rejected() {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(rejected.message())
                        .small()
                        .color(theme::WARNING),
                );
            }
            // Keys the hook has actually reported. Shown because "I press keys
            // and nothing happens" has two very different causes — the hook
            // never sees them, or it sees them and the chord never lands — and
            // this is the one number that tells them apart without a log.
            let seen = capture.edges_seen();
            if nothing_held && seen > 0 {
                ui.label(
                    RichText::new(format!("{seen} key events seen"))
                        .small()
                        .weak(),
                );
            }
        });

    // Escape comes from egui, not the hook: it is not a chord-capable key, so
    // the recorder never sees it and it can never end up in a shortcut.
    let escaped = ui.input(|i| i.key_pressed(egui::Key::Escape));
    if (ui.button("Cancel").clicked() || escaped) && !matches!(action, HotkeyAction::Ended) {
        action = capture.cancel(pipeline);
    }
    // Pressing a key with Hark focused wakes winit on its own, so this tick is
    // a backstop rather than the mechanism: it covers a chord recorded while
    // the window is not focused, and a hook Windows has dropped (which sends
    // nothing at all, so without a pass nothing would ever notice).
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

//! One history row: two-line final-text preview, caption (relative time ·
//! voice · model), always-visible copy/delete, and the expanded detail
//! (raw transcript, timing breakdown, full timestamp).

use crate::theme;
use crate::ui::format;
use egui::{Align, Frame, Id, Label, Layout, Margin, RichText, Sense, Ui};
use hark_store::Entry;
use jiff::tz::TimeZone;

/// What the user did to a row this frame; the page applies it after the
/// list loop (it cannot mutate state while iterating the cache).
pub enum Action {
    Toggle(i64),
    Copy(i64),
    Delete(i64),
}

/// Animation key for a row's "Copied" fade.
pub fn copied_id(entry_id: i64) -> Id {
    Id::new(("history-copied", entry_id))
}

/// Preview length: roughly two wrapped lines at the list column width.
const PREVIEW_CHARS: usize = 160;
/// Room reserved for the copy/delete buttons and the "Copied" affirmation.
const ACTIONS_WIDTH: f32 = 96.0;

pub fn show(
    ui: &mut Ui,
    entry: &Entry,
    expanded: bool,
    copied: bool,
    now_ms: i64,
    tz: &TimeZone,
) -> Option<Action> {
    let mut action = None;
    ui.horizontal(|ui| {
        let text_width = (ui.available_width() - ACTIONS_WIDTH).max(120.0);
        ui.vertical(|ui| {
            ui.set_width(text_width);
            let preview =
                Label::new(RichText::new(preview(&entry.final_text))).sense(Sense::click());
            let response =
                ui.add(preview)
                    .on_hover_text(if expanded { "Collapse" } else { "Show details" });
            if response.clicked() {
                action = Some(Action::Toggle(entry.id));
            }
            ui.label(RichText::new(caption(entry, now_ms)).small().weak());
        });
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            if ui
                .button(RichText::new(theme::icons::TRASH))
                .on_hover_text("Delete entry")
                .clicked()
            {
                action = Some(Action::Delete(entry.id));
            }
            if ui
                .button(RichText::new(theme::icons::COPY))
                .on_hover_text("Copy text")
                .clicked()
            {
                action = Some(Action::Copy(entry.id));
            }
            if copied {
                // Fades over ~800 ms; reads correctly with animation ignored
                // too, since the affirmation simply stays until it clears.
                let alpha = ui
                    .ctx()
                    .animate_value_with_time(copied_id(entry.id), 0.0, 0.8);
                if alpha > 0.02 {
                    ui.label(
                        RichText::new(format!("{} Copied", theme::icons::CHECK))
                            .small()
                            .color(theme::SUCCESS.gamma_multiply(alpha)),
                    );
                }
            }
        });
    });
    if expanded {
        details(ui, entry, tz);
    }
    // The Nocturne signature: a 1px rule fading to transparent at both ends,
    // in an 8px strip that gives each row its breathing room.
    theme::fading_rule(ui, 8.0);
    action
}

/// Expanded detail (a surface panel): the raw transcript exactly as the
/// provider returned it, the timing breakdown, and the full timestamp with
/// the provider label (disappointing output must have an obvious cause).
fn details(ui: &mut Ui, entry: &Entry, tz: &TimeZone) {
    ui.add_space(4.0);
    Frame::default()
        .fill(theme::surface(ui.visuals()))
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(8)
        .inner_margin(Margin {
            left: 14,
            right: 14,
            top: 11,
            bottom: 11,
        })
        .show(ui, |ui| {
            // Naming the trigger explains why the entry reads nothing like the
            // raw transcript directly below it.
            if let Some(trigger) = &entry.invocation {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(theme::icons::LIGHTNING)
                            .small()
                            .color(theme::accent(ui.visuals())),
                    );
                    ui.label(
                        RichText::new(format!("Invocation \u{201C}{trigger}\u{201D} fired"))
                            .small()
                            .weak(),
                    );
                });
                ui.add_space(2.0);
            }
            ui.label(RichText::new("RAW TRANSCRIPT").size(10.5).weak());
            ui.label(RichText::new(entry.raw_text.trim()).monospace());
            ui.add_space(4.0);
            ui.label(RichText::new(timing_line(entry)).monospace().small());
            ui.label(
                RichText::new(format!(
                    "{} · {}",
                    format::full_timestamp(entry.ts_ms, tz),
                    entry.stt_provider
                ))
                .small()
                .weak(),
            );
        });
}

/// "4m ago · clean · nova-3", with a leading "⚡ Invocation" segment when the
/// text was pasted from an invocation rather than transcribed. Without it a
/// row of canned text looks like a suspiciously articulate dictation.
fn caption(entry: &Entry, now_ms: i64) -> String {
    let base = format!(
        "{} · {} · {}",
        format::relative_time(entry.ts_ms, now_ms),
        entry.voice,
        entry.stt_model
    );
    match entry.invocation {
        // Icon + label, never color alone (design system rule).
        Some(_) => format!("{} Invocation · {base}", theme::icons::LIGHTNING),
        None => base,
    }
}

/// Whitespace-normalized, char-boundary-safe preview with an ellipsis.
fn preview(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(PREVIEW_CHARS) {
        Some((byte, _)) => format!("{}…", flat[..byte].trim_end()),
        None => flat,
    }
}

/// "stt 400 ms · cleanup 300 ms (gpt-5-nano) · total 800 ms"; the cleanup
/// segment appears only when a cleanup response actually shaped the text.
fn timing_line(entry: &Entry) -> String {
    match (entry.cleanup_ms, &entry.cleanup_model) {
        (Some(ms), Some(model)) => format!(
            "stt {} ms · cleanup {} ms ({model}) · total {} ms",
            entry.stt_ms, ms, entry.total_ms
        ),
        _ => format!("stt {} ms · total {} ms", entry.stt_ms, entry.total_ms),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(cleanup: bool) -> Entry {
        Entry {
            id: 1,
            ts_ms: 1_000,
            raw_text: "raw".to_string(),
            final_text: "final".to_string(),
            voice: "clean".to_string(),
            stt_provider: "deepgram".to_string(),
            stt_model: "nova-3".to_string(),
            cleanup_model: cleanup.then(|| "gpt-5-nano".to_string()),
            invocation: None,
            stt_ms: 400,
            cleanup_ms: cleanup.then_some(300),
            total_ms: 800,
        }
    }

    #[test]
    fn preview_flattens_whitespace_and_truncates_on_char_boundaries() {
        assert_eq!(preview("one\ntwo\t three"), "one two three");
        let short = "word ".repeat(4);
        assert_eq!(preview(&short), "word word word word");

        let long = "abcde ".repeat(60);
        let cut = preview(&long);
        assert!(cut.ends_with('…'));
        assert!(cut.chars().count() <= PREVIEW_CHARS + 1);

        // Multi-byte characters never split a boundary.
        let emoji = "🎤".repeat(PREVIEW_CHARS + 20);
        let cut = preview(&emoji);
        assert!(cut.ends_with('…'));
        assert_eq!(cut.chars().count(), PREVIEW_CHARS + 1);
    }

    #[test]
    fn caption_badges_invocations_and_leaves_ordinary_rows_alone() {
        let plain = entry(false);
        let caption_plain = caption(&plain, plain.ts_ms);
        assert!(
            !caption_plain.contains("Invocation"),
            "an ordinary dictation must not be badged: {caption_plain}"
        );
        assert!(caption_plain.contains("clean") && caption_plain.contains("nova-3"));

        let mut fired = entry(false);
        fired.invocation = Some("access granted".to_string());
        let badged = caption(&fired, fired.ts_ms);
        assert!(badged.starts_with(theme::icons::LIGHTNING), "{badged}");
        assert!(badged.contains("Invocation"));
        // The badge is additive: the usual segments still read the same.
        assert!(badged.ends_with(&caption_plain), "{badged}");
    }

    #[test]
    fn timing_line_names_the_cleanup_model_only_when_it_ran() {
        assert_eq!(
            timing_line(&entry(true)),
            "stt 400 ms · cleanup 300 ms (gpt-5-nano) · total 800 ms"
        );
        assert_eq!(timing_line(&entry(false)), "stt 400 ms · total 800 ms");
    }
}

//! The banner at the top of History for a dictation whose words did not make
//! it to the cursor, and the one-slot state behind it.
//!
//! Two ways to get here, both ending in the same place so the words are never
//! just gone: the text was transcribed but the paste failed (the text is kept,
//! Copy puts it on the clipboard), or the transcription itself failed and the
//! pipeline is holding the audio in memory (Try again asks it to retry; the
//! result comes back as text, never pasted, because the cursor is no longer
//! where the dictation was aimed).
//!
//! The slot lives in memory only and holds the *last* such dictation. It is
//! shared with the page through the egui context, because the History page is
//! built by code that owns neither the pipeline nor its events.

use crate::theme;
use egui::{Id, RichText, Ui};
use hark_pipeline::PipelineEvent;
use std::sync::{Arc, Mutex, MutexGuard};

/// The last dictation that did not arrive. Text content, so no `Debug`.
#[derive(Clone, PartialEq, Eq)]
pub enum Undelivered {
    /// Transcribed, not pasted. Holds the final text for Copy.
    Text(String),
    /// The pipeline is holding the audio. `retrying` while a retry runs.
    Audio { retrying: bool },
}

/// What the page asks of the controller, which owns the pipeline handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    Retry,
    Dismiss,
}

#[derive(Default)]
struct State {
    slot: Option<Undelivered>,
    request: Option<Request>,
}

/// The shared slot. Cheap to lock: touched on events and clicks, never held
/// across a frame.
#[derive(Default)]
pub struct Recovery {
    state: Mutex<State>,
}

impl Recovery {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// Keep the slot in step with the pipeline. The worker states every change
    /// to what it holds, so nothing here guesses from other events.
    pub fn observe(&self, event: &PipelineEvent) {
        let mut state = self.lock();
        match event {
            PipelineEvent::NotPasted { record, .. } | PipelineEvent::Recovered(record) => {
                state.slot = Some(Undelivered::Text(record.final_text.clone()));
            }
            PipelineEvent::ClipHeld { held: true } => {
                state.slot = Some(Undelivered::Audio { retrying: false });
            }
            PipelineEvent::ClipHeld { held: false } => {
                // Only an audio banner is about the clip; a text banner stays
                // until the user is done with it.
                if matches!(state.slot, Some(Undelivered::Audio { .. })) {
                    state.slot = None;
                }
            }
            _ => {}
        }
    }

    pub fn snapshot(&self) -> Option<Undelivered> {
        self.lock().slot.clone()
    }

    /// The user pressed Try again. Ignored unless there is an idle clip.
    pub fn retry(&self) {
        let mut state = self.lock();
        if matches!(state.slot, Some(Undelivered::Audio { retrying: false })) {
            state.slot = Some(Undelivered::Audio { retrying: true });
            state.request = Some(Request::Retry);
        }
    }

    /// The user pressed Dismiss. Ignored mid-retry: the answer is on its way
    /// and the button is disabled, so the request cannot be ambiguous.
    pub fn dismiss(&self) {
        let mut state = self.lock();
        match state.slot {
            Some(Undelivered::Audio { retrying: true }) | None => {}
            Some(Undelivered::Audio { retrying: false }) => {
                state.slot = None;
                state.request = Some(Request::Dismiss);
            }
            Some(Undelivered::Text(_)) => state.slot = None,
        }
    }

    /// The controller's pickup: what the page asked since the last frame.
    pub fn take_request(&self) -> Option<Request> {
        self.lock().request.take()
    }

    /// The pipeline stopped, and a clip it held went with it. Text outlives
    /// the pipeline; audio does not.
    pub fn pipeline_stopped(&self) {
        let mut state = self.lock();
        if matches!(state.slot, Some(Undelivered::Audio { .. })) {
            state.slot = None;
        }
        state.request = None;
    }
}

fn context_id() -> Id {
    Id::unique("hark-undelivered-dictation")
}

/// Make the slot reachable from the History page. Called when the pipeline
/// starts, which is where the controller first sees the context.
pub fn register(ctx: &egui::Context, recovery: &Arc<Recovery>) {
    ctx.data_mut(|data| data.insert_temp(context_id(), Arc::clone(recovery)));
}

/// Draw the banner, if there is anything to say. A no-op otherwise.
pub fn show(ui: &mut Ui) {
    let Some(recovery) = ui
        .ctx()
        .data(|data| data.get_temp::<Arc<Recovery>>(context_id()))
    else {
        return;
    };
    let Some(slot) = recovery.snapshot() else {
        return;
    };
    let (message, detail) = match &slot {
        Undelivered::Text(text) => (
            "Your last dictation could not be pasted.",
            Some(preview(text)),
        ),
        Undelivered::Audio { retrying: false } => {
            ("Your last dictation could not be transcribed.", None)
        }
        Undelivered::Audio { retrying: true } => ("Transcribing your last dictation again…", None),
    };
    theme::card(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(theme::icon_text(theme::icons::WARNING).color(theme::warning(ui.visuals())));
            ui.label(RichText::new(message).strong());
        });
        if let Some(detail) = detail {
            ui.label(RichText::new(detail).small().weak());
        }
        ui.add_space(theme::GAP);
        ui.horizontal(|ui| match &slot {
            Undelivered::Text(text) => {
                if ui
                    .add(theme::primary_button(ui.visuals(), "Copy text"))
                    .clicked()
                {
                    ui.ctx().copy_text(text.clone());
                }
                if ui.button("Dismiss").clicked() {
                    recovery.dismiss();
                }
            }
            Undelivered::Audio { retrying } => {
                if *retrying {
                    ui.spinner();
                }
                if ui
                    .add_enabled(
                        !retrying,
                        egui::Button::new("Try again")
                            .min_size(egui::vec2(0.0, theme::CONTROL_HEIGHT)),
                    )
                    .clicked()
                {
                    recovery.retry();
                }
                if ui
                    .add_enabled(!retrying, egui::Button::new("Dismiss"))
                    .clicked()
                {
                    recovery.dismiss();
                }
            }
        });
    });
    ui.add_space(theme::ROW_GAP);
}

/// A short, single-line look at the text so the user can tell which dictation
/// this is. The full text goes out through Copy.
fn preview(text: &str) -> String {
    const CHARS: usize = 160;
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(CHARS) {
        Some((byte, _)) => format!("{}…", flat[..byte].trim_end()),
        None => flat,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hark_pipeline::DictationRecord;

    fn record(text: &str) -> Box<DictationRecord> {
        Box::new(DictationRecord {
            raw_text: text.to_string(),
            final_text: text.to_string(),
            voice: "verbatim".to_string(),
            stt_provider: "deepgram".to_string(),
            stt_model: "nova-3".to_string(),
            cleanup_model: None,
            invocation: None,
            audio_ms: 900,
            stt_ms: 300,
            cleanup_ms: None,
            total_ms: 450,
            spellbook_replacements: Some(0),
            foreground_app: None,
        })
    }

    fn not_pasted(text: &str) -> PipelineEvent {
        PipelineEvent::NotPasted {
            record: record(text),
            detail: "no focus".to_string(),
        }
    }

    fn is_text(recovery: &Recovery, expected: &str) -> bool {
        matches!(recovery.snapshot(), Some(Undelivered::Text(t)) if t == expected)
    }

    #[test]
    fn a_failed_paste_becomes_the_text_slot() {
        let recovery = Recovery::default();
        assert!(recovery.snapshot().is_none());
        recovery.observe(&not_pasted("kept words"));
        assert!(is_text(&recovery, "kept words"));
    }

    #[test]
    fn a_held_clip_becomes_the_audio_slot_and_replaces_older_text() {
        let recovery = Recovery::default();
        recovery.observe(&not_pasted("older"));
        recovery.observe(&PipelineEvent::ClipHeld { held: true });
        assert!(matches!(
            recovery.snapshot(),
            Some(Undelivered::Audio { retrying: false })
        ));
    }

    #[test]
    fn retry_shows_busy_queues_one_request_and_cannot_be_pressed_twice() {
        let recovery = Recovery::default();
        recovery.observe(&PipelineEvent::ClipHeld { held: true });
        recovery.retry();
        recovery.retry();
        assert!(matches!(
            recovery.snapshot(),
            Some(Undelivered::Audio { retrying: true })
        ));
        assert_eq!(recovery.take_request(), Some(Request::Retry));
        assert_eq!(recovery.take_request(), None, "one press, one request");
    }

    #[test]
    fn a_successful_retry_turns_the_audio_slot_into_text() {
        let recovery = Recovery::default();
        recovery.observe(&PipelineEvent::ClipHeld { held: true });
        recovery.retry();
        recovery.observe(&PipelineEvent::Recovered(record("recovered words")));
        assert!(is_text(&recovery, "recovered words"));
    }

    #[test]
    fn a_failed_or_declined_retry_goes_back_to_idle_audio() {
        let recovery = Recovery::default();
        recovery.observe(&PipelineEvent::ClipHeld { held: true });
        recovery.retry();
        recovery.observe(&PipelineEvent::ClipHeld { held: true });
        assert!(matches!(
            recovery.snapshot(),
            Some(Undelivered::Audio { retrying: false })
        ));
    }

    #[test]
    fn dismissing_audio_asks_the_worker_to_drop_it_and_dismissing_text_asks_nothing() {
        let recovery = Recovery::default();
        recovery.observe(&PipelineEvent::ClipHeld { held: true });
        recovery.dismiss();
        assert!(recovery.snapshot().is_none());
        assert_eq!(recovery.take_request(), Some(Request::Dismiss));

        recovery.observe(&not_pasted("words"));
        recovery.dismiss();
        assert!(recovery.snapshot().is_none());
        assert_eq!(recovery.take_request(), None);
    }

    #[test]
    fn dismiss_is_ignored_while_a_retry_runs() {
        let recovery = Recovery::default();
        recovery.observe(&PipelineEvent::ClipHeld { held: true });
        recovery.retry();
        recovery.take_request();
        recovery.dismiss();
        assert!(matches!(
            recovery.snapshot(),
            Some(Undelivered::Audio { retrying: true })
        ));
        assert_eq!(recovery.take_request(), None);
    }

    #[test]
    fn the_worker_dropping_the_clip_clears_audio_but_never_text() {
        let recovery = Recovery::default();
        recovery.observe(&PipelineEvent::ClipHeld { held: true });
        recovery.observe(&PipelineEvent::ClipHeld { held: false });
        assert!(recovery.snapshot().is_none());

        recovery.observe(&not_pasted("still wanted"));
        recovery.observe(&PipelineEvent::ClipHeld { held: false });
        assert!(is_text(&recovery, "still wanted"));
    }

    #[test]
    fn stopping_the_pipeline_loses_the_clip_but_keeps_the_text() {
        let recovery = Recovery::default();
        recovery.observe(&PipelineEvent::ClipHeld { held: true });
        recovery.retry();
        recovery.pipeline_stopped();
        assert!(recovery.snapshot().is_none());
        assert_eq!(
            recovery.take_request(),
            None,
            "no request outlives the pipeline"
        );

        recovery.observe(&not_pasted("kept"));
        recovery.pipeline_stopped();
        assert!(is_text(&recovery, "kept"));
    }

    #[test]
    fn retry_and_dismiss_do_nothing_with_nothing_to_act_on() {
        let recovery = Recovery::default();
        recovery.retry();
        recovery.dismiss();
        assert_eq!(recovery.take_request(), None);
        recovery.observe(&not_pasted("words"));
        recovery.retry();
        assert_eq!(recovery.take_request(), None, "text has no retry");
    }

    #[test]
    fn the_preview_is_one_short_line() {
        assert_eq!(preview("one\ntwo   three"), "one two three");
        let long = "word ".repeat(100);
        let cut = preview(&long);
        assert!(cut.ends_with('…'));
        assert!(cut.chars().count() <= 161, "one line, not the whole text");
    }
}

//! "Record a shortcut" state for the push-to-talk field. egui's high-level
//! input cannot tell left from right modifiers and does not reliably see the
//! Win key, so recording rides the same low-level hook the pipeline uses
//! (`hark_hotkey::spawn_capture`). A dedicated pump thread wakes the event
//! loop per key edge (the sanctioned cross-thread wake-up), so the UI drains
//! edges without ever polling the main thread.

use egui::Context;
use hark_hotkey::{CaptureBuffer, CaptureEvent, ListenerHandle};
use std::sync::mpsc::{self, Receiver};

/// What a render of the hotkey section did to the recording state. The page
/// reacts by pausing/resuming the pipeline: only one keyboard hook may run at
/// a time, so the push-to-talk listener stands down while recording.
pub enum CaptureTransition {
    /// Nothing changed this frame.
    None,
    /// Recording just began: pause the pipeline's hook.
    Started,
    /// Recording just finished or was cancelled: resume the pipeline.
    Ended,
}

/// An in-progress recording: the live hook plus the chord being built.
struct Recording {
    /// Dropping the handle posts WM_QUIT to the hook thread, which unhooks and
    /// exits; the pump then sees its sender drop and exits too.
    _handle: ListenerHandle,
    edges: Receiver<CaptureEvent>,
    buffer: CaptureBuffer,
}

impl Recording {
    fn start(ctx: &Context) -> Result<Recording, String> {
        let (hook_tx, hook_rx) = mpsc::channel();
        let (ui_tx, ui_rx) = mpsc::channel();
        let handle = hark_hotkey::spawn_capture(hook_tx).map_err(|e| e.to_string())?;

        // Forward every edge to the UI lane and wake the event loop, mirroring
        // pipeline::spawn_repaint_pump. Exits when the hook thread drops its
        // sender (recording stopped) or the UI drops its receiver. Idle cost is
        // zero: key edges are sparse and user-driven.
        let ctx = ctx.clone();
        std::thread::Builder::new()
            .name("hark-ptt-capture-pump".to_string())
            .spawn(move || {
                while let Ok(edge) = hook_rx.recv() {
                    if ui_tx.send(edge).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            })
            .expect("spawning the capture pump thread cannot fail");

        Ok(Recording {
            _handle: handle,
            edges: ui_rx,
            buffer: CaptureBuffer::new(),
        })
    }

    /// Drain pending edges. `Some(chord)` once the user completes a chord.
    fn poll(&mut self) -> Option<String> {
        while let Ok(edge) = self.edges.try_recv() {
            if let Some(chord) = self.buffer.on_event(edge.key, edge.down) {
                return Some(chord.to_string());
            }
        }
        None
    }
}

/// The push-to-talk section's cross-frame state: an optional live recording and
/// the notice from the last failed attempt (e.g. recording is not wired up on
/// this platform yet).
#[derive(Default)]
pub struct HotkeyCapture {
    recording: Option<Recording>,
    /// Shown until the next action; e.g. "Recording isn't available here yet".
    notice: Option<String>,
}

impl HotkeyCapture {
    pub fn new() -> HotkeyCapture {
        HotkeyCapture::default()
    }

    pub fn is_recording(&self) -> bool {
        self.recording.is_some()
    }

    /// A failed record attempt to surface under the field, if any.
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    /// Start recording. Returns `Started` on success; on failure it leaves a
    /// notice and reports `None` so the pipeline keeps running.
    pub fn begin(&mut self, ctx: &Context) -> CaptureTransition {
        match Recording::start(ctx) {
            Ok(rec) => {
                self.recording = Some(rec);
                self.notice = None;
                CaptureTransition::Started
            }
            Err(detail) => {
                self.notice = Some(format!("Can't record a shortcut here: {detail}"));
                CaptureTransition::None
            }
        }
    }

    /// Stop recording without setting a chord.
    pub fn cancel(&mut self) -> CaptureTransition {
        if self.recording.take().is_some() {
            CaptureTransition::Ended
        } else {
            CaptureTransition::None
        }
    }

    /// Drain edges. When the user completes a chord, write it to `target`,
    /// stop recording, and report `Ended`.
    pub fn poll_into(&mut self, target: &mut String) -> CaptureTransition {
        let done = self.recording.as_mut().and_then(|rec| rec.poll());
        if let Some(chord) = done {
            *target = chord;
            self.recording = None;
            CaptureTransition::Ended
        } else {
            CaptureTransition::None
        }
    }

    /// Live "LCtrl + LWin" of the keys held so far, for the recording prompt.
    pub fn held_display(&self) -> String {
        let Some(rec) = &self.recording else {
            return String::new();
        };
        rec.buffer
            .held()
            .iter()
            .map(|k| k.to_string())
            .collect::<Vec<_>>()
            .join(" + ")
    }
}

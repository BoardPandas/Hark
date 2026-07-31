//! "Record a shortcut" state for the push-to-talk field. egui's high-level
//! input cannot tell left from right modifiers and does not reliably see the
//! Win key, so recording rides the same low-level hook the pipeline uses
//! (`hark_hotkey::spawn_capture`). A dedicated pump thread wakes the event
//! loop per key edge (the sanctioned cross-thread wake-up), so the UI drains
//! edges without ever polling the main thread.

use egui::Context;
use hark_hotkey::{CaptureBuffer, CaptureEvent, ListenerHandle};
use std::sync::mpsc::{self, Receiver};

/// What a render of the hotkey section asks the page to do. The page owns the
/// pipeline, and the pipeline owns the push-to-talk hook: only one keyboard
/// hook may run at a time, so the listener stands down while recording.
pub enum HotkeyAction {
    /// Nothing changed this frame.
    None,
    /// The user asked to record. The page must stop the pipeline's hook and
    /// *then* call [`HotkeyCapture::begin`] — never the other way round.
    /// Teardown posts a quit message to the listener thread's id, and a
    /// capture hook installed before that message lands can be killed by it
    /// once Windows recycles the id onto the new thread.
    StartRequested,
    /// Recording just finished, was cancelled, or died: resume the pipeline.
    Ended,
}

/// An in-progress recording: the live hook plus the chord being built.
struct Recording {
    /// Dropping the handle posts WM_QUIT to the hook thread, which unhooks and
    /// exits; the pump then sees its sender drop and exits too.
    handle: ListenerHandle,
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
                    crate::app::wake_ui(&ctx);
                }
            })
            .expect("spawning the capture pump thread cannot fail");

        Ok(Recording {
            handle,
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
    /// Whether the typed-chord escape hatch is showing. Off by default: the
    /// recorder is the way to set a shortcut, and a chord typed by hand is a
    /// string nobody validates against a real keyboard. It exists because a
    /// recorder that cannot install its hook would otherwise leave no way at
    /// all to change the shortcut.
    typing: bool,
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

    /// Is the typed-chord fallback showing? Always true where the platform
    /// cannot record, so there is never a state with no way to set a shortcut.
    pub fn typing(&self) -> bool {
        self.typing || !hark_hotkey::capture_supported()
    }

    pub fn toggle_typing(&mut self) {
        self.typing = !self.typing;
        self.notice = None;
    }

    /// Start recording. Returns whether it began; on failure it leaves a
    /// notice and reveals the typed fallback, so the user is never stuck.
    pub fn begin(&mut self, ctx: &Context) -> bool {
        match Recording::start(ctx) {
            Ok(rec) => {
                self.recording = Some(rec);
                self.notice = None;
                true
            }
            Err(detail) => {
                self.notice = Some(format!("Can't record a shortcut here: {detail}"));
                self.typing = true;
                false
            }
        }
    }

    /// Stop recording without setting a chord.
    pub fn cancel(&mut self) -> HotkeyAction {
        if self.recording.take().is_some() {
            HotkeyAction::Ended
        } else {
            HotkeyAction::None
        }
    }

    /// Drain edges. When the user completes a chord, write it to `target`,
    /// stop recording, and report `Ended`. A hook that died on its own ends
    /// recording too, with the reason on screen: the alternative is a prompt
    /// that asks for keys forever and never answers.
    pub fn poll_into(&mut self, target: &mut String) -> HotkeyAction {
        let Some(rec) = self.recording.as_mut() else {
            return HotkeyAction::None;
        };
        if let Some(chord) = rec.poll() {
            *target = chord;
            self.recording = None;
            return HotkeyAction::Ended;
        }
        if !rec.handle.is_alive() {
            log::warn!("shortcut recording ended: the keyboard hook stopped on its own");
            self.notice =
                Some("Recording stopped: Windows dropped Hark's keyboard hook.".to_string());
            self.typing = true;
            self.recording = None;
            return HotkeyAction::Ended;
        }
        HotkeyAction::None
    }

    /// Live "Left Ctrl + Left Win" of the keys held so far, for the prompt.
    pub fn held_display(&self) -> String {
        let Some(rec) = &self.recording else {
            return String::new();
        };
        rec.buffer
            .held()
            .iter()
            .map(|k| k.label())
            .collect::<Vec<_>>()
            .join(" + ")
    }
}

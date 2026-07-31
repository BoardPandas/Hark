//! "Record a shortcut" state for the push-to-talk field. egui's high-level
//! input cannot tell left from right modifiers and does not reliably see the
//! Win key, so recording watches real key edges from the platform hook.
//!
//! It rides the *pipeline's* hook rather than installing one of its own. A
//! second `WH_KEYBOARD_LL` hook installs, reports itself alive and pumps
//! messages, yet is never called on real hardware, while the push-to-talk hook
//! — same code, same thread shape — delivers every key; a build that logged
//! both showed the capture hook installed for 51 seconds without a single
//! callback while dictation through the other hook worked in the same session.
//! Tapping the working hook sidesteps that whole question, and costs one
//! relaxed atomic load per key event.
//!
//! Installing a hook is still the fallback for the one case with nothing to
//! tap: a stopped pipeline (no API key yet, or a config error).

use crate::pipeline::PipelineController;
use egui::Context;
use hark_hotkey::{CaptureBuffer, CaptureEvent, CaptureTap, ListenerHandle};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;

/// What a render of the hotkey section asks the page to do.
pub enum HotkeyAction {
    /// Nothing changed this frame.
    None,
    /// The user asked to record; the page calls [`HotkeyCapture::begin`].
    StartRequested,
    /// Recording just finished, was cancelled, or died.
    Ended,
}

/// Where the raw key edges are coming from.
enum Source {
    /// The pipeline's own listener, tapped in place. Nothing was started and
    /// nothing needs stopping; dictation keeps working the instant we disarm.
    Tap(Arc<CaptureTap>),
    /// A hook of our own, because the pipeline is stopped. Dropping the handle
    /// posts WM_QUIT to the hook thread, which unhooks and exits.
    OwnHook(ListenerHandle),
}

/// An in-progress recording: the edge source plus the chord being built.
struct Recording {
    source: Source,
    /// `Option` only so the receiver can be handed back on disarm; always
    /// `Some` while recording.
    edges: Option<Receiver<CaptureEvent>>,
    buffer: CaptureBuffer,
}

impl Recording {
    /// Drain pending edges. `Some(chord)` once the user completes a chord.
    fn poll(&mut self) -> Option<String> {
        let edges = self.edges.as_ref()?;
        while let Ok(edge) = edges.try_recv() {
            if let Some(chord) = self.buffer.on_event(edge.key, edge.down) {
                return Some(chord.to_string());
            }
        }
        None
    }

    /// Key edges the source has actually seen. Zero while the user swears they
    /// are pressing keys means the hook is not being called at all — the one
    /// fact that separates a dead hook from a lost message, and the reason it
    /// is on screen rather than only in the log.
    fn edges_seen(&self) -> u64 {
        match &self.source {
            Source::Tap(tap) => tap.edges_seen(),
            // A hook of our own has no counter; the held keys are the signal.
            Source::OwnHook(_) => 0,
        }
    }

    fn is_alive(&self) -> bool {
        match &self.source {
            Source::Tap(_) => true,
            Source::OwnHook(handle) => handle.is_alive(),
        }
    }
}

/// The push-to-talk section's cross-frame state.
#[derive(Default)]
pub struct HotkeyCapture {
    recording: Option<Recording>,
    /// Shown until the next action; e.g. "Recording isn't available here yet".
    notice: Option<String>,
    /// Whether the typed-chord escape hatch is showing. Off by default: a chord
    /// typed by hand is a string nobody has validated against a real keyboard.
    typing: bool,
}

impl HotkeyCapture {
    pub fn new() -> HotkeyCapture {
        HotkeyCapture::default()
    }

    pub fn is_recording(&self) -> bool {
        self.recording.is_some()
    }

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

    /// Start recording. Taps the running listener when there is one; otherwise
    /// stops the (already stopped) pipeline's place in the world and installs a
    /// hook. Returns whether recording began.
    pub fn begin(&mut self, ctx: &Context, pipeline: &mut PipelineController) -> bool {
        if let Some((tap, rx)) = pipeline.arm_capture() {
            log::info!("shortcut recording: tapping the live push-to-talk hook");
            self.recording = Some(Recording {
                source: Source::Tap(tap),
                edges: Some(rx),
                buffer: CaptureBuffer::new(),
            });
            self.notice = None;
            return true;
        }

        log::info!("shortcut recording: pipeline is stopped, installing a capture hook");
        let (hook_tx, hook_rx) = mpsc::channel();
        match hark_hotkey::spawn_capture(hook_tx) {
            Ok(handle) => {
                self.recording = Some(Recording {
                    source: Source::OwnHook(handle),
                    edges: Some(hook_rx),
                    buffer: CaptureBuffer::new(),
                });
                self.notice = None;
                // Nothing wakes the UI for us on this path, and the recording
                // box repaints on a slow tick; a keypress with Hark focused
                // also wakes winit, so this is a backstop, not the mechanism.
                ctx.request_repaint();
                true
            }
            Err(e) => {
                self.notice = Some(format!("Can't record a shortcut here: {e}"));
                self.typing = true;
                false
            }
        }
    }

    /// Stop recording without setting a chord.
    pub fn cancel(&mut self, pipeline: &mut PipelineController) -> HotkeyAction {
        match self.recording.take() {
            Some(rec) => {
                self.release(rec, pipeline);
                HotkeyAction::Ended
            }
            None => HotkeyAction::None,
        }
    }

    /// Drain edges. When the user completes a chord, write it to `target` and
    /// report `Ended`. A hook that died on its own ends recording too, with the
    /// reason on screen rather than a prompt that asks for keys forever.
    pub fn poll_into(
        &mut self,
        target: &mut String,
        pipeline: &mut PipelineController,
    ) -> HotkeyAction {
        let Some(rec) = self.recording.as_mut() else {
            return HotkeyAction::None;
        };
        if let Some(chord) = rec.poll() {
            log::info!("shortcut recorded after {} key edges", rec.edges_seen());
            *target = chord;
            let rec = self.recording.take().expect("checked just above");
            self.release(rec, pipeline);
            return HotkeyAction::Ended;
        }
        if !rec.is_alive() {
            log::warn!("shortcut recording ended: the keyboard hook stopped on its own");
            self.notice =
                Some("Recording stopped: Windows dropped Hark's keyboard hook.".to_string());
            self.typing = true;
            let rec = self.recording.take().expect("checked just above");
            self.release(rec, pipeline);
            return HotkeyAction::Ended;
        }
        HotkeyAction::None
    }

    /// Give the key stream back. For a tap that hands the receiver to the
    /// listener so the chord tracker resumes; an owned hook simply drops.
    fn release(&mut self, mut rec: Recording, pipeline: &mut PipelineController) {
        if let (Source::Tap(_), Some(rx)) = (&rec.source, rec.edges.take()) {
            pipeline.disarm_capture(rx);
        }
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

    /// Key edges the hook has reported since recording began.
    pub fn edges_seen(&self) -> u64 {
        self.recording.as_ref().map_or(0, |r| r.edges_seen())
    }
}

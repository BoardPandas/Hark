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
use hark_hotkey::{
    CaptureBuffer, CaptureCounts, CaptureEvent, CaptureTap, ListenerHandle, Rejected,
};
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
    /// The edges are drained through the pipeline, because the receiver stays
    /// with the listener that owns it — handing it back and forth let a restart
    /// mid-recording poison the next listener with a dead channel.
    Tap(Arc<CaptureTap>),
    /// A hook of our own, because the pipeline is stopped. Dropping the handle
    /// posts WM_QUIT to the hook thread, which unhooks and exits.
    OwnHook {
        handle: ListenerHandle,
        edges: Receiver<CaptureEvent>,
    },
}

/// An in-progress recording: the edge source plus the chord being built.
struct Recording {
    source: Source,
    buffer: CaptureBuffer,
}

impl Recording {
    /// Feed every pending edge to the buffer. `Some(chord)` once one completes.
    fn poll(&mut self, pipeline: &PipelineController) -> Option<String> {
        let mut done = None;
        let buffer = &mut self.buffer;
        let mut feed = |edge: CaptureEvent| {
            if done.is_none() {
                if let Some(chord) = buffer.on_event(edge.key, edge.down) {
                    done = Some(chord.to_string());
                }
            }
        };
        match &self.source {
            Source::Tap(_) => pipeline.drain_capture(feed),
            Source::OwnHook { edges, .. } => {
                while let Ok(edge) = edges.try_recv() {
                    feed(edge);
                }
            }
        }
        done
    }

    /// Edges each source has contributed. The split is the point: a recording
    /// that only completes because the scanner filled gaps is a hook that is
    /// dropping events, and that is visible here rather than needing a log.
    fn counts(&self) -> CaptureCounts {
        match &self.source {
            Source::Tap(tap) => tap.counts(),
            Source::OwnHook { .. } => CaptureCounts { hook: 0, polled: 0 },
        }
    }

    fn is_alive(&self) -> bool {
        match &self.source {
            Source::Tap(tap) => tap.is_alive(),
            Source::OwnHook { handle, .. } => handle.is_alive(),
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
        if let Some(tap) = pipeline.arm_capture() {
            log::info!("shortcut recording: tapping the live push-to-talk hook");
            self.recording = Some(Recording {
                source: Source::Tap(tap),
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
                    source: Source::OwnHook {
                        handle,
                        edges: hook_rx,
                    },
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
        if let Some(chord) = rec.poll(pipeline) {
            let c = rec.counts();
            log::info!(
                "shortcut recorded: {} edges from the hook, {} from the scanner",
                c.hook,
                c.polled
            );
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

    /// Give the key stream back to the chord tracker; an owned hook just drops.
    fn release(&mut self, rec: Recording, pipeline: &PipelineController) {
        if matches!(rec.source, Source::Tap(_)) {
            pipeline.disarm_capture();
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

    /// Key edges each source has reported since recording began.
    pub fn counts(&self) -> CaptureCounts {
        self.recording
            .as_ref()
            .map_or(CaptureCounts { hook: 0, polled: 0 }, |r| r.counts())
    }

    /// Why the last combination the user let go of was turned down, if it was.
    /// Recording carries on, so they can simply try another one.
    pub fn rejected(&self) -> Option<Rejected> {
        self.recording.as_ref().and_then(|r| r.buffer.rejected())
    }

    /// What else the keys currently held already do, if anything well-known
    /// does. Live while the user holds them, so they learn the combination is
    /// spoken for BEFORE they let go and set it.
    pub fn held_collision(&self) -> Option<&'static hark_hotkey::KnownShortcut> {
        let held = self.recording.as_ref()?.buffer.held();
        if held.is_empty() {
            return None;
        }
        hark_hotkey::known::lookup(held)
    }
}

//! The detection prompt (plan §4.7): "Teams is using your mic. Take meeting
//! notes?" with Start, Not this meeting, and Settings. It must work while the
//! main window is hidden in the tray, never take focus from the meeting app,
//! and dismiss itself after 30 s.
//!
//! It follows `overlay.rs` where that module learned the hard way: **one
//! persistent deferred viewport, created hidden and only shown/hidden, never a
//! window per prompt** (a window per event lost the GPU device and flashed),
//! registered from `App::logic` so it runs while the app sits in the tray.
//!
//! **The root shows it, not the prompt itself.** eframe 0.36 runs a hidden
//! deferred viewport's UI callback only while egui considers it visible, and
//! `ViewportInfo::visible()` comes from minimized/occluded state, not from
//! whether the window is shown. A prompt that revealed itself from its own
//! callback therefore depended on how egui happened to see a hidden window:
//! it appeared for a Teams call and never for a Meet call in the next session.
//! So `App::logic` sends `Visible(true)` to this viewport and places the
//! window through Win32 in physical pixels; the callback only paints the
//! prompt, times it out, and hides it once answered. Every step is logged
//! (labels only), so a prompt that still fails to appear is answerable from
//! the log.

use crate::meeting::Answer;
use crate::theme;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The window's title, which is also how Win32 finds it (`win::place` looks
/// it up by this exact string).
const TITLE: &str = "Hark meeting prompt";
const SIZE: egui::Vec2 = egui::vec2(360.0, 124.0);
/// Plan §4.7: a prompt nobody answers goes away on its own.
const TIMEOUT: Duration = Duration::from_secs(30);
/// A prompt shown this long without being painted is logged as a failure.
const NOT_PAINTED_AFTER: Duration = Duration::from_secs(3);

/// What the prompt asked the app to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply {
    Answer(Answer),
    /// Dismiss the prompt and open Settings > Meetings.
    OpenSettings,
}

#[derive(Default)]
struct Shared {
    /// The app name to show, and when the prompt appeared.
    showing: Option<(String, Instant)>,
    reply: Option<Reply>,
    /// The `shown` instant of the last prompt the callback painted.
    painted: Option<Instant>,
}

/// The root's side of the prompt window.
pub struct PromptWindow {
    shared: Arc<Mutex<Shared>>,
    /// The prompt already reported as never painted (one warning each).
    warned: Option<Instant>,
}

impl PromptWindow {
    pub fn new() -> Self {
        PromptWindow {
            shared: Arc::new(Mutex::new(Shared::default())),
            warned: None,
        }
    }

    /// Register the viewport for this pass (every pass while meetings are
    /// available, so egui never retires it), show or hide it as the prompt
    /// comes and goes, and return the user's reply if one arrived.
    pub fn show(&mut self, ctx: &egui::Context, prompt: Option<(&str, Instant)>) -> Option<Reply> {
        let (reply, appeared, gone, unpainted) = {
            let Ok(mut shared) = self.shared.lock() else {
                return None;
            };
            let wanted = prompt.map(|(name, shown)| (name.to_string(), shown));
            let was = shared.showing.as_ref().map(|(_, s)| *s);
            let now = wanted.as_ref().map(|(_, s)| *s);
            let appeared = now.is_some() && now != was;
            let gone = was.is_some() && now.is_none();
            if appeared || gone {
                shared.showing = wanted;
            }
            let unpainted = shared
                .showing
                .as_ref()
                .map(|(_, s)| *s)
                .filter(|s| shared.painted != Some(*s) && s.elapsed() >= NOT_PAINTED_AFTER);
            (shared.reply.take(), appeared, gone, unpainted)
        };

        if appeared {
            log::info!("meeting prompt: showing");
            #[cfg(windows)]
            win::place(ctx.zoom_factor());
            ctx.send_viewport_cmd_to(viewport_id(), egui::ViewportCommand::Visible(true));
            ctx.request_repaint_of(viewport_id());
        } else if gone && reply.is_none() {
            // Retracted (the app let go of the mic) or superseded by a start.
            log::info!("meeting prompt: withdrawn");
            ctx.send_viewport_cmd_to(viewport_id(), egui::ViewportCommand::Visible(false));
        }
        if let Some(shown) = unpainted {
            if self.warned != Some(shown) {
                self.warned = Some(shown);
                log::warn!(
                    "meeting prompt: shown {} s ago but never painted",
                    shown.elapsed().as_secs()
                );
            }
        }
        if let Some(reply) = reply {
            log::info!("meeting prompt: answered {reply:?}");
        }
        register(ctx, self.shared.clone());
        reply
    }
}

fn viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("hark_meeting_prompt")
}

fn register(ctx: &egui::Context, shared: Arc<Mutex<Shared>>) {
    let builder = egui::ViewportBuilder::default()
        .with_title(TITLE)
        .with_inner_size(SIZE)
        .with_decorations(false)
        .with_resizable(false)
        .with_always_on_top()
        .with_taskbar(false)
        // Born hidden and left hidden in the builder: the root toggles
        // visibility with commands, so the two never fight.
        .with_visible(false)
        // Never take focus from the meeting app.
        .with_active(false);
    ctx.show_viewport_deferred(viewport_id(), builder, move |ui, _class| {
        paint(ui, &shared);
    });
}

fn paint(ui: &mut egui::Ui, shared: &Mutex<Shared>) {
    let ctx = ui.ctx().clone();
    let Ok(mut state) = shared.lock() else {
        return;
    };
    let Some((name, shown)) = state.showing.clone() else {
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        return;
    };
    if state.painted != Some(shown) {
        state.painted = Some(shown);
        log::info!("meeting prompt: painted");
    }
    let mut reply = None;
    if shown.elapsed() >= TIMEOUT {
        reply = Some(Reply::Answer(Answer::Dismissed));
    } else {
        // Wake once a second to notice the timeout.
        ctx.request_repaint_after(Duration::from_secs(1));
        egui::Frame::new()
            .fill(ui.visuals().window_fill)
            .stroke(egui::Stroke::new(1.0, theme::edge(ui.visuals())))
            .inner_margin(egui::Margin::same(theme::CARD_PADDING))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                ui.horizontal(|ui| {
                    ui.label(
                        theme::icon_text(theme::icons::MICROPHONE)
                            .color(theme::accent(ui.visuals())),
                    );
                    ui.label(egui::RichText::new(format!("{name} is using your mic.")).strong());
                });
                ui.label("Take meeting notes?");
                ui.add_space(theme::GAP);
                ui.horizontal(|ui| {
                    if ui
                        .add(theme::primary_button(ui.visuals(), "Start"))
                        .clicked()
                    {
                        reply = Some(Reply::Answer(Answer::Start));
                    }
                    if ui.button("Not this meeting").clicked() {
                        reply = Some(Reply::Answer(Answer::NotThisMeeting));
                    }
                    if ui.button("Settings").clicked() {
                        reply = Some(Reply::OpenSettings);
                    }
                });
            });
    }
    if let Some(reply) = reply {
        state.showing = None;
        state.reply = Some(reply);
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        // Same thread (the event loop), so a direct root repaint is safe.
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
}

#[cfg(windows)]
mod win {
    use super::SIZE;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
    };
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GetForegroundWindow, SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE,
        SWP_NOOWNERZORDER,
    };

    /// Distance from the work area's bottom-right corner, in logical points.
    const MARGIN: f32 = 16.0;

    /// Strip the frame, then move and size the window to the bottom-right of
    /// the work area (taskbar excluded) of the monitor the user is on, in that
    /// monitor's physical pixels. Straight Win32 rather than a viewport
    /// command: `OuterPosition` is converted with the scale of the monitor the
    /// window currently sits on, which is wrong the moment it moves to another.
    pub(super) fn place(zoom: f32) {
        // SAFETY: a plain lookup by our own unique window title.
        let hwnd = match unsafe { FindWindowW(PCWSTR::null(), w!("Hark meeting prompt")) } {
            Ok(hwnd) if !hwnd.is_invalid() => hwnd,
            _ => {
                log::warn!("meeting prompt: its window does not exist yet; not placed");
                return;
            }
        };
        crate::overlay::strip_frame_styles(hwnd);

        // SAFETY (this block and the two below): plain Win32 getters. Every
        // handle comes from the call before it or a documented primary-monitor
        // fallback, and every out-param is a fully initialized local.
        let monitor = unsafe {
            let foreground = GetForegroundWindow();
            if foreground.is_invalid() {
                MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY)
            } else {
                MonitorFromWindow(foreground, MONITOR_DEFAULTTOPRIMARY)
            }
        };
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
            log::warn!("meeting prompt: no monitor information; not placed");
            return;
        }
        let (mut dpi_x, mut dpi_y) = (96_u32, 96_u32);
        if unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) }.is_err()
        {
            log::warn!("meeting prompt: no monitor DPI; not placed");
            return;
        }
        let scale = zoom * dpi_x as f32 / 96.0;
        let work = info.rcWork;
        let (w, h) = (
            (SIZE.x * scale).round() as i32,
            (SIZE.y * scale).round() as i32,
        );
        let margin = (MARGIN * scale).round() as i32;
        let (x, y) = (work.right - w - margin, work.bottom - h - margin);
        // SAFETY: repositioning our own window; NOACTIVATE keeps the focus on
        // the meeting app.
        let moved = unsafe {
            SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                x,
                y,
                w,
                h,
                SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            )
        };
        match moved {
            Ok(()) => log::info!(
                "meeting prompt: placed at {x},{y} ({w}x{h} px, {dpi_x} dpi; work area {},{}-{},{})",
                work.left,
                work.top,
                work.right,
                work.bottom
            ),
            Err(e) => log::warn!("meeting prompt: could not be placed ({e})"),
        }
    }
}

//! The detection prompt (plan §4.7): "Teams is using your mic. Take meeting
//! notes?" with Start, Not this meeting, and Settings. It must work while the
//! main window is hidden in the tray, never take focus from the meeting app,
//! and dismiss itself after 30 s.
//!
//! It follows `overlay.rs` exactly where that module learned the hard way:
//! **one persistent deferred viewport, created hidden and only shown/hidden,
//! never a window per prompt** (a window per event lost the GPU device and
//! flashed), registered from `App::logic` so it runs while the app sits in
//! the tray. The viewport callback cannot borrow the app, so the two talk
//! through a small shared cell: the root writes the prompt, the callback
//! writes the answer and wakes the root.

use crate::meeting::Answer;
use crate::theme;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const TITLE: &str = "Hark meeting prompt";
const SIZE: egui::Vec2 = egui::vec2(360.0, 124.0);
/// Plan §4.7: a prompt nobody answers goes away on its own.
const TIMEOUT: Duration = Duration::from_secs(30);
/// Distance from the work area's bottom-right corner, in logical points.
#[cfg(windows)]
const MARGIN: f32 = 16.0;

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
}

/// The root's side of the prompt window.
pub struct PromptWindow {
    shared: Arc<Mutex<Shared>>,
}

impl PromptWindow {
    pub fn new() -> Self {
        PromptWindow {
            shared: Arc::new(Mutex::new(Shared::default())),
        }
    }

    /// Register the viewport for this pass (every pass while meetings are
    /// available, so egui never retires it) and sync what it shows. Returns
    /// the user's reply, if one arrived since the last pass.
    pub fn show(&self, ctx: &egui::Context, prompt: Option<(&str, Instant)>) -> Option<Reply> {
        let reply = {
            let Ok(mut shared) = self.shared.lock() else {
                return None;
            };
            let wanted = prompt.map(|(name, shown)| (name.to_string(), shown));
            let changed = shared.showing.as_ref().map(|(n, s)| (n.as_str(), *s))
                != wanted.as_ref().map(|(n, s)| (n.as_str(), *s));
            if changed {
                shared.showing = wanted;
                ctx.request_repaint_of(viewport_id());
            }
            shared.reply.take()
        };
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
        // Born hidden and left hidden in the builder: `paint` toggles
        // visibility with commands, like the recording overlay.
        .with_visible(false)
        // Never take focus from the meeting app.
        .with_active(false);
    ctx.show_viewport_deferred(viewport_id(), builder, move |ui, _class| {
        paint(ui, &shared);
    });
}

fn paint(ui: &mut egui::Ui, shared: &Mutex<Shared>) {
    let ctx = ui.ctx().clone();
    #[cfg(windows)]
    strip_frame();
    let Ok(mut state) = shared.lock() else {
        return;
    };
    let Some((name, shown)) = state.showing.clone() else {
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        return;
    };
    let mut reply = None;
    if shown.elapsed() >= TIMEOUT {
        reply = Some(Reply::Answer(Answer::Dismissed));
    } else {
        #[cfg(windows)]
        place(&ctx);
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
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

/// Make the window a plain popup (no caption, not independently closable),
/// once per window; see `overlay::strip_frame_styles` for why winit's
/// "undecorated" is not enough on Windows.
#[cfg(windows)]
fn strip_frame() {
    use windows::core::{w, PCWSTR};
    use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
    // SAFETY: a plain lookup by our own unique window title.
    if let Ok(hwnd) = unsafe { FindWindowW(PCWSTR::null(), w!("Hark meeting prompt")) } {
        if !hwnd.is_invalid() {
            crate::overlay::strip_frame_styles(hwnd);
        }
    }
}

/// Bottom-right of the work area (taskbar excluded) of the monitor the user
/// is on, where notification-style prompts live on Windows.
#[cfg(windows)]
fn place(ctx: &egui::Context) {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
    };
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    // SAFETY: plain Win32 getters; every handle comes from the call before
    // it or a documented primary-monitor fallback, and out-params are locals.
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
        return;
    }
    let (mut dpi_x, mut dpi_y) = (96_u32, 96_u32);
    if unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) }.is_err() {
        return;
    }
    let scale = ctx.zoom_factor() * dpi_x as f32 / 96.0;
    let work = info.rcWork;
    let target = egui::pos2(
        work.right as f32 - (SIZE.x + MARGIN) * scale,
        work.bottom as f32 - (SIZE.y + MARGIN) * scale,
    );
    let ppp = ctx.pixels_per_point();
    let placed = ctx.input(|i| i.viewport().outer_rect).is_some_and(|r| {
        (r.min.x * ppp - target.x).abs() <= 2.0 && (r.min.y * ppp - target.y).abs() <= 2.0
    });
    if !placed {
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
            target.x / ppp,
            target.y / ppp,
        )));
    }
}

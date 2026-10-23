//! The detection prompt (plan §4.7): "Teams is using your mic. Take meeting
//! notes?" with Start, Not this meeting, and Settings. It must work while the
//! main window is hidden in the tray, never take focus from the meeting app,
//! and dismiss itself after 30 s. It always appears on the primary monitor and
//! stays on top of everything else while it waits: placed by the monitor of
//! the foreground window, it landed at the foot of a tall portrait screen,
//! far below the Teams window the user was watching, and went unseen.
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
//!
//! **The window is sized from what it paints, never from a guess.** A fixed
//! 124 pt height clipped the buttons and the bottom edge on every display:
//! each button row is a full 34 pt control row, and the sum ran past it.
//! Every pass measures the content's height, and a change resizes the window
//! (through Win32 on Windows, so it stays anchored above the taskbar).

use crate::meeting::Answer;
use crate::theme;
use egui::RichText;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The window's title, which is also how Win32 finds it (`win::place` looks
/// it up by this exact string).
const TITLE: &str = "Hark meeting prompt";
/// The size before the first paint has measured the content: close, so the
/// first prompt rarely needs a resize, but the measurement is what counts.
const SIZE: egui::Vec2 = egui::vec2(theme::PROMPT_WIDTH, 168.0);
/// Plan §4.7: a prompt nobody answers goes away on its own.
const TIMEOUT: Duration = Duration::from_secs(30);
/// A prompt shown this long without being painted is logged as a failure.
const NOT_PAINTED_AFTER: Duration = Duration::from_secs(3);
/// How often a showing prompt reclaims the top of the z-order. Other
/// always-on-top windows (Teams' floating call window among them) join the
/// topmost band above it after it appears; the prompt already wakes this
/// often for its timeout, so this adds no repaints.
#[cfg(windows)]
const RAISE_EVERY: Duration = Duration::from_secs(1);

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
    /// The size the content last measured, in points; `SIZE` until then.
    size: Option<egui::Vec2>,
    /// When the callback last put the prompt back on top.
    #[cfg(windows)]
    raised: Option<Instant>,
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
            {
                let size = self.shared.lock().ok().and_then(|s| s.size);
                win::place(
                    ctx.zoom_factor(),
                    size.unwrap_or(SIZE),
                    theme::accent_fill(&ctx.global_style().visuals),
                );
            }
            #[cfg(target_os = "macos")]
            crate::macos::place_overlay(TITLE, true);
            #[cfg(not(target_os = "macos"))]
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
        #[cfg(target_os = "macos")]
        {
            crate::macos::place_overlay(TITLE, true);
            let visible = prompt.is_some() && reply.is_none();
            if !crate::macos::overlay_visible(TITLE, visible) && visible {
                ctx.request_repaint_after_for(Duration::from_millis(50), egui::ViewportId::ROOT);
            }
            if visible {
                ctx.request_repaint_of(viewport_id());
            }
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
    #[cfg(target_os = "macos")]
    crate::macos::place_overlay(TITLE, true);
    if state.painted != Some(shown) {
        state.painted = Some(shown);
        log::info!("meeting prompt: painted");
    }
    let mut reply = None;
    let mut resize = None;
    let left = TIMEOUT.saturating_sub(shown.elapsed());
    if left.is_zero() {
        reply = Some(Reply::Answer(Answer::Dismissed));
    } else {
        // Wake once a second to notice the timeout and tick the countdown.
        ctx.request_repaint_after(Duration::from_secs(1));
        let height = card(ui, &name, left, &mut reply);
        let size = egui::vec2(theme::PROMPT_WIDTH, height.ceil());
        if (state.size.unwrap_or(SIZE) - size).length() > 0.5 {
            state.size = Some(size);
            resize = reply.is_none().then_some(size);
        }
    }
    if let Some(reply) = reply {
        state.showing = None;
        state.reply = Some(reply);
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        // Same thread (the event loop), so a direct root repaint is safe.
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
    #[cfg(windows)]
    {
        let raise = reply.is_none() && state.raised.is_none_or(|at| at.elapsed() >= RAISE_EVERY);
        if raise || resize.is_some() {
            state.raised = Some(Instant::now());
        }
        // Unlocked first: SetWindowPos sends this window's own messages
        // synchronously on this thread.
        drop(state);
        if let Some(size) = resize {
            log::info!("meeting prompt: resized to fit its content");
            win::place(ctx.zoom_factor(), size, theme::accent_fill(ui.visuals()));
        } else if raise {
            win::raise();
        }
    }
    #[cfg(not(windows))]
    if let Some(size) = resize {
        log::info!("meeting prompt: resized to fit its content");
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
    }
}

/// Paint the prompt and return the height its content takes, in points: an
/// accent header with a draining countdown, then the question and its
/// answers, all inside an accent border. The width is fixed so the only
/// variable string (the app's name) wraps, and the height follows from it.
fn card(ui: &mut egui::Ui, name: &str, left: Duration, reply: &mut Option<Reply>) -> f32 {
    let visuals = ui.visuals().clone();
    let (accent, on_accent) = (theme::accent_fill(&visuals), theme::on_accent(&visuals));
    let window = ui.max_rect();
    let inner = theme::PROMPT_WIDTH - 2.0 * f32::from(theme::CARD_PADDING);
    // Fill first: the window can be taller than the content for the one
    // pass before a resize lands.
    ui.painter().rect_filled(window, 0.0, visuals.window_fill);
    let spacing = ui.spacing().item_spacing.y;
    ui.spacing_mut().item_spacing.y = 0.0;

    let header = egui::Frame::new()
        .fill(accent)
        .inner_margin(theme::PROMPT_HEADER_MARGIN)
        .show(ui, |ui| {
            ui.set_width(inner);
            // A bare text row, not a 34 pt control row.
            ui.spacing_mut().interact_size.y = 0.0;
            ui.horizontal(|ui| {
                ui.label(theme::icon_text(theme::icons::MICROPHONE).color(on_accent));
                ui.label(
                    RichText::new("Hark")
                        .text_style(egui::TextStyle::Button)
                        .color(on_accent),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let secs = left.as_millis().div_ceil(1000);
                    ui.label(
                        RichText::new(format!("Closes in {secs} s"))
                            .small()
                            .color(on_accent),
                    );
                });
            });
        })
        .response
        .rect;
    let remaining = left.as_secs_f32() / TIMEOUT.as_secs_f32();
    let bar = egui::Rect::from_min_max(
        egui::pos2(
            header.left(),
            header.bottom() - theme::PROMPT_COUNTDOWN_HEIGHT,
        ),
        egui::pos2(header.left() + header.width() * remaining, header.bottom()),
    );
    ui.painter()
        .rect_filled(bar, 0.0, theme::countdown(&visuals));

    let body = egui::Frame::new()
        .inner_margin(egui::Margin::same(theme::CARD_PADDING))
        .show(ui, |ui| {
            ui.set_width(inner);
            ui.spacing_mut().item_spacing.y = spacing;
            ui.label(
                RichText::new(format!("{name} is using your mic."))
                    .font(theme::prompt_title_font())
                    .strong(),
            );
            ui.label(RichText::new("Take meeting notes?").weak());
            ui.add_space(theme::GAP);
            ui.horizontal(|ui| {
                if ui
                    .add(theme::primary_button(ui.visuals(), "Start"))
                    .clicked()
                {
                    *reply = Some(Reply::Answer(Answer::Start));
                }
                if ui.button("Not this meeting").clicked() {
                    *reply = Some(Reply::Answer(Answer::NotThisMeeting));
                }
                if ui.button("Settings").clicked() {
                    *reply = Some(Reply::OpenSettings);
                }
            });
        })
        .response
        .rect;

    ui.painter().rect_stroke(
        window,
        0.0,
        egui::Stroke::new(theme::PROMPT_BORDER, accent),
        egui::StrokeKind::Inside,
    );
    header.height() + body.height()
}

#[cfg(windows)]
mod win {
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Foundation::{COLORREF, POINT};
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
        DWM_WINDOW_CORNER_PREFERENCE,
    };
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
    };
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER,
        SWP_NOSIZE,
    };

    /// Distance from the work area's bottom-right corner, in logical points.
    const MARGIN: f32 = 16.0;

    fn find() -> Option<HWND> {
        // SAFETY: a plain lookup by our own unique window title.
        match unsafe { FindWindowW(PCWSTR::null(), w!("Hark meeting prompt")) } {
            Ok(hwnd) if !hwnd.is_invalid() => Some(hwnd),
            _ => None,
        }
    }

    /// Strip the frame, then move and size the window (`size` in points) to
    /// the bottom-right of the primary monitor's work area (taskbar
    /// excluded), in that monitor's physical pixels. Straight Win32 rather
    /// than a viewport command: `OuterPosition` is converted with the scale of
    /// the monitor the window currently sits on, which is wrong the moment it
    /// moves to another.
    pub(super) fn place(zoom: f32, size: egui::Vec2, border: egui::Color32) {
        let Some(hwnd) = find() else {
            log::warn!("meeting prompt: its window does not exist yet; not placed");
            return;
        };
        crate::overlay::strip_frame_styles(hwnd);
        round(hwnd, border);

        // SAFETY (this block and the two below): plain Win32 getters. The
        // primary monitor's top-left corner is (0, 0) by definition, and every
        // out-param is a fully initialized local.
        let monitor = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
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
            (size.x * scale).round() as i32,
            (size.y * scale).round() as i32,
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
                "meeting prompt: placed at {x},{y} on the primary monitor ({w}x{h} px, {dpi_x} dpi; work area {},{}-{},{})",
                work.left,
                work.top,
                work.right,
                work.bottom
            ),
            Err(e) => log::warn!("meeting prompt: could not be placed ({e})"),
        }
    }

    /// Round the corners and tint DWM's outline with the accent, so the prompt
    /// reads like Windows 11's own notifications instead of a bare rectangle.
    /// Windows 10 rejects both attributes and keeps square corners; the
    /// painted border still frames it, so that failure only changes the look.
    fn round(hwnd: HWND, border: egui::Color32) {
        let corner = DWMWCP_ROUND;
        let color = COLORREF(
            u32::from(border.r()) | (u32::from(border.g()) << 8) | (u32::from(border.b()) << 16),
        );
        // SAFETY: each pointer is a live local of exactly the size passed.
        let set = unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                std::ptr::from_ref(&corner).cast(),
                std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
            )
            .and_then(|()| {
                DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_BORDER_COLOR,
                    std::ptr::from_ref(&color).cast(),
                    std::mem::size_of::<COLORREF>() as u32,
                )
            })
        };
        if let Err(e) = set {
            log::debug!("meeting prompt: square corners ({e})");
        }
    }

    /// Put the prompt back at the top of the always-on-top band, where a
    /// topmost window opened after it would otherwise cover it. Z-order only:
    /// it neither moves, resizes, nor activates the window.
    pub(super) fn raise() {
        let Some(hwnd) = find() else {
            return;
        };
        // SAFETY: restacking our own window; NOACTIVATE keeps the focus on
        // the meeting app.
        let raised = unsafe {
            SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            )
        };
        if let Err(e) = raised {
            log::warn!("meeting prompt: could not be kept on top ({e})");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lay the card out headlessly and return (height it reported, width
    /// its widest row took).
    fn measure(name: &str) -> (f32, f32) {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SIZE)),
            ..Default::default()
        };
        let mut measured = (0.0, 0.0);
        // The first pass installs the fonts; the second lays out with them.
        for _ in 0..2 {
            let mut output = ctx.run_ui(input.clone(), |ui| {
                let height = card(ui, name, TIMEOUT, &mut None);
                measured = (height, ui.min_rect().width());
            });
            // No renderer consumes the font atlas here.
            output.textures_delta.clear();
        }
        measured
    }

    #[test]
    fn the_first_window_already_fits_a_typical_prompt() {
        for name in ["Teams", "Zoom", "Google Meet", "Slack"] {
            let (height, width) = measure(name);
            assert!(height <= SIZE.y, "{name}: {height} pt tall in {}", SIZE.y);
            assert!(width <= SIZE.x, "{name}: {width} pt wide in {}", SIZE.x);
        }
    }

    #[test]
    fn a_long_name_grows_the_prompt_instead_of_clipping_it() {
        let (short, _) = measure("Teams");
        let (long, width) = measure("Microsoft Teams (work or school) meeting window");
        assert!(long > short, "{long} pt is not taller than {short} pt");
        assert!(width <= SIZE.x, "{width} pt wide in {}", SIZE.x);
    }
}

//! The recording overlay (the Phase 5 "floating recording pill"): a small
//! always-on-top "pill" that appears near the bottom of the screen while the
//! push-to-talk chord is held, with a purple circle that pulses to the mic
//! input (the WhisperFlow-style cue).
//!
//! It is an egui **deferred viewport** (a second borderless OS window driven
//! by the same main-thread event loop), so it honours the one hard rule: no
//! UI off the main thread. It is registered from [`crate::app`]'s `logic`
//! (which runs even while Hark's main window is hidden in the tray, unlike
//! `ui`), so the overlay works during normal daemon operation. Once
//! registered it animates on its own repaint requests while the parent window
//! sleeps; dropping the registration on chord release tears the window down.
//!
//! Placement is platform-split on purpose. egui only ever exposes a monitor
//! *size*, never its origin or its own DPI, which is not enough to position a
//! window on a multi-monitor desktop; on Windows we therefore ask Win32 for the
//! real work area and move the window ourselves (`reposition`). Getting this
//! wrong does not look like a bug — the pill is created and painted as usual,
//! just at coordinates no monitor covers.
//!
//! The window is transparent and mouse-transparent and never takes focus:
//! Hark injects text into whatever app is focused, so the overlay must not
//! steal that focus. Those flags (`with_active(false)`,
//! `with_mouse_passthrough`, `with_taskbar(false)`) are validated on real
//! Windows/macOS hardware, not on this build-only machine.

use crate::theme;
use hark_pipeline::LevelMeter;
use std::sync::Arc;
use std::time::Duration;

/// Logical size of the overlay window. Larger than the pill so the pulse's
/// soft glow has room to bloom without being clipped.
const WINDOW: egui::Vec2 = egui::vec2(140.0, 60.0);
/// The dark capsule inside the window.
const PILL: egui::Vec2 = egui::vec2(112.0, 40.0);
/// Circle radius at rest and the extra radius at a full-scale pulse.
const CIRCLE_BASE: f32 = 7.5;
const CIRCLE_PULSE: f32 = 7.5;
/// Fraction of the screen height to float above the bottom edge.
const BOTTOM_MARGIN_FRAC: f32 = 0.09;
/// A gentle idle "breathing" so the dot is alive even in silence.
const BREATH_HZ: f32 = 0.8;

/// Register (or keep alive) the overlay viewport for this frame. Call every
/// frame the chord is held; stop calling it to dismiss the overlay.
///
/// `monitor` is a monitor's logical size when known. It is only good enough to
/// place the pill off Windows; on Windows `reposition` moves the window from
/// the real work area once it exists, because a size with no origin cannot
/// place a window on a multi-monitor desktop.
pub fn show(ctx: &egui::Context, meter: Arc<LevelMeter>, monitor: Option<egui::Vec2>) {
    let id = egui::ViewportId::from_hash_of("hark_recording_overlay");

    let builder = egui::ViewportBuilder::default()
        .with_title("Hark recording")
        .with_inner_size(WINDOW)
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_always_on_top()
        .with_taskbar(false)
        // Never take focus: injection targets the previously focused app.
        .with_active(false)
        // Clicks fall through to whatever is underneath.
        .with_mouse_passthrough(true);

    // Windows places the pill itself, from the real work area of the monitor
    // the user is on (`reposition`); a creation-time guess would only make it
    // flash somewhere wrong first.
    #[cfg(windows)]
    let _ = monitor;
    #[cfg(not(windows))]
    let builder = match monitor {
        Some(monitor) => {
            let x = (monitor.x - WINDOW.x) / 2.0;
            let y = monitor.y - WINDOW.y - monitor.y * BOTTOM_MARGIN_FRAC;
            builder.with_position(egui::pos2(x.max(0.0), y.max(0.0)))
        }
        None => builder,
    };

    ctx.show_viewport_deferred(id, builder, move |ui, _class| {
        paint(ui, &meter);
    });
}

/// Put the pill on the bottom-centre of the monitor the user is working on.
///
/// `ViewportBuilder::with_position` cannot do this. egui hands it to winit as a
/// *logical* position in virtual-desktop coordinates (origin = top-left of the
/// primary monitor), converted with the scale factor winit guesses before the
/// window belongs to any monitor — while the only geometry egui exposes,
/// `monitor_size`, is a size with no origin. Computing a position from that
/// size alone silently assumes the current monitor starts at (0, 0) and shares
/// the primary's DPI. On a multi-monitor, mixed-scaling desktop both are false,
/// and the pill lands on the wrong monitor or in the dead space between them:
/// created, painted, and never visible. (egui's own
/// `ViewportCommand::center_on_screen` makes the same assumption.)
///
/// So ask Win32 for the work area in real desktop pixels and move the window
/// there. `ViewportCommand::OuterPosition` is applied as
/// `pixels_per_point * pos`, so dividing by this window's own
/// `pixels_per_point` round-trips to the exact physical pixel no matter which
/// monitor the window currently sits on.
#[cfg(windows)]
fn reposition(ctx: &egui::Context) {
    let Some(target) = work_area_position(ctx.zoom_factor()) else {
        // No usable monitor geometry: leave the window where the OS put it.
        // Visible in the wrong place beats moved to a computed nowhere.
        return;
    };
    let ppp = ctx.pixels_per_point();
    // `outer_rect` is the ground truth for "am I already there", and is in
    // points of this window's own scale factor, so compare in physical pixels.
    // Comparing against the window rather than caching the last send is what
    // makes each dictation's fresh window get placed, not just the first.
    let placed = ctx.input(|i| i.viewport().outer_rect).is_some_and(|r| {
        (r.min.x * ppp - target.x).abs() <= 2.0 && (r.min.y * ppp - target.y).abs() <= 2.0
    });
    if placed {
        return;
    }
    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
        target.x / ppp,
        target.y / ppp,
    )));
}

/// The pill's top-left in physical desktop pixels, from the work area (taskbar
/// excluded) of the monitor holding the foreground window. The overlay never
/// takes focus, so that window is still the app being dictated into — the
/// monitor the user is looking at. `None` if Windows will not tell us.
#[cfg(windows)]
fn work_area_position(zoom: f32) -> Option<egui::Pos2> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
    };
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    // SAFETY (this block and the two below): plain Win32 getters. Every handle
    // is either produced by the call before it or a documented "give me the
    // primary" fallback, and every out-param is a fully initialized local.
    let monitor = unsafe {
        let foreground = GetForegroundWindow();
        if foreground.is_invalid() {
            // A bare desktop with nothing focused: the primary monitor is the
            // only defensible guess.
            MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY)
        } else {
            MonitorFromWindow(foreground, MONITOR_DEFAULTTOPRIMARY)
        }
    };

    // cbSize must be set before the call; a zero return means the handle was
    // rejected, leaving `rcWork` untouched (i.e. all zeros).
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        return None;
    }
    let work = info.rcWork;

    // Per-monitor DPI, not the process or primary DPI: the pill's logical size
    // becomes physical pixels at *this* monitor's scale, which is exactly what
    // differs across a mixed-scaling desktop.
    let (mut dpi_x, mut dpi_y) = (96_u32, 96_u32);
    unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) }.ok()?;
    let scale = zoom * dpi_x as f32 / 96.0;

    let (window_w, window_h) = (WINDOW.x * scale, WINDOW.y * scale);
    let work_w = (work.right - work.left) as f32;
    let work_h = (work.bottom - work.top) as f32;
    Some(egui::pos2(
        work.left as f32 + (work_w - window_w) / 2.0,
        work.bottom as f32 - window_h - work_h * BOTTOM_MARGIN_FRAC,
    ))
}

/// Draw one frame of the pill + pulsing circle, and schedule the next frame.
fn paint(ui: &mut egui::Ui, meter: &LevelMeter) {
    let ctx = ui.ctx();
    #[cfg(windows)]
    reposition(ctx);
    // Keep the pulse animating while the parent window sleeps. ~60 fps is
    // plenty for a breathing dot and stays light during a short hold.
    ctx.request_repaint_after(Duration::from_millis(16));

    let time = ui.input(|i| i.time) as f32;

    // Raw peak (0..=1) is small for normal speech; a square-root curve lifts
    // conversational levels into a visible range without pinning loud peaks.
    let raw = meter.level();
    let target = (raw.sqrt() * 1.25).clamp(0.0, 1.0);
    // Ease the displayed amplitude so block-to-block jitter reads as a smooth
    // swell; egui caches the animation state by id across frames.
    let amp = ctx.animate_value_with_time(egui::Id::new("hark_overlay_amp"), target, 0.09);
    let breath = 0.5 + 0.5 * (time * BREATH_HZ * std::f32::consts::TAU).sin();
    // Audio dominates; the breath keeps a faint life in silence.
    let pulse = (amp * 0.9 + breath * 0.12).clamp(0.0, 1.0);

    let painter = ui.painter();
    let center = ui.max_rect().center();

    // The dark capsule.
    let pill = egui::Rect::from_center_size(center, PILL);
    let corner = egui::CornerRadius::same((PILL.y / 2.0) as u8);
    painter.rect_filled(pill, corner, theme::OVERLAY_PILL_FILL);
    painter.rect_stroke(
        pill,
        corner,
        egui::Stroke::new(1.0, theme::OVERLAY_PILL_STROKE),
        egui::StrokeKind::Inside,
    );

    let accent = theme::OVERLAY_ACCENT;
    let radius = CIRCLE_BASE + pulse * CIRCLE_PULSE;

    // A soft glow: two translucent rings that bloom with the pulse.
    for (scale, base_alpha) in [(2.1_f32, 26.0_f32), (1.5, 44.0)] {
        let alpha = (base_alpha * pulse) as u8;
        if alpha > 0 {
            painter.circle_filled(
                center,
                radius * scale,
                egui::Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), alpha),
            );
        }
    }
    // The core dot.
    painter.circle_filled(center, radius, accent);
}

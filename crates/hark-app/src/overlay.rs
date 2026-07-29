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
//! **The parent cannot be relied on to be awake, so this window takes itself
//! off screen.** Only a parent pass can *retire* a deferred viewport, and while
//! this one animates the parent may not get one for seconds: a visible window
//! is painted only when the OS delivers a paint, `WM_PAINT` is the lowest
//! priority message Windows has, and a 60 fps sibling with a vsync-blocking
//! buffer swap keeps the message queue from ever being empty long enough. (A
//! hidden or minimized main window is immune — eframe paints those straight
//! from its own loop, which is why a stranded pill used to clear the moment you
//! minimised Hark and not before.) So `paint` reads the pipeline's recording
//! flag directly and hides this window the instant a dictation ends. Retiring
//! it for real still waits for the parent, which is fine: by then nothing is on
//! screen, and hiding this window ends the queue pressure that was keeping the
//! parent asleep in the first place.
//!
//! Placement is platform-split on purpose. egui only ever exposes a monitor
//! *size*, never its origin or its own DPI, which is not enough to position a
//! window on a multi-monitor desktop; on Windows we therefore ask Win32 for the
//! real work area and move the window ourselves (`reposition`). Getting this
//! wrong does not look like a bug — the pill is created and painted as usual,
//! just at coordinates no monitor covers.
//!
//! The window is transparent and never takes focus: Hark injects text into
//! whatever app is focused, so the overlay must not steal that focus.
//! `with_active(false)` (never activate) and `with_taskbar(false)` handle
//! that, and are validated on real Windows/macOS hardware, not on this
//! build-only machine.
//!
//! It deliberately does NOT set `with_mouse_passthrough`. On Windows winit
//! implements passthrough by adding `WS_EX_LAYERED` to the window, and a
//! layered window is composited by DWM from a redirection bitmap that our
//! hardware GL surface never fills with per-pixel alpha — so the transparent
//! area renders as an opaque white (or black) box around the pill instead of
//! showing the desktop (egui #2537). Per-pixel transparency and passthrough
//! are mutually exclusive here, and a floating capsule needs the
//! transparency. The overlay only exists while the push-to-talk chord is
//! held and never activates, so the small bottom-centre rect briefly
//! swallowing a click matters far less than a broken-looking box.
//!
//! `with_decorations(false)` is not enough on Windows, either. winit keeps
//! `WS_CAPTION | WS_BORDER | WS_SYSMENU` on every window it creates ("required
//! styles to properly support common window functionality like aero snap") and
//! hides the frame only by overriding `WM_NCCALCSIZE` — an override it declines
//! whenever `wParam` is `FALSE`. Setting a window region (`shape_to_capsule`)
//! takes the window off DWM's frame path, and the still-present caption styles
//! then get drawn the classic way: minimise/maximise/close buttons painted over
//! the top-right of the pill. So `strip_frame_styles` removes those styles
//! outright, which is also what stops the pill being an independently closable
//! window instead of a transient cue.

use crate::theme;
use hark_pipeline::LevelMeter;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Logical size of the overlay window. The window *is* the capsule now: it is
/// filled edge to edge with the pill and, on Windows, clipped to that rounded
/// shape by the OS (`clip_to_capsule`). Per-pixel window transparency does not
/// work there with the glow backend — a larger transparent window renders an
/// opaque box (black, or white once the framebuffer has alpha) around the
/// pill instead of the desktop — so we stop relying on it and cut the window
/// to the pill's outline instead. The pulse glow is clipped to the capsule.
const WINDOW: egui::Vec2 = egui::vec2(160.0, 40.0);
/// Circle radius at rest and the extra radius at a full-scale pulse.
const CIRCLE_BASE: f32 = 6.5;
const CIRCLE_PULSE: f32 = 6.5;
/// Fraction of the screen height to float above the bottom edge.
const BOTTOM_MARGIN_FRAC: f32 = 0.09;
/// A gentle idle "breathing" so the dot is alive even in silence.
const BREATH_HZ: f32 = 0.8;

/// Register (or keep alive) the overlay viewport for this frame. Call every
/// frame the chord is held; stop calling it to dismiss the overlay.
///
/// `recording` is the pipeline's live capture flag, which `paint` uses to take
/// this window off screen without waiting for a parent pass (see the module
/// docs). `dictation` changes with every dictation, so the viewport id does
/// too: a window that hid itself can then never be handed to the *next*
/// dictation still hidden, which would show no pill at all.
///
/// `monitor` is a monitor's logical size when known. It is only good enough to
/// place the pill off Windows; on Windows `reposition` moves the window from
/// the real work area once it exists, because a size with no origin cannot
/// place a window on a multi-monitor desktop.
pub fn show(
    ctx: &egui::Context,
    meter: Arc<LevelMeter>,
    recording: Arc<AtomicBool>,
    dictation: u64,
    monitor: Option<egui::Vec2>,
) {
    let id = egui::ViewportId::from_hash_of(("hark_recording_overlay", dictation));

    let builder = egui::ViewportBuilder::default()
        .with_title("Hark recording")
        .with_inner_size(WINDOW)
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_always_on_top()
        .with_taskbar(false)
        // Never take focus: injection targets the previously focused app.
        // (No `with_mouse_passthrough`: on Windows it forces a layered window
        // that breaks per-pixel GL transparency — see the module docs.)
        .with_active(false);

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
        paint(ui, &meter, &recording);
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

/// Turn the overlay's OS window into a real borderless capsule: strip the frame
/// styles winit leaves on it, then clip the window to the pill's outline.
///
/// The window is drawn edge to edge as an opaque capsule; the region cuts the
/// actual window to that shape so the corners outside it are not part of the
/// window at all. That sidesteps window transparency entirely — which is broken
/// here (see [`WINDOW`]) — so no opaque margin can show, and clicks outside the
/// pill fall through to the app underneath instead of being swallowed.
///
/// The region is idempotent per window: we cache the last handle+size we
/// clipped, so it runs once per dictation's fresh window (and again only if its
/// size or DPI changes), not every frame. `strip_frame_styles` reports whether
/// it just restyled the window, which is exactly the "this handle is a window we
/// have not shaped yet" signal — needed because Windows recycles HWND values, so
/// a fresh window can otherwise match the cached key and never get its region.
#[cfg(windows)]
fn shape_to_capsule() {
    use std::cell::Cell;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{CreateRoundRectRgn, SetWindowRgn};
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, GetClientRect};

    thread_local! {
        // (HWND as isize, width_px, height_px) of the last window we clipped.
        // A fresh dictation is a new window (new handle), which re-triggers.
        static LAST: Cell<(isize, i32, i32)> = const { Cell::new((0, 0, 0)) };
    }

    // The overlay is the only window titled "Hark recording" (the main window
    // is "Hark"), and single-instance guarantees one Hark process.
    // SAFETY: plain Win32 getters; the handle is validated before use and the
    // RECT out-param is a fully initialized local.
    let hwnd = match unsafe { FindWindowW(PCWSTR::null(), w!("Hark recording")) } {
        Ok(hwnd) if !hwnd.is_invalid() => hwnd,
        _ => return,
    };
    // Before the region: a framed window that then gets a region is precisely
    // the combination that makes Windows draw the caption buttons by hand.
    let restyled = strip_frame_styles(hwnd);

    let mut rc = RECT::default();
    if unsafe { GetClientRect(hwnd, &mut rc) }.is_err() {
        return;
    }
    let (wpx, hpx) = (rc.right - rc.left, rc.bottom - rc.top);
    if wpx <= 0 || hpx <= 0 {
        return;
    }

    let key = (hwnd.0 as isize, wpx, hpx);
    if !restyled && LAST.with(|last| last.get()) == key {
        return;
    }

    // Inset the region 1px so its hard (unantialiased) edge lands on solid pill
    // pixels, clipping off the soft edge that would otherwise composite to a
    // bright fringe where window transparency does not work. A full-height
    // corner ellipse makes the ends semicircular — a capsule.
    let region = unsafe { CreateRoundRectRgn(1, 1, wpx - 1, hpx - 1, hpx, hpx) };
    if region.0.is_null() {
        return;
    }
    // SetWindowRgn takes ownership of the region on success; do not free it.
    if unsafe { SetWindowRgn(hwnd, Some(region), true) } != 0 {
        LAST.with(|last| last.set(key));
    }
}

/// Remove the caption/border/system-menu styles winit leaves on an
/// "undecorated" window, making the overlay a plain `WS_POPUP`. Returns whether
/// anything actually changed — true exactly once per freshly created window.
///
/// winit does not undecorate by style: it keeps `WS_CAPTION | WS_BORDER |
/// WS_SYSMENU` (plus the min/max boxes) on every window and hides the frame in
/// its `WM_NCCALCSIZE` handler, which forwards to `DefWindowProc` whenever
/// `wParam` is `FALSE`. Left in place, those styles are what let Windows paint
/// minimise/maximise/close buttons across the top of the pill once the window
/// has a region, and what make the pill a window the user can close on its own —
/// neither of which a transient recording cue should ever be.
///
/// `SWP_NOACTIVATE` is load-bearing: injection targets the previously focused
/// app, so nothing here may hand the overlay focus.
#[cfg(windows)]
fn strip_frame_styles(hwnd: windows::Win32::Foundation::HWND) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, GWL_STYLE,
        SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_NOZORDER,
        WS_BORDER, WS_CAPTION, WS_DLGFRAME, WS_EX_CLIENTEDGE, WS_EX_DLGMODALFRAME,
        WS_EX_WINDOWEDGE, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU, WS_THICKFRAME,
    };

    let framed = (WS_CAPTION.0
        | WS_BORDER.0
        | WS_DLGFRAME.0
        | WS_SYSMENU.0
        | WS_THICKFRAME.0
        | WS_MINIMIZEBOX.0
        | WS_MAXIMIZEBOX.0) as isize;
    let framed_ex = (WS_EX_WINDOWEDGE.0 | WS_EX_CLIENTEDGE.0 | WS_EX_DLGMODALFRAME.0) as isize;

    // SAFETY: plain Win32 style getters/setters on a handle already validated by
    // the caller. Neither call has an out-param.
    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) };
    let ex_style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) };
    let want = (style & !framed) | WS_POPUP.0 as isize;
    let want_ex = ex_style & !framed_ex;
    if style == want && ex_style == want_ex {
        return false;
    }

    unsafe {
        SetWindowLongPtrW(hwnd, GWL_STYLE, want);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, want_ex);
        // A style change is only honoured once the frame is recalculated, and
        // SetWindowPos is the documented way to ask for that.
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED
                | SWP_NOMOVE
                | SWP_NOSIZE
                | SWP_NOZORDER
                | SWP_NOOWNERZORDER
                | SWP_NOACTIVATE,
        );
    }
    true
}

/// Draw one frame of the pill + pulsing circle, and schedule the next frame.
fn paint(ui: &mut egui::Ui, meter: &LevelMeter, recording: &AtomicBool) {
    let ctx = ui.ctx();

    // Ask the parent for a pass: only a parent pass can retire this viewport
    // for real (egui tears a deferred viewport down at the end of the pass that
    // stopped registering it, and this viewport's own repaints are not that
    // pass). Asked for first so it is asked for on the way out too.
    ctx.request_repaint_after_for(Duration::from_millis(100), egui::ViewportId::ROOT);

    // The dictation is over. Go away now, on this window's own pass, rather
    // than waiting for a parent that may be seconds from its next one — that
    // gap is the whole "the pill stays up until I minimise Hark" bug. Hiding
    // also stops this callback running at all (eframe skips the viewport
    // callback for an invisible window), which ends the 60 fps repaint that was
    // starving the parent of paints in the first place. The window itself is
    // destroyed by the parent, whenever it gets there, with nothing on screen
    // in the meantime.
    if !recording.load(Ordering::Relaxed) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        return;
    }

    #[cfg(windows)]
    reposition(ctx);
    #[cfg(windows)]
    shape_to_capsule();
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
    let rect = ui.max_rect();
    let center = rect.center();

    // The capsule fills the whole window. On Windows the OS clips the window
    // to this shape (`clip_to_capsule`); on macOS the window is transparent
    // outside it. Either way there is no opaque margin around the pill.
    let corner = egui::CornerRadius::same((rect.height() / 2.0) as u8);
    painter.rect_filled(rect, corner, theme::OVERLAY_PILL_FILL);
    painter.rect_stroke(
        rect,
        corner,
        egui::Stroke::new(1.0, theme::OVERLAY_PILL_STROKE),
        egui::StrokeKind::Inside,
    );

    let accent = theme::OVERLAY_ACCENT;
    let radius = CIRCLE_BASE + pulse * CIRCLE_PULSE;
    // The dot sits at the capsule's left; the label follows it.
    let dot = egui::pos2(rect.left() + 22.0, center.y);

    // A soft glow: two translucent rings that bloom with the pulse.
    for (scale, base_alpha) in [(2.1_f32, 26.0_f32), (1.5, 44.0)] {
        let alpha = (base_alpha * pulse) as u8;
        if alpha > 0 {
            painter.circle_filled(
                dot,
                radius * scale,
                egui::Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), alpha),
            );
        }
    }
    // The core dot.
    painter.circle_filled(dot, radius, accent);

    // "Listening…" in neutral-200, to the right of the dot.
    painter.text(
        egui::pos2(dot.x + 18.0, center.y),
        egui::Align2::LEFT_CENTER,
        "Listening\u{2026}",
        egui::FontId::proportional(13.0),
        theme::OVERLAY_TEXT,
    );
}

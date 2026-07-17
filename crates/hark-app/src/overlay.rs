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
/// frame the chord is held; stop calling it to dismiss the overlay. `monitor`
/// is the primary monitor's logical size when known, used to place the pill
/// bottom-centre; without it the OS chooses the position.
pub fn show(ctx: &egui::Context, meter: Arc<LevelMeter>, monitor: Option<egui::Vec2>) {
    let id = egui::ViewportId::from_hash_of("hark_recording_overlay");

    let mut builder = egui::ViewportBuilder::default()
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

    if let Some(monitor) = monitor {
        let x = (monitor.x - WINDOW.x) / 2.0;
        let y = monitor.y - WINDOW.y - monitor.y * BOTTOM_MARGIN_FRAC;
        builder = builder.with_position(egui::pos2(x.max(0.0), y.max(0.0)));
    }

    ctx.show_viewport_deferred(id, builder, move |ui, _class| {
        paint(ui, &meter);
    });
}

/// Draw one frame of the pill + pulsing circle, and schedule the next frame.
fn paint(ui: &mut egui::Ui, meter: &LevelMeter) {
    let ctx = ui.ctx();
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

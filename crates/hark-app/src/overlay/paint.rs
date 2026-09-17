use super::{feedback::State, Feedback};
use crate::theme;
use hark_pipeline::LevelMeter;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub(super) fn paint(
    ui: &mut egui::Ui,
    meter: &LevelMeter,
    recording: &AtomicBool,
    feedback: &Feedback,
    monitor: Option<egui::Vec2>,
) {
    let ctx = ui.ctx();
    ctx.request_repaint_after_for(Duration::from_millis(100), egui::ViewportId::ROOT);
    // Shape the persistent window while hidden, before revealing it. Preserve
    // the nonactivating native path: insertion still targets the user's app.
    #[cfg(windows)]
    super::shape_to_capsule();
    let state = feedback.state(recording.load(Ordering::Relaxed));
    if state == State::Hidden {
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        return;
    }
    super::place(ctx, monitor);
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    let animated = ui.style().animation_time > 0.0;
    let moving = matches!(
        state,
        State::Listening | State::Processing | State::LoadingModel
    );
    ctx.request_repaint_after(Duration::from_millis(if animated && moving {
        33
    } else {
        100
    }));
    let time = if animated {
        ui.input(|i| i.time) as f32
    } else {
        0.0
    };
    let amp = ctx.animate_value_with_time(
        egui::Id::new("hark_overlay_amp"),
        (meter.level().sqrt() * 1.25).clamp(0.0, 1.0),
        if animated { 0.09 } else { 0.0 },
    );
    let painter = ui.painter();
    let rect = ui.max_rect();
    let corner = egui::CornerRadius::same((rect.height() / 2.0) as u8);
    painter.rect_filled(rect, corner, theme::OVERLAY_PILL_FILL);
    painter.rect_stroke(
        rect,
        corner,
        egui::Stroke::new(1.0, theme::OVERLAY_PILL_STROKE),
        egui::StrokeKind::Inside,
    );
    // A restrained light-catching upper edge gives the floating pill depth.
    painter.hline(
        rect.left() + rect.height() / 2.0..=rect.right() - rect.height() / 2.0,
        rect.top() + 1.0,
        egui::Stroke::new(1.0, theme::OVERLAY_HIGHLIGHT),
    );
    let origin = egui::pos2(rect.left() + theme::OVERLAY_INSET, rect.center().y);
    let (icon, color) = match state {
        State::Inserted => (theme::icons::CHECK, theme::SUCCESS),
        State::Quiet => (theme::icons::MICROPHONE, theme::WARNING),
        State::AudioError | State::ProviderError | State::InsertError => {
            (theme::icons::WARNING, theme::DANGER)
        }
        _ => (theme::icons::CIRCLE_NOTCH, theme::OVERLAY_ACCENT),
    };
    if state == State::Listening {
        for index in 0..5 {
            let phase = (time * 5.0 + index as f32 * 1.2).sin().abs();
            let height = theme::OVERLAY_WAVE_WIDTH
                + theme::OVERLAY_WAVE_HEIGHT * amp * (0.45 + phase * 0.55);
            let bar = egui::Rect::from_center_size(
                egui::pos2(
                    origin.x + index as f32 * (theme::OVERLAY_WAVE_WIDTH + theme::OVERLAY_WAVE_GAP),
                    origin.y,
                ),
                egui::vec2(theme::OVERLAY_WAVE_WIDTH, height),
            );
            painter.rect_filled(bar, theme::OVERLAY_WAVE_WIDTH / 2.0, color);
        }
    } else if matches!(state, State::Processing | State::LoadingModel) {
        let center = origin + egui::vec2(theme::OVERLAY_WAVE_HEIGHT / 2.0, 0.0);
        let points = (0..=24)
            .map(|index| {
                let angle = time * 3.0 + index as f32 / 24.0 * std::f32::consts::TAU * 0.75;
                center + egui::vec2(angle.cos(), angle.sin()) * (theme::OVERLAY_ICON_SIZE / 2.0)
            })
            .collect();
        painter.add(egui::Shape::line(
            points,
            egui::Stroke::new(theme::OVERLAY_SPINNER_STROKE, color),
        ));
    } else {
        painter.text(
            origin + egui::vec2(theme::OVERLAY_WAVE_HEIGHT / 2.0, 0.0),
            egui::Align2::CENTER_CENTER,
            icon,
            egui::FontId::new(theme::OVERLAY_ICON_SIZE, theme::icon()),
            color,
        );
    }
    painter.text(
        origin + egui::vec2(theme::OVERLAY_LABEL_OFFSET, 0.0),
        egui::Align2::LEFT_CENTER,
        state.label(),
        egui::FontId::proportional(theme::OVERLAY_FONT),
        theme::OVERLAY_TEXT,
    );
}

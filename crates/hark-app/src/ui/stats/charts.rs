//! Bounded native charts with date/value tooltips and keyboard-readable data.
use crate::theme;
use crate::ui::format;
use egui::{Align2, FontId, Rect, RichText, Sense, Stroke, Ui};
use hark_store::{DailyActivity, Insights};
use jiff::civil::Date;

struct Bucket {
    first: Date,
    last: Date,
    words: i64,
}

fn buckets(days: &[DailyActivity]) -> Vec<Bucket> {
    let size = if days.len() > 30 { 7 } else { 1 };
    days.chunks(size)
        .map(|chunk| Bucket {
            first: chunk[0].date,
            last: chunk[chunk.len() - 1].date,
            words: chunk.iter().map(|day| day.words).sum(),
        })
        .collect()
}

fn date_label(date: Date) -> String {
    format!("{}/{}", date.month(), date.day())
}

pub fn daily(ui: &mut Ui, days: &[DailyActivity]) {
    let values = buckets(days);
    if values.is_empty() {
        ui.label("No dated activity is available.");
        return;
    }
    ui.label(
        RichText::new(format!(
            "{} – {} · words per {}",
            values[0].first,
            values[values.len() - 1].last,
            if days.len() > 30 {
                "7-day group"
            } else {
                "day"
            }
        ))
        .small()
        .weak(),
    );
    ui.add_space(theme::ROW_GAP);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(
            ui.available_width(),
            theme::CHART_HEIGHT + theme::ROW_GAP * 2.0,
        ),
        Sense::hover(),
    );
    let plot = Rect::from_min_max(
        rect.min + egui::vec2(theme::CHART_AXIS_WIDTH, theme::GAP),
        rect.max - egui::vec2(0.0, theme::ROW_GAP),
    );
    let max = values.iter().map(|day| day.words).max().unwrap_or(0).max(1);
    let font = FontId::proportional(theme::META_SIZE);
    let color = ui.visuals().weak_text_color();
    for ratio in [0.0, 0.5, 1.0] {
        let y = plot.bottom() - plot.height() * ratio;
        ui.painter().hline(
            plot.x_range(),
            y,
            Stroke::new(1.0, theme::divider(ui.visuals())),
        );
        let value = (max as f32 * ratio).round() as i64;
        let label = if value >= 1_000 {
            format!("{:.1}k", value as f32 / 1_000.0)
        } else {
            value.to_string()
        };
        ui.painter().text(
            egui::pos2(plot.left() - theme::GAP, y),
            Align2::RIGHT_CENTER,
            label,
            font.clone(),
            color,
        );
    }
    let stride = plot.width() / values.len() as f32;
    let gap = theme::CHART_BAR_GAP.min(stride / 3.0);
    for (i, day) in values.iter().enumerate() {
        let left = plot.left() + i as f32 * stride + gap / 2.0;
        let height = plot.height() * day.words.max(0) as f32 / max as f32;
        let bar = Rect::from_min_max(
            egui::pos2(left, plot.bottom() - height),
            egui::pos2(left + stride - gap, plot.bottom()),
        );
        let fill = if i + 7 >= values.len() {
            theme::chart(ui.visuals())
        } else {
            theme::chart_soft(ui.visuals())
        };
        ui.painter().rect_filled(bar, theme::CHART_BAR_GAP, fill);
        let hit = Rect::from_min_max(
            egui::pos2(left, plot.top()),
            egui::pos2(left + stride, plot.bottom()),
        );
        ui.interact(hit, ui.id().with(("daily-bar", i)), Sense::hover())
            .on_hover_text(format!(
                "{} – {}: {} words",
                day.first,
                day.last,
                format::count(day.words)
            ));
    }
    let ticks = if plot.width() >= theme::MIN_CARD_WIDTH {
        4
    } else {
        2
    };
    for i in 0..ticks {
        let index = i * (values.len() - 1) / (ticks - 1);
        let x = plot.left() + plot.width() * i as f32 / (ticks - 1) as f32;
        ui.painter().text(
            egui::pos2(x, plot.bottom() + theme::GAP),
            if i == 0 {
                Align2::LEFT_TOP
            } else if i == ticks - 1 {
                Align2::RIGHT_TOP
            } else {
                Align2::CENTER_TOP
            },
            date_label(if i == ticks - 1 {
                values[index].last
            } else {
                values[index].first
            }),
            font.clone(),
            color,
        );
    }
    let sum: i64 = days.iter().map(|day| day.words).sum();
    let active = days.iter().filter(|day| day.dictations > 0).count();
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(format!(
                "{} words/day on average · {active} active days",
                format::count(sum / days.len() as i64)
            ))
            .small()
            .weak(),
        );
        ui.label(
            RichText::new(format!(
                "Best day: {} words",
                format::count(days.iter().map(|day| day.words).max().unwrap_or(0))
            ))
            .small()
            .weak(),
        );
    });
    egui::CollapsingHeader::new("Read daily values")
        .id_salt("daily-values")
        .show(ui, |ui| {
            for day in days {
                ui.label(format!(
                    "{} · {} words · {} dictations",
                    day.date,
                    format::count(day.words),
                    day.dictations
                ));
            }
        });
}

pub fn activity(ui: &mut Ui, data: &Insights) {
    ui.label(RichText::new("Showing up adds up.").text_style(theme::subheading()));
    ui.label(RichText::new(format!("{} days", data.current_streak)).font(theme::hero_font()));
    ui.label(
        RichText::new("Current streak · today or yesterday")
            .small()
            .weak(),
    );
    ui.add_space(theme::ROW_GAP);
    let days = &data.activity[data.activity.len().saturating_sub(90)..];
    let Some(first) = days.first() else {
        return;
    };
    let leading = first.date.weekday().to_sunday_zero_offset() as usize;
    let weeks = (leading + days.len()).div_ceil(7);
    let width = ui.available_width();
    let step = ((width - theme::CHART_AXIS_WIDTH) / weeks as f32)
        .min(theme::ROW_GAP + theme::HEAT_CELL_GAP);
    let cell = (step - theme::HEAT_CELL_GAP).max(1.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, step * 7.0), Sense::hover());
    let max = days.iter().map(|day| day.words).max().unwrap_or(0).max(1);
    for (row, label) in [(0, "Sun"), (2, "Tue"), (4, "Thu"), (6, "Sat")] {
        ui.painter().text(
            rect.min + egui::vec2(0.0, step * row as f32 + cell / 2.0),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(theme::META_SIZE),
            ui.visuals().weak_text_color(),
        );
    }
    for (i, day) in days.iter().enumerate() {
        let index = leading + i;
        let pos = rect.min
            + egui::vec2(
                theme::CHART_AXIS_WIDTH + (index / 7) as f32 * step,
                (index % 7) as f32 * step,
            );
        let cell_rect = Rect::from_min_size(pos, egui::vec2(cell, cell));
        let fill = if day.words == 0 {
            theme::tint(ui.visuals())
        } else {
            theme::chart_soft(ui.visuals())
                .lerp_to_gamma(theme::chart(ui.visuals()), day.words as f32 / max as f32)
        };
        ui.painter()
            .rect_filled(cell_rect, theme::HEAT_CELL_GAP / 2.0, fill);
        ui.interact(
            cell_rect,
            ui.id().with(("activity-day", day.date)),
            Sense::hover(),
        )
        .on_hover_text(format!(
            "{} · {} words · {} dictations",
            day.date,
            format::count(day.words),
            day.dictations
        ));
    }
    ui.add_space(theme::GAP);
    ui.label(RichText::new(format!("Last 90 days · color intensity shows word count. Best observed streak: {} days in retained activity.", data.longest_streak)).small().weak());
    ui.label(
        RichText::new("Missing historical days may shorten observed streaks.")
            .small()
            .weak(),
    );
    egui::CollapsingHeader::new("Read activity dates")
        .id_salt("activity-dates")
        .show(ui, |ui| {
            for day in days.iter().filter(|day| day.dictations > 0) {
                ui.label(format!("{} · {} words", day.date, format::count(day.words)));
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weekly_groups_keep_partial_tail_and_exact_dates_and_totals() {
        let days: Vec<_> = (0..90)
            .map(|i| DailyActivity {
                date: jiff::civil::date(2026, 7, 3)
                    .checked_add(jiff::Span::new().days(i))
                    .unwrap(),
                words: i + 1,
                dictations: 1,
            })
            .collect();
        let grouped = buckets(&days);
        assert_eq!(grouped.len(), 13);
        assert_eq!(grouped.last().unwrap().first, days[84].date);
        assert_eq!(grouped.last().unwrap().last, days[89].date);
        assert_eq!(
            grouped.iter().map(|b| b.words).sum::<i64>(),
            days.iter().map(|d| d.words).sum::<i64>()
        );
    }

    #[test]
    fn charts_handle_empty_and_zero_days_in_small_native_surfaces() {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let days = vec![DailyActivity {
            date: jiff::civil::date(2026, 9, 30),
            words: 0,
            dictations: 0,
        }];
        for input in [&[][..], days.as_slice()] {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(320.0, 500.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default().show(ui, |ui| daily(ui, input));
                },
            );
            output.textures_delta.clear();
            for shape in output.shapes {
                assert!(shape.shape.visual_bounding_rect().is_finite());
            }
        }
    }
}

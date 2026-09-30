//! Metric cards and contextual panels. Missing historical inputs stay unknown.
use crate::theme;
use crate::ui::format;
use egui::{RichText, Ui};
use hark_config::Settings;
use hark_store::{InsightSummary, Insights, Stats};

struct Metric {
    label: &'static str,
    value: String,
    note: String,
}

fn metrics(ui: &mut Ui, values: &[Metric]) {
    let columns = if ui.available_width() >= theme::METRIC_MIN_WIDTH * 4.0 + theme::GAP * 3.0 {
        4
    } else if ui.available_width() >= theme::METRIC_MIN_WIDTH * 2.0 + theme::GAP {
        2
    } else {
        1
    };
    for row in values.chunks(columns) {
        ui.columns(row.len(), |columns| {
            for (ui, metric) in columns.iter_mut().zip(row) {
                theme::card(ui, |ui| {
                    ui.label(RichText::new(metric.label).small().weak());
                    ui.add_space(theme::ROW_GAP);
                    ui.label(RichText::new(&metric.value).size(theme::STAT_SIZE));
                    ui.add_space(theme::GAP);
                    ui.label(RichText::new(&metric.note).small().weak());
                });
            }
        });
        ui.add_space(theme::GAP);
    }
}

fn ms(value: Option<i64>) -> String {
    value.map_or_else(|| "—".into(), |v| format!("{:.2} s", v as f64 / 1000.0))
}

pub fn overview_metrics(ui: &mut Ui, s: &InsightSummary) {
    let coverage = if s.timed_dictations == s.dictations && s.dictations > 0 {
        "Estimated from clip duration".into()
    } else {
        format!("{} of {} clips measured", s.timed_dictations, s.dictations)
    };
    metrics(
        ui,
        &[
            Metric {
                label: "Words dictated",
                value: format::count(s.words),
                note: format!("{} dictations", format::count(s.dictations)),
            },
            Metric {
                label: "Time saved",
                value: s
                    .estimated_saved_ms()
                    .map(format::duration)
                    .unwrap_or_else(|| "—".into()),
                note: format!(
                    "Estimate vs. 40 WPM · {} measured clips",
                    s.timed_dictations
                ),
            },
            Metric {
                label: "Dictation pace",
                value: s
                    .estimated_wpm()
                    .map_or_else(|| "—".into(), |v| format!("{v:.0} wpm")),
                note: coverage,
            },
            Metric {
                label: "Median latency",
                value: ms(s.median_ms),
                note: "Release → text at cursor".into(),
            },
        ],
    );
}

pub fn apps(ui: &mut Ui, data: &Insights, settings: &Settings) -> bool {
    ui.label(RichText::new("Where you find your words").text_style(theme::subheading()));
    ui.label(
        RichText::new("Dictations by app · selected period")
            .small()
            .weak(),
    );
    ui.add_space(theme::ROW_GAP);
    if data.apps.is_empty() {
        ui.label(if settings.insights.track_apps { "No app labels have been recorded yet. Detection may be unavailable on your desktop." }
            else { "App statistics are off. Enable optional local app tracking in Privacy to see them here." });
    } else {
        for app in data.apps.iter().take(5) {
            let share = app.dictations as f32 / data.period.dictations.max(1) as f32;
            ui.label(format!(
                "{} · {} dictations ({:.0}%)",
                app.label,
                app.dictations,
                share * 100.0
            ));
            ui.add(
                egui::ProgressBar::new(share)
                    .fill(theme::chart(ui.visuals()))
                    .desired_height(theme::GAP),
            );
            ui.add_space(theme::GAP);
        }
        if data.apps.len() > 5 {
            ui.label(
                RichText::new(format!("{} other recorded apps", data.apps.len() - 5))
                    .small()
                    .weak(),
            );
        }
    }
    if data.unknown_app_dictations > 0 {
        ui.label(
            RichText::new(format!(
                "{} dictations have no app label.",
                data.unknown_app_dictations
            ))
            .small()
            .weak(),
        );
    }
    if !settings.insights.track_apps && !data.apps.is_empty() {
        ui.label(RichText::new("Collection is off. Previously recorded labels remain until stats are reset or expire.").small().weak());
    }
    #[cfg(target_os = "linux")]
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        ui.label(
            RichText::new("App detection is unavailable on Wayland.")
                .small()
                .weak(),
        );
    }
    ui.add_space(theme::GAP);
    ui.button("Privacy settings").clicked()
}

pub fn voice(ui: &mut Ui, data: &Insights, settings: &Settings) -> bool {
    let s = &data.period;
    theme::card(ui, |ui| {
        ui.label(RichText::new("YOUR RHYTHM").small().weak());
        ui.label(
            RichText::new(data.peak_hour.map_or_else(
                || "Your next idea starts here.".into(),
                |hour| format!("Your busiest hour: {hour:02}:00–{hour:02}:59"),
            ))
            .font(theme::hero_font()),
        );
        ui.label(
            RichText::new(
                "Based on recorded dictation times in the selected period, in your local timezone.",
            )
            .small()
            .weak(),
        );
    });
    ui.add_space(theme::ROW_GAP);
    metrics(
        ui,
        &[
            Metric {
                label: "Spellbook terms",
                value: format::count(settings.spellbook.entries.len() as i64),
                note: "Your current vocabulary".into(),
            },
            Metric {
                label: "Dictionary corrections",
                value: if s.measured_corrections > 0 {
                    format::count(s.corrections)
                } else {
                    "—".into()
                },
                note: format!(
                    "{} of {} dictations measured",
                    s.measured_corrections, s.dictations
                ),
            },
            Metric {
                label: "Invocations used",
                value: format::count(s.invocations),
                note: "Selected period".into(),
            },
            Metric {
                label: "Invocation output",
                value: format::count(s.expanded_words),
                note: "Output words, including surrounding speech".into(),
            },
        ],
    );
    ui.add_space(theme::ROW_GAP);
    let mut privacy = false;
    theme::card(ui, |ui| {
        ui.label(RichText::new("Words and phrases you return to").text_style(theme::subheading()));
        ui.add_space(theme::ROW_GAP);
        if let Some(patterns) = &data.patterns {
            if patterns.words.is_empty() {
                ui.label("There isn’t enough retained text in this period yet.");
            } else {
                for word in patterns.words.iter().take(5) {
                    ui.label(format!("{} · {} uses", word.text, word.count));
                }
                ui.add_space(theme::ROW_GAP);
                if let Some(phrase) = patterns.phrases.first() {
                    ui.label(RichText::new(format!("“{}”", phrase.text)).font(theme::hero_font()));
                    ui.label(
                        RichText::new(format!("{} uses in analyzed history", phrase.count))
                            .small()
                            .weak(),
                    );
                } else {
                    ui.label("No repeated phrase in the analyzed history yet.");
                }
            }
            ui.add_space(theme::ROW_GAP);
            ui.label(RichText::new(format!("{} of {} retained dictations analyzed locally. Common words and invocation expansions are excluded.", patterns.sampled_dictations, patterns.matching_dictations)).small().weak());
            if patterns.truncated {
                ui.label(
                    RichText::new("Analysis is limited to recent history to keep Hark responsive.")
                        .small()
                        .weak(),
                );
            }
        } else {
            ui.label("Word and phrase insights are off. Optional analysis runs on this device using retained history; it does not upload transcripts.");
        }
        ui.add_space(theme::GAP);
        privacy = ui.button("Privacy settings").clicked();
    });
    privacy
}

pub fn performance(ui: &mut Ui, data: &Insights) {
    let s = &data.period;
    metrics(
        ui,
        &[
            Metric {
                label: "Median latency",
                value: ms(s.median_ms),
                note: "Half of recorded completions".into(),
            },
            Metric {
                label: "95th percentile",
                value: ms(s.p95_ms),
                note: "95% of recorded completions".into(),
            },
            Metric {
                label: "Completed dictations",
                value: format::count(s.dictations),
                note: "Selected period".into(),
            },
            Metric {
                label: "Average clip",
                value: s
                    .mean_clip_ms()
                    .map(format::duration)
                    .unwrap_or_else(|| "—".into()),
                note: format!("{} measured clips", s.timed_dictations),
            },
        ],
    );
    ui.add_space(theme::ROW_GAP);
    theme::card(ui, |ui| {
        ui.label(
            RichText::new("From key release to your next sentence").text_style(theme::subheading()),
        );
        ui.label(RichText::new("Latency is the wait after you release the shortcut until text is delivered. These are completed dictations, not a success-rate measure.").small().weak());
        ui.add_space(theme::ROW_GAP);
        if data.providers.is_empty() {
            ui.label("No recorded completions in this period.");
        }
        for provider in &data.providers {
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(&provider.label).strong());
                ui.label(format!(
                    "{} dictations · median {} · p95 {}",
                    format::count(provider.dictations),
                    ms(provider.median_ms),
                    ms(provider.p95_ms)
                ));
            });
            ui.add_space(theme::GAP);
        }
    });
    ui.add_space(theme::ROW_GAP);
    theme::card(ui, |ui| {
        ui.label(RichText::new("Your cleanup voices").text_style(theme::subheading()));
        for voice in &data.voices {
            ui.label(format!(
                "{} · {} dictations",
                voice.label,
                format::count(voice.dictations)
            ));
        }
    });
}

pub fn lifetime(ui: &mut Ui, s: &Stats) {
    ui.label(format!(
        "{} dictations · {} words · {} captured audio",
        format::count(s.dictations),
        format::count(s.words),
        format::duration(s.audio_ms)
    ));
    let mean = (s.total_ms > 0 && s.dictations > 0).then(|| s.total_ms / s.dictations);
    ui.label(format!(
        "Average release to insert: {} · estimated time saved: {}",
        ms(mean),
        format::duration(format::time_saved_ms(s.words, s.audio_ms))
    ));
}

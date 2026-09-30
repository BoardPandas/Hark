use super::patterns;
use super::persistence::DAY_MS;
use super::types::*;
use crate::{Store, StoreError};
use jiff::{civil::Date, Span, Timestamp};
use rusqlite::params;
use std::collections::BTreeMap;

struct Event {
    ts_ms: i64,
    words: i64,
    audio_ms: Option<i64>,
    total_ms: i64,
    provider: String,
    voice: String,
    invocation: bool,
    expanded_words: i64,
    corrections: Option<i64>,
    app: Option<String>,
    legacy: bool,
}

#[derive(Default)]
struct Accumulator {
    totals: InsightSummary,
    latencies: Vec<i64>,
}

impl Accumulator {
    fn add(&mut self, event: &Event) {
        let t = &mut self.totals;
        t.dictations += 1;
        t.words += event.words;
        t.total_ms += event.total_ms;
        if let Some(audio_ms) = event.audio_ms {
            t.audio_ms += audio_ms;
            t.timed_dictations += 1;
            t.timed_words += event.words;
        }
        if let Some(corrections) = event.corrections {
            t.corrections += corrections;
            t.measured_corrections += 1;
        }
        t.invocations += i64::from(event.invocation);
        t.expanded_words += event.expanded_words;
        self.latencies.push(event.total_ms);
    }

    fn finish(mut self) -> InsightSummary {
        self.latencies.sort_unstable();
        let n = self.latencies.len();
        if n > 0 {
            self.totals.median_ms = Some(if n.is_multiple_of(2) {
                let sum = i128::from(self.latencies[n / 2 - 1]) + i128::from(self.latencies[n / 2]);
                (sum / 2) as i64
            } else {
                self.latencies[n / 2]
            });
            self.totals.p95_ms = Some(self.latencies[(n * 95).div_ceil(100) - 1]);
        }
        self.totals
    }
}

impl Store {
    /// Aggregate numeric facts and optional retained text off the UI thread.
    /// The range and retained event window are bounded; nothing is uploaded.
    pub fn insights(&self, request: &InsightsRequest) -> Result<Insights, StoreError> {
        if !(1..=INSIGHTS_RETENTION_DAYS as u16).contains(&request.days) {
            return Err(StoreError::InsightsRequest(
                "days must be between 1 and 366",
            ));
        }
        let today = Timestamp::from_millisecond(request.now_ms)?
            .to_zoned(request.time_zone.clone())
            .date();
        let first = today.checked_sub(Span::new().days(i64::from(request.days) - 1))?;
        let previous_first = first.checked_sub(Span::new().days(i64::from(request.days)))?;
        let activity_first = today.checked_sub(Span::new().days(INSIGHTS_RETENTION_DAYS - 1))?;
        let start_ms = first
            .to_zoned(request.time_zone.clone())?
            .timestamp()
            .as_millisecond();
        let previous_ms = previous_first
            .to_zoned(request.time_zone.clone())?
            .timestamp()
            .as_millisecond();
        let query_ms = activity_first
            .min(previous_first)
            .to_zoned(request.time_zone.clone())?
            .timestamp()
            .as_millisecond();
        let retained_ms = request
            .now_ms
            .saturating_sub(INSIGHTS_RETENTION_DAYS * DAY_MS);

        let mut period = Accumulator::default();
        let mut previous = Accumulator::default();
        let mut days = BTreeMap::<Date, (i64, i64)>::new();
        let mut providers = BTreeMap::<String, Accumulator>::new();
        let mut voices = BTreeMap::<String, Accumulator>::new();
        let mut apps = BTreeMap::<String, Accumulator>::new();
        let mut hours = [0_i64; 24];
        let mut unknown_app_dictations = 0;
        let mut backfilled_dictations = 0;
        let mut oldest_event_ms = None;
        let mut query = self.conn.prepare(
            "SELECT ts_ms, words, audio_ms, total_ms, stt_provider, voice, invocation, \
             expanded_words, spellbook_replacements, foreground_app, legacy \
             FROM insight_events WHERE ts_ms >= ?1 AND ts_ms <= ?2 ORDER BY ts_ms, id",
        )?;
        let rows = query.query_map(params![query_ms.max(retained_ms), request.now_ms], |r| {
            Ok(Event {
                ts_ms: r.get(0)?,
                words: r.get(1)?,
                audio_ms: r.get(2)?,
                total_ms: r.get(3)?,
                provider: r.get(4)?,
                voice: r.get(5)?,
                invocation: r.get(6)?,
                expanded_words: r.get(7)?,
                corrections: r.get(8)?,
                app: r.get(9)?,
                legacy: r.get(10)?,
            })
        })?;
        for row in rows {
            let event = row?;
            oldest_event_ms.get_or_insert(event.ts_ms);
            let local =
                Timestamp::from_millisecond(event.ts_ms)?.to_zoned(request.time_zone.clone());
            let date = local.date();
            let day = days.entry(date).or_default();
            day.0 += 1;
            day.1 += event.words;
            if event.ts_ms >= start_ms {
                period.add(&event);
                providers
                    .entry(event.provider.clone())
                    .or_default()
                    .add(&event);
                voices.entry(event.voice.clone()).or_default().add(&event);
                if let Some(app) = &event.app {
                    apps.entry(app.clone()).or_default().add(&event);
                } else {
                    unknown_app_dictations += 1;
                }
                hours[local.hour() as usize] += 1;
                backfilled_dictations += i64::from(event.legacy);
            } else if event.ts_ms >= previous_ms {
                previous.add(&event);
            }
        }
        let tracking_since_ms: i64 = self.conn.query_row(
            "SELECT since_ts_ms FROM insight_tracking WHERE id = 1",
            [],
            |r| r.get(0),
        )?;
        let activity = daily_series(activity_first, INSIGHTS_RETENTION_DAYS as u16, &days)?;
        let (current_streak, longest_streak) = streaks(&activity);
        // Iteration order plus a strict comparison keeps ties deterministic.
        let mut peak_hour = None;
        let mut highest = 0;
        for (hour, count) in hours.into_iter().enumerate() {
            if count > highest {
                peak_hour = Some(hour as u8);
                highest = count;
            }
        }
        Ok(Insights {
            lifetime: self.stats()?,
            period: period.finish(),
            previous: previous.finish(),
            daily: daily_series(first, request.days, &days)?,
            activity,
            current_streak,
            longest_streak,
            providers: breakdown(providers),
            voices: breakdown(voices),
            apps: breakdown(apps),
            unknown_app_dictations,
            peak_hour,
            coverage: InsightCoverage {
                tracking_since_ms,
                oldest_event_ms,
                backfilled_dictations,
                period_complete: tracking_since_ms.max(retained_ms) <= start_ms,
                previous_complete: tracking_since_ms.max(retained_ms) <= previous_ms,
            },
            patterns: if request.analyze_text {
                Some(patterns::analyze(&self.conn, start_ms, request)?)
            } else {
                None
            },
        })
    }
}

fn daily_series(
    first: Date,
    count: u16,
    values: &BTreeMap<Date, (i64, i64)>,
) -> Result<Vec<DailyActivity>, StoreError> {
    (0..count)
        .map(|offset| {
            let date = first.checked_add(Span::new().days(offset))?;
            let (dictations, words) = values.get(&date).copied().unwrap_or_default();
            Ok(DailyActivity {
                date,
                dictations,
                words,
            })
        })
        .collect()
}

fn streaks(activity: &[DailyActivity]) -> (u32, u32) {
    let mut longest = 0;
    let mut run = 0;
    for day in activity {
        run = if day.dictations > 0 { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    let skip_today = usize::from(activity.last().is_some_and(|d| d.dictations == 0));
    let current = activity
        .iter()
        .rev()
        .skip(skip_today)
        .take_while(|day| day.dictations > 0)
        .count() as u32;
    (current, longest)
}

fn breakdown(values: BTreeMap<String, Accumulator>) -> Vec<UsageBreakdown> {
    let mut values: Vec<_> = values
        .into_iter()
        .map(|(label, acc)| {
            let totals = acc.finish();
            UsageBreakdown {
                label,
                dictations: totals.dictations,
                words: totals.words,
                median_ms: totals.median_ms,
                p95_ms: totals.p95_ms,
            }
        })
        .collect();
    values.sort_by(|a, b| {
        b.dictations
            .cmp(&a.dictations)
            .then_with(|| a.label.cmp(&b.label))
    });
    values
}

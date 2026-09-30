//! Public, UI-independent insights data. Content-bearing results deliberately
//! have no `Debug` implementation: words and phrases stay out of logs.

use crate::Stats;
use jiff::{civil::Date, tz::TimeZone};

pub const INSIGHTS_RETENTION_DAYS: i64 = 366;

#[derive(Clone)]
pub struct InsightsRequest {
    /// Calendar days ending today, inclusive. Valid range: 1..=366.
    pub days: u16,
    pub now_ms: i64,
    pub time_zone: TimeZone,
    /// Explicit opt-in. Reads retained transcripts locally, on the worker.
    pub analyze_text: bool,
}

pub struct Insights {
    pub lifetime: Stats,
    pub period: InsightSummary,
    pub previous: InsightSummary,
    /// Selected range, with a zero-valued point for every missing date.
    pub daily: Vec<DailyActivity>,
    /// 366 local calendar dates ending today, for heatmaps and streaks.
    pub activity: Vec<DailyActivity>,
    /// Active consecutive days ending today, or yesterday if today is empty.
    pub current_streak: u32,
    /// Longest observed run within the retained 366-day window.
    pub longest_streak: u32,
    pub providers: Vec<UsageBreakdown>,
    pub voices: Vec<UsageBreakdown>,
    /// Known app labels only; unknowns are counted separately.
    pub apps: Vec<UsageBreakdown>,
    pub unknown_app_dictations: i64,
    /// Most dictations in a local hour, 0..23; ties choose the earlier hour.
    pub peak_hour: Option<u8>,
    pub coverage: InsightCoverage,
    pub patterns: Option<VoicePatterns>,
}

#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct InsightSummary {
    pub dictations: i64,
    pub words: i64,
    /// Sum only for rows with measured clip duration.
    pub audio_ms: i64,
    pub timed_dictations: i64,
    /// Words on exactly those rows that contributed to `audio_ms`.
    pub timed_words: i64,
    pub total_ms: i64,
    pub median_ms: Option<i64>,
    pub p95_ms: Option<i64>,
    /// Actual spellbook replacements, including both correction passes.
    pub corrections: i64,
    pub measured_corrections: i64,
    pub invocations: i64,
    /// Full output words on rows where an invocation fired, separate from
    /// dictated `words`. Anywhere-scope output includes surrounding speech.
    /// Display as "Invocation output", not the size of the canned expansion.
    pub expanded_words: i64,
}

impl InsightSummary {
    pub fn estimated_wpm(&self) -> Option<f64> {
        (self.audio_ms > 0).then(|| self.timed_words as f64 * 60_000.0 / self.audio_ms as f64)
    }

    pub fn estimated_saved_ms(&self) -> Option<i64> {
        (self.timed_dictations > 0)
            .then(|| (self.timed_words.saturating_mul(1_500) - self.audio_ms).max(0))
    }

    pub fn mean_clip_ms(&self) -> Option<i64> {
        (self.timed_dictations > 0).then(|| self.audio_ms / self.timed_dictations)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DailyActivity {
    pub date: Date,
    pub dictations: i64,
    pub words: i64,
}

pub struct UsageBreakdown {
    pub label: String,
    pub dictations: i64,
    pub words: i64,
    pub median_ms: Option<i64>,
    pub p95_ms: Option<i64>,
}

#[derive(Clone, Debug)]
pub struct InsightCoverage {
    /// New numeric tracking began at migration or the latest stats reset.
    pub tracking_since_ms: i64,
    pub oldest_event_ms: Option<i64>,
    pub backfilled_dictations: i64,
    /// True only when continuous tracking covers the whole selected period.
    pub period_complete: bool,
    pub previous_complete: bool,
}

pub struct VoicePatterns {
    pub words: Vec<PhraseCount>,
    pub phrases: Vec<PhraseCount>,
    pub sampled_dictations: i64,
    pub matching_dictations: i64,
    /// Analysis is bounded to 5,000 recent entries and 200,000 tokens.
    pub truncated: bool,
}

pub struct PhraseCount {
    pub text: String,
    pub count: i64,
}

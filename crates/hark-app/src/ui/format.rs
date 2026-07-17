//! Presentation formatting for the history and stats panels: relative
//! times, local-calendar day labels, durations, and counts. Pure functions
//! over millisecond timestamps; callers pass `now` and the timezone so
//! every path is testable without the wall clock.

use jiff::tz::TimeZone;
use jiff::Timestamp;

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// Caption-sized recency: "just now" through "3 d ago". Day grouping carries
/// the calendar context, so this never needs a date.
pub fn relative_time(ts_ms: i64, now_ms: i64) -> String {
    let delta = now_ms.saturating_sub(ts_ms);
    if delta < 60_000 {
        // Includes small negative skews; "in the future" is never honest.
        return "just now".to_string();
    }
    if delta < 3_600_000 {
        return format!("{} min ago", delta / 60_000);
    }
    if delta < 86_400_000 {
        return format!("{} h ago", delta / 3_600_000);
    }
    format!("{} d ago", delta / 86_400_000)
}

/// History day-group header: Today / Yesterday / "July 3" / "July 3, 2025"
/// (year only when it is not the current one). Calendar days in the user's
/// timezone, not 24-hour buckets.
pub fn day_label(ts_ms: i64, now_ms: i64, tz: &TimeZone) -> String {
    let (Some(date), Some(today)) = (local_date(ts_ms, tz), local_date(now_ms, tz)) else {
        return "Earlier".to_string();
    };
    if date == today {
        return "Today".to_string();
    }
    if Some(date) == today.yesterday().ok() {
        return "Yesterday".to_string();
    }
    let month = MONTHS[(date.month() as usize).saturating_sub(1).min(11)];
    if date.year() == today.year() {
        format!("{month} {}", date.day())
    } else {
        format!("{month} {}, {}", date.day(), date.year())
    }
}

/// Absolute date with the year, for the stats since-caption: "July 2, 2026".
pub fn date(ts_ms: i64, tz: &TimeZone) -> String {
    let Some(date) = local_date(ts_ms, tz) else {
        return "an unknown date".to_string();
    };
    let month = MONTHS[(date.month() as usize).saturating_sub(1).min(11)];
    format!("{month} {}, {}", date.day(), date.year())
}

/// Expanded-row timestamp: "July 16, 2026, 2:34 PM".
pub fn full_timestamp(ts_ms: i64, tz: &TimeZone) -> String {
    let Ok(ts) = Timestamp::from_millisecond(ts_ms) else {
        return "Unknown time".to_string();
    };
    let zoned = ts.to_zoned(tz.clone());
    let month = MONTHS[(zoned.month() as usize).saturating_sub(1).min(11)];
    let (hour, meridiem) = match zoned.hour() {
        0 => (12, "AM"),
        h @ 1..=11 => (h, "AM"),
        12 => (12, "PM"),
        h => (h - 12, "PM"),
    };
    format!(
        "{month} {}, {}, {hour}:{:02} {meridiem}",
        zoned.day(),
        zoned.year(),
        zoned.minute()
    )
}

/// Compact duration for stat cards and the time-saved line: "42 s",
/// "5 m 12 s", "1 h 4 m". Zero remainders are dropped ("2 m", "1 h").
pub fn duration(ms: i64) -> String {
    let secs = ms.max(0) / 1_000;
    let (hours, mins, secs) = (secs / 3_600, (secs % 3_600) / 60, secs % 60);
    if hours > 0 {
        if mins > 0 {
            return format!("{hours} h {mins} m");
        }
        return format!("{hours} h");
    }
    if mins > 0 {
        if secs > 0 {
            return format!("{mins} m {secs} s");
        }
        return format!("{mins} m");
    }
    format!("{secs} s")
}

/// Thousands-separated integer for stat cards.
pub fn count(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if n < 0 {
        out.push('-');
    }
    let lead = digits.len() % 3;
    for (i, c) in digits.chars().enumerate() {
        if i != 0 && (i + 3 - lead).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// "Time saved vs typing at 40 WPM": typing the same words at 40 WPM costs
/// 1.5 s per word; subtract the time actually spent speaking. Clamped at
/// zero (a slow speaker never "owes" time).
pub fn time_saved_ms(words: i64, audio_ms: i64) -> i64 {
    (words.saturating_mul(1_500))
        .saturating_sub(audio_ms)
        .max(0)
}

fn local_date(ts_ms: i64, tz: &TimeZone) -> Option<jiff::civil::Date> {
    let ts = Timestamp::from_millisecond(ts_ms).ok()?;
    Some(ts.to_zoned(tz.clone()).date())
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    /// Fixed zone (UTC-5, no DST) so calendar assertions never depend on
    /// the machine or the wall clock.
    fn tz() -> TimeZone {
        TimeZone::fixed(jiff::tz::offset(-5))
    }

    fn ms_at(d: jiff::civil::DateTime) -> i64 {
        d.to_zoned(tz())
            .expect("valid")
            .timestamp()
            .as_millisecond()
    }

    #[test]
    fn relative_time_buckets() {
        assert_eq!(relative_time(1_000, 1_500), "just now");
        assert_eq!(relative_time(1_000, 61_000), "1 min ago");
        assert_eq!(relative_time(0, 59 * 60_000), "59 min ago");
        assert_eq!(relative_time(0, 60 * 60_000), "1 h ago");
        assert_eq!(relative_time(0, 23 * 3_600_000), "23 h ago");
        assert_eq!(relative_time(0, 26 * 3_600_000), "1 d ago");
        assert_eq!(relative_time(5_000, 1_000), "just now", "skew tolerated");
    }

    #[test]
    fn day_labels_split_on_local_calendar_days_not_24h_buckets() {
        let now = ms_at(date(2026, 7, 16).at(1, 0, 0, 0));
        // 2 hours earlier but yesterday on the local calendar.
        let last_night = ms_at(date(2026, 7, 15).at(23, 0, 0, 0));
        assert_eq!(day_label(now, now, &tz()), "Today");
        assert_eq!(day_label(last_night, now, &tz()), "Yesterday");

        let same_year = ms_at(date(2026, 7, 3).at(12, 0, 0, 0));
        assert_eq!(day_label(same_year, now, &tz()), "July 3");
        let older = ms_at(date(2025, 12, 31).at(12, 0, 0, 0));
        assert_eq!(day_label(older, now, &tz()), "December 31, 2025");
    }

    #[test]
    fn date_always_carries_the_year() {
        let ts = ms_at(date(2026, 7, 2).at(9, 0, 0, 0));
        assert_eq!(super::date(ts, &tz()), "July 2, 2026");
    }

    #[test]
    fn full_timestamp_uses_twelve_hour_clock() {
        let afternoon = ms_at(date(2026, 7, 16).at(14, 34, 0, 0));
        assert_eq!(full_timestamp(afternoon, &tz()), "July 16, 2026, 2:34 PM");
        let midnight = ms_at(date(2026, 1, 5).at(0, 7, 0, 0));
        assert_eq!(full_timestamp(midnight, &tz()), "January 5, 2026, 12:07 AM");
        let noon = ms_at(date(2026, 1, 5).at(12, 0, 0, 0));
        assert_eq!(full_timestamp(noon, &tz()), "January 5, 2026, 12:00 PM");
    }

    #[test]
    fn durations_drop_zero_remainders() {
        assert_eq!(duration(0), "0 s");
        assert_eq!(duration(999), "0 s");
        assert_eq!(duration(42_000), "42 s");
        assert_eq!(duration(5 * 60_000 + 12_000), "5 m 12 s");
        assert_eq!(duration(2 * 60_000), "2 m");
        assert_eq!(duration(3_600_000 + 4 * 60_000), "1 h 4 m");
        assert_eq!(duration(2 * 3_600_000), "2 h");
        assert_eq!(duration(-5), "0 s", "negative input clamps");
    }

    #[test]
    fn counts_get_thousands_separators() {
        assert_eq!(count(0), "0");
        assert_eq!(count(999), "999");
        assert_eq!(count(1_000), "1,000");
        assert_eq!(count(1_234_567), "1,234,567");
    }

    #[test]
    fn time_saved_is_typing_minus_speaking_clamped_at_zero() {
        // 100 words at 40 WPM = 150 s typing; 60 s spoken => 90 s saved.
        assert_eq!(time_saved_ms(100, 60_000), 90_000);
        assert_eq!(time_saved_ms(10, 60_000), 0, "never negative");
        assert_eq!(time_saved_ms(0, 0), 0);
    }
}

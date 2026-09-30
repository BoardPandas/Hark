//! Asynchronous, generation-keyed insights queries. No SQLite or text analysis
//! runs on the UI thread; a worker reply explicitly wakes the root viewport.
use crate::storage::{StorageCmd, StorageHandle};
use hark_store::{Insights, InsightsRequest};
use jiff::{civil::Date, tz::TimeZone, Timestamp};
use std::sync::mpsc::{self, Receiver, TryRecvError};

#[derive(PartialEq, Eq)]
struct Key {
    generation: u64,
    days: u16,
    date: Date,
    analyze_text: bool,
}

pub struct InsightsCache {
    pub data: Option<Insights>,
    pub error: Option<String>,
    pub tz: TimeZone,
    key: Option<Key>,
    pending: Option<Receiver<Result<Insights, String>>>,
}

impl InsightsCache {
    pub fn new() -> Self {
        Self {
            data: None,
            error: None,
            tz: TimeZone::system(),
            key: None,
            pending: None,
        }
    }

    pub fn refresh(
        &mut self,
        ctx: &egui::Context,
        storage: &StorageHandle,
        days: u16,
        analyze_text: bool,
    ) {
        let now = Timestamp::now();
        // A visible idle window must roll its date range forward at midnight.
        // Local midnight respects DST; this schedules one wake, not a polling loop.
        ctx.request_repaint_after(until_next_day(now, &self.tz));
        let key = Key {
            generation: storage.generation(),
            days,
            date: now.to_zoned(self.tz.clone()).date(),
            analyze_text,
        };
        if self.key.as_ref() != Some(&key) {
            // Drop an obsolete receiver before requesting a different range.
            // Its reply cannot overwrite the newly selected view.
            let (reply, rx) = mpsc::channel();
            self.data = None;
            self.error = None;
            self.pending = Some(rx);
            self.key = Some(key);
            storage.send(StorageCmd::GetInsights {
                request: InsightsRequest {
                    days,
                    now_ms: now.as_millisecond(),
                    time_zone: self.tz.clone(),
                    analyze_text,
                },
                reply,
            });
        }
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(Ok(data)) => {
                    self.data = Some(data);
                    self.pending = None;
                }
                Ok(Err(error)) => {
                    self.error = Some(error);
                    self.pending = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.error = Some("The storage worker is unavailable.".into());
                    self.pending = None;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
    }

    pub fn retry(&mut self) {
        self.key = None;
    }
}

fn until_next_day(now: Timestamp, tz: &TimeZone) -> std::time::Duration {
    now.to_zoned(tz.clone())
        .date()
        .tomorrow()
        .and_then(|date| date.at(0, 0, 0, 0).to_zoned(tz.clone()))
        .map(|next| {
            std::time::Duration::from_secs_f64(
                next.timestamp()
                    .duration_since(now)
                    .as_secs_f64()
                    .max(0.001),
            )
        })
        .unwrap_or(std::time::Duration::from_secs(3600))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_day_wake_respects_short_and_long_dst_days() {
        let tz = TimeZone::get("America/New_York").unwrap();
        for (timestamp, hours) in [("2026-03-08T05:00:00Z", 23), ("2026-11-01T04:00:00Z", 25)] {
            assert_eq!(
                until_next_day(timestamp.parse().unwrap(), &tz).as_secs(),
                hours * 3600
            );
        }
    }
}

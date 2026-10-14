//! The silence watchdog, pure: has a track heard nothing for too long?
//!
//! Six Teams meetings in one week recorded only silence (the wrong mic, and a
//! loopback deaf to the call), and the only trace was an INFO line once the
//! call was over. A track that stays under the floor long enough now says so
//! while the call is still on.
//!
//! Each batch the recorder places is judged by its loudest 100 ms window,
//! never a mean (hark-audio's rule), against a floor, never exact zero:
//! per-process loopback idles at about -96 dBFS of 1-LSB noise (LL-G HIGH
//! `kb/rust/process-loopback-idle-not-digital-zero.md`) and a muted mic reads
//! about -120 dBFS. Any batch above the floor restarts the count. Time is
//! 16 kHz samples, never a clock.

use hark_audio::window::{peak_window_rms, GATE_WINDOW_MS};

const RATE: u64 = hark_meeting::SAMPLE_RATE as u64;

/// -85 dBFS as linear RMS: above idle loopback, and far below the room noise
/// any live microphone picks up.
pub(super) const FLOOR_RMS: f32 = 5.623_413e-5;

/// Me: a working mic hears the room, so 45 s under the floor is a dead one.
pub(super) const ME_LIMIT: u64 = 45 * RATE;

/// Them: two minutes, since a call can be quiet for a while.
pub(super) const THEM_LIMIT: u64 = 120 * RATE;

pub(super) struct SilenceWatch {
    limit: u64,
    /// Samples since the last batch above the floor.
    silent: u64,
}

impl SilenceWatch {
    pub fn new(limit: u64) -> SilenceWatch {
        SilenceWatch { limit, silent: 0 }
    }

    /// Feed one placed batch of 16 kHz samples. True each time the silence
    /// reaches another whole `limit`; the caller decides whether to say so
    /// again.
    pub fn observe(&mut self, samples: &[f32]) -> bool {
        if samples.is_empty() {
            return false;
        }
        if peak_window_rms(samples, RATE as u32, GATE_WINDOW_MS) >= FLOOR_RMS {
            self.silent = 0;
            return false;
        }
        let before = self.silent / self.limit;
        self.silent += samples.len() as u64;
        self.silent / self.limit > before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tone at `dbfs`, its RMS exactly that level.
    fn tone(dbfs: f32, len: usize) -> Vec<f32> {
        let peak = 10f32.powf(dbfs / 20.0) * std::f32::consts::SQRT_2;
        (0..len).map(|i| (i as f32 * 0.3).sin() * peak).collect()
    }

    /// Process loopback with nothing playing: alternating 1-LSB noise.
    fn idle_loopback(len: usize) -> Vec<f32> {
        (0..len)
            .map(|i| if i % 3 == 0 { 1.0 / 32768.0 } else { 0.0 })
            .collect()
    }

    #[test]
    fn the_floor_sits_between_idle_loopback_and_a_quiet_room() {
        let mut watch = SilenceWatch::new(1_600);
        assert!(watch.observe(&idle_loopback(1_600)), "-95 dBFS is silence");
        let mut watch = SilenceWatch::new(1_600);
        assert!(!watch.observe(&tone(-80.0, 1_600)), "-80 dBFS is a room");
        assert!(!watch.observe(&tone(-80.0, 1_600)));
    }

    #[test]
    fn fires_at_each_whole_limit_of_unbroken_silence() {
        let mut watch = SilenceWatch::new(ME_LIMIT);
        let batch = vec![0.0; 1_600];
        let fired: Vec<u64> = (1..=9_000u64)
            .filter(|_| watch.observe(&batch))
            .map(|n| n * 1_600 / RATE)
            .collect();
        assert_eq!(fired, (1..=20).map(|k| k * 45).collect::<Vec<u64>>());
    }

    #[test]
    fn one_loud_window_restarts_the_count_and_empty_batches_are_not_time() {
        let mut watch = SilenceWatch::new(THEM_LIMIT);
        assert!(!watch.observe(&vec![0.0; THEM_LIMIT as usize - 1]));
        // 50 ms of speech inside an otherwise silent 1 s batch.
        let mut batch = vec![0.0; 16_000];
        batch[8_000..8_800].copy_from_slice(&tone(-30.0, 800));
        assert!(!watch.observe(&batch));
        assert!(!watch.observe(&[]));
        assert!(!watch.observe(&vec![0.0; THEM_LIMIT as usize - 1]));
        assert!(watch.observe(&[0.0]));
    }
}

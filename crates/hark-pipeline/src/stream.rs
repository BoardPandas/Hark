//! Streaming-while-holding: feed the STT session as the user speaks.
//!
//! The batch path can only start uploading once the key is released, so the
//! whole clip sits between release and text. A live session is opened on
//! key-down instead and fed as the ring fills, leaving only the tail
//! outstanding at release.
//!
//! # This is an accelerator, never the spine
//!
//! Every failure here — no live adapter, a refused connection, a socket that
//! dies mid-hold, a reader that falls behind the ring — drops the pump and
//! lets the ordinary batch path run. That is safe precisely because the pump
//! reads the ring rather than consuming it: the worker still assembles the
//! same window afterwards, still runs the same gates, and still has every
//! sample to POST. A dictation is never lost to the optimisation, which is
//! the same fail-open rule cleanup follows.
//!
//! # What streaming cannot carry
//!
//! [`hark_audio::assemble_window`] normalizes the finished clip against its
//! own peak. That is unstreamable by construction — the peak is not known
//! until the clip exists — so streamed audio goes up at capture level. On a
//! quiet microphone the batch path would have applied a boost the live path
//! cannot, so `finish` logs the gain that was skipped, making a quiet-mic
//! accuracy gap visible rather than mysterious.

use hark_audio::resample::StreamResampler;
use hark_audio::ring::Consumer;
use hark_audio::{window, WindowParams};
use hark_stt::{LiveSession, LiveStt, SttError, Transcript};

/// One dictation's live session plus its position in the ring.
pub(crate) struct LivePump {
    session: Box<dyn LiveSession>,
    reader: Consumer,
    resampler: StreamResampler,
    /// Next device-rate sample index to send. Advances only on a successful
    /// push, so a failed send is never silently skipped over.
    next_abs: u64,
    /// Device-rate samples sent so far, for the log line.
    sent: u64,
}

impl LivePump {
    /// Open a session and position the reader at the window start.
    ///
    /// Returns `None` on any failure: the caller then simply has no pump, and
    /// the batch path runs exactly as it did before streaming existed.
    pub fn start(
        live: &dyn LiveStt,
        consumer: &Consumer,
        down_abs: u64,
        rate: u32,
        params: &WindowParams,
    ) -> Option<Self> {
        // Same function the worker will use to assemble the clip, called with
        // a zero-length hold so it yields that window's start. Deriving it
        // here instead would let the streamed audio and the assembled clip
        // disagree about where the dictation begins.
        let (start, _) = window::window_bounds(down_abs, down_abs, rate, params);
        // The pre-roll may reach before the ring's oldest sample on a press
        // taken moments after launch.
        let start = start.max(consumer.oldest_available());

        let resampler = match StreamResampler::new(rate) {
            Ok(r) => r,
            Err(e) => {
                log::warn!("live stream unavailable (resampler): {e}");
                return None;
            }
        };
        let session = match live.start_session() {
            Ok(s) => s,
            Err(e) => {
                log::warn!("live session did not open, falling back to batch: {e}");
                return None;
            }
        };
        log::debug!("live session open; streaming from sample {start}");
        Some(Self {
            session,
            reader: consumer.reader(),
            resampler,
            next_abs: start,
            sent: 0,
        })
    }

    /// Send whatever the ring holds up to `limit_abs`.
    ///
    /// Called on a timer during the hold with `u64::MAX`, and once at release
    /// with the real window end.
    pub fn pump(&mut self, limit_abs: u64) -> Result<(), SttError> {
        let available = self.reader.total_written().min(limit_abs);
        if available <= self.next_abs {
            return Ok(());
        }
        let samples = self
            .reader
            .read_range(self.next_abs, available)
            .map_err(|e| SttError::BadAudio(format!("live stream fell behind the ring: {e}")))?;
        let resampled = self
            .resampler
            .push(&samples)
            .map_err(|e| SttError::BadAudio(format!("live stream resampling failed: {e}")))?;
        self.session.push(&resampled)?;
        self.sent += available - self.next_abs;
        self.next_abs = available;
        Ok(())
    }

    /// Send the rest of the window, end the turn, and wait for the transcript.
    ///
    /// `skipped_gain` is what the assembled clip needed; anything above 1.0
    /// went to the provider on the batch path but not on this one.
    pub fn finish(mut self, end_abs: u64, skipped_gain: f32) -> Result<Transcript, SttError> {
        self.pump(end_abs)?;
        let tail = self
            .resampler
            .drain()
            .map_err(|e| SttError::BadAudio(format!("live stream tail failed: {e}")))?;
        self.session.push(&tail)?;
        if skipped_gain > 1.0 {
            log::info!(
                "live stream sent audio at capture level; the batch path would have \
                 normalized by {skipped_gain:.2}x"
            );
        }
        self.session.finish()
    }

    /// Device-rate samples sent so far. For logging and tests.
    pub fn sent(&self) -> u64 {
        self.sent
    }
}

/// How much of the window was already uploaded when the key came up.
///
/// Pure, and the number worth watching: at 100% the release-to-inject wait is
/// just the provider finalising, and at 0% streaming bought nothing and the
/// dictation paid a socket for it.
pub fn streamed_fraction(sent: u64, window_len: u64) -> f32 {
    if window_len == 0 {
        return 0.0;
    }
    (sent as f64 / window_len as f64).clamp(0.0, 1.0) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use hark_stt::Transcript;
    use std::sync::{Arc, Mutex};

    /// Records everything pushed so a test can assert on the audio that would
    /// have reached the provider.
    #[derive(Default)]
    struct FakeLive {
        pushed: Arc<Mutex<Vec<f32>>>,
        /// Fail `start_session`, standing in for a refused connection.
        refuse_start: bool,
        /// Fail the first `push`, standing in for a socket dying mid-hold.
        fail_push: bool,
    }

    struct FakeSession {
        pushed: Arc<Mutex<Vec<f32>>>,
        fail_push: bool,
    }

    impl hark_stt::LiveSession for FakeSession {
        fn push(&mut self, samples: &[f32]) -> Result<(), SttError> {
            if self.fail_push {
                return Err(SttError::BadAudio("socket died".to_string()));
            }
            self.pushed.lock().unwrap().extend_from_slice(samples);
            Ok(())
        }
        fn finish(&mut self) -> Result<Transcript, SttError> {
            Ok(Transcript {
                text: "streamed".to_string(),
                cleaned: None,
                request_ms: 1,
            })
        }
    }

    impl LiveStt for FakeLive {
        fn start_session(&self) -> Result<Box<dyn hark_stt::LiveSession>, SttError> {
            if self.refuse_start {
                return Err(SttError::BadAudio("refused".to_string()));
            }
            Ok(Box::new(FakeSession {
                pushed: self.pushed.clone(),
                fail_push: self.fail_push,
            }))
        }
    }

    /// A ring already holding `n` samples, so pre-roll has something to reach.
    fn ring_with(n: usize) -> (hark_audio::ring::Producer, Consumer) {
        let (producer, consumer) = hark_audio::ring::ring(16_000 * 8);
        producer.push(&vec![0.25f32; n]);
        (producer, consumer)
    }

    #[test]
    fn a_refused_session_yields_no_pump_rather_than_an_error() {
        // The caller treats None as "no streaming this time" and runs the
        // batch path; a hard error here would take the dictation with it.
        let (_p, consumer) = ring_with(16_000);
        let live = FakeLive {
            refuse_start: true,
            ..Default::default()
        };
        let pump = LivePump::start(&live, &consumer, 16_000, 16_000, &WindowParams::default());
        assert!(pump.is_none());
    }

    #[test]
    fn the_pump_streams_from_the_preroll_not_from_the_keypress() {
        // 300 ms of pre-roll at 16 kHz = 4800 samples before the press.
        let (producer, consumer) = ring_with(16_000);
        let live = FakeLive::default();
        let pushed = live.pushed.clone();
        let mut pump = LivePump::start(&live, &consumer, 16_000, 16_000, &WindowParams::default())
            .expect("session opens");
        producer.push(&vec![0.5f32; 16_000]);
        pump.pump(u64::MAX).expect("pump succeeds");

        // Everything from (press - preroll) to now.
        let expected = 4_800 + 16_000;
        assert_eq!(pushed.lock().unwrap().len(), expected);
        assert_eq!(pump.sent(), expected as u64);
    }

    #[test]
    fn pumping_twice_does_not_resend_the_same_audio() {
        // A duplicated region would be transcribed twice and stutter the text.
        let (producer, consumer) = ring_with(16_000);
        let live = FakeLive::default();
        let pushed = live.pushed.clone();
        let mut pump =
            LivePump::start(&live, &consumer, 16_000, 16_000, &WindowParams::default()).unwrap();
        pump.pump(u64::MAX).unwrap();
        let after_first = pushed.lock().unwrap().len();
        pump.pump(u64::MAX).unwrap();
        assert_eq!(pushed.lock().unwrap().len(), after_first);

        producer.push(&vec![0.5f32; 1_000]);
        pump.pump(u64::MAX).unwrap();
        assert_eq!(pushed.lock().unwrap().len(), after_first + 1_000);
    }

    #[test]
    fn the_limit_caps_what_is_sent_so_the_tail_is_not_overshot() {
        let (producer, consumer) = ring_with(16_000);
        let live = FakeLive::default();
        let pushed = live.pushed.clone();
        let mut pump =
            LivePump::start(&live, &consumer, 16_000, 16_000, &WindowParams::default()).unwrap();
        producer.push(&vec![0.5f32; 16_000]);
        // Stop at absolute sample 20000, well short of what the ring holds.
        pump.pump(20_000).unwrap();
        assert_eq!(pushed.lock().unwrap().len(), 20_000 - 11_200);
    }

    #[test]
    fn a_failed_push_surfaces_so_the_caller_can_fall_back() {
        let (_p, consumer) = ring_with(16_000);
        let live = FakeLive {
            fail_push: true,
            ..Default::default()
        };
        let mut pump =
            LivePump::start(&live, &consumer, 16_000, 16_000, &WindowParams::default()).unwrap();
        assert!(pump.pump(u64::MAX).is_err());
    }

    #[test]
    fn a_failed_push_does_not_advance_the_read_position() {
        // If the position advanced on failure, a recovering stream would have
        // a silent hole where the failed chunk was.
        let (_p, consumer) = ring_with(16_000);
        let live = FakeLive {
            fail_push: true,
            ..Default::default()
        };
        let mut pump =
            LivePump::start(&live, &consumer, 16_000, 16_000, &WindowParams::default()).unwrap();
        let _ = pump.pump(u64::MAX);
        assert_eq!(pump.sent(), 0);
    }

    #[test]
    fn a_48k_device_is_resampled_on_the_way_out() {
        // The provider is promised 16 kHz; a device-rate stream would be
        // transcribed as chipmunk audio.
        let (producer, consumer) = hark_audio::ring::ring(48_000 * 8);
        producer.push(&vec![0.25f32; 48_000]);
        let live = FakeLive::default();
        let pushed = live.pushed.clone();
        let mut pump =
            LivePump::start(&live, &consumer, 48_000, 48_000, &WindowParams::default()).unwrap();
        producer.push(&vec![0.5f32; 48_000]);
        pump.pump(u64::MAX).unwrap();
        // ~1 s of new audio plus 300 ms pre-roll, at a third of the rate.
        let got = pushed.lock().unwrap().len();
        let expected = (48_000 + 14_400) / 3;
        assert!(
            got.abs_diff(expected) < 2_048,
            "expected about {expected} samples at 16 kHz, got {got}"
        );
    }

    #[test]
    fn an_empty_window_is_not_a_division_by_zero() {
        assert_eq!(streamed_fraction(0, 0), 0.0);
    }

    #[test]
    fn nothing_streamed_reads_as_zero() {
        assert_eq!(streamed_fraction(0, 48_000), 0.0);
    }

    #[test]
    fn a_fully_streamed_hold_reads_as_one() {
        assert_eq!(streamed_fraction(48_000, 48_000), 1.0);
    }

    #[test]
    fn overshoot_is_clamped_rather_than_reported_above_one() {
        // The tail can push `sent` past the window the fraction is measured
        // against; a 130% progress bar would be a bug report.
        assert_eq!(streamed_fraction(60_000, 48_000), 1.0);
    }

    #[test]
    fn a_partial_hold_reports_its_share() {
        assert!((streamed_fraction(24_000, 48_000) - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn the_pump_starts_where_the_assembled_window_starts() {
        // If these drifted, the streamed audio would begin at a different
        // instant from the clip the gates and the fallback use.
        let params = WindowParams::default();
        let rate = 48_000;
        let down_abs = 10 * rate as u64;
        let (stream_start, _) = window::window_bounds(down_abs, down_abs, rate, &params);
        let (clip_start, _) =
            window::window_bounds(down_abs, down_abs + 2 * rate as u64, rate, &params);
        assert_eq!(stream_start, clip_start);
    }
}

//! The endpoint-loopback timeline, pure. WASAPI endpoint loopback sends no
//! packets while nothing plays (LL-G HIGH `kb/rust/wasapi-loopback-silence-gaps.md`),
//! so appending packets as they come would squeeze every silence out of the
//! Them track and leave it ahead of Me. Each packet carries the QPC time of
//! its first frame; this places it at that time and fills what came before
//! with silence. Process loopback is continuous and does not use this.
//!
//! Times are QPC ticks in 100 ns units (WASAPI's `u64QPCPosition`); positions
//! are frames at the stream rate since `origin`, the moment the stream
//! started.

/// QPC positions count 100 ns ticks.
pub(crate) const TICKS_PER_SECOND: u64 = 10_000_000;

/// Packet timestamps jitter by about one device period (~10 ms): a gap
/// shorter than this is jitter, not silence.
const TOLERANCE_MS: u64 = 20;

/// While no packet comes, silence is placed only up to this far behind the
/// clock. It exceeds the 200 ms capture buffer, so a packet waiting there
/// can never land on silence already placed.
const IDLE_MARGIN_MS: u64 = 300;

pub(crate) struct GapFiller {
    origin: u64,
    rate: u64,
    /// Frames on the timeline so far, silence included.
    placed: u64,
}

impl GapFiller {
    pub fn new(origin_ticks: u64, rate: u32) -> GapFiller {
        GapFiller {
            origin: origin_ticks,
            rate: u64::from(rate),
            placed: 0,
        }
    }

    /// A packet of `frames` whose first frame was captured at `ticks`: the
    /// silent frames to push before it. A packet that arrives early (jitter,
    /// or a clock running slightly fast) is appended, never trimmed: losing
    /// audio is worse than a few milliseconds of drift.
    pub fn before_packet(&mut self, ticks: u64, frames: u64) -> u64 {
        let due = self.frame_at(ticks);
        let gap = due.saturating_sub(self.placed);
        let pad = if gap > self.rate * TOLERANCE_MS / 1000 {
            gap
        } else {
            0
        };
        self.placed += pad + frames;
        pad
    }

    /// A packet whose timestamp is flagged unreliable: it follows on.
    pub fn contiguous(&mut self, frames: u64) {
        self.placed += frames;
    }

    /// No packet is waiting at `now_ticks`: the silent frames that bring the
    /// timeline up to the clock, less the margin.
    pub fn idle(&mut self, now_ticks: u64) -> u64 {
        let margin = IDLE_MARGIN_MS * TICKS_PER_SECOND / 1000;
        let due = self.frame_at(now_ticks.saturating_sub(margin));
        let pad = due.saturating_sub(self.placed);
        self.placed += pad;
        pad
    }

    fn frame_at(&self, ticks: u64) -> u64 {
        let elapsed = u128::from(ticks.saturating_sub(self.origin));
        (elapsed * u128::from(self.rate) / u128::from(TICKS_PER_SECOND)) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 16_000;
    /// 10 ms in ticks, and in frames at 16 kHz.
    const PERIOD: u64 = TICKS_PER_SECOND / 100;
    const FRAMES: u64 = 160;
    const ORIGIN: u64 = 7_000_000_000;

    #[test]
    fn continuous_packets_are_never_padded() {
        let mut gaps = GapFiller::new(ORIGIN, RATE);
        for n in 0..1_000 {
            assert_eq!(gaps.before_packet(ORIGIN + n * PERIOD, FRAMES), 0);
        }
        assert_eq!(gaps.placed, 160_000);
    }

    #[test]
    fn untimestamped_packets_still_move_the_timeline_on() {
        let mut gaps = GapFiller::new(ORIGIN, RATE);
        assert_eq!(gaps.before_packet(ORIGIN, FRAMES), 0);
        // 30 ms of packets with unreliable timestamps: past the tolerance,
        // so forgetting them would pad the next stamped packet with silence.
        for _ in 0..3 {
            gaps.contiguous(FRAMES);
        }
        assert_eq!(gaps.before_packet(ORIGIN + 4 * PERIOD, FRAMES), 0);
        assert_eq!(gaps.placed, 5 * FRAMES);
    }

    #[test]
    fn a_silence_between_packets_becomes_exactly_that_many_frames() {
        let mut gaps = GapFiller::new(ORIGIN, RATE);
        assert_eq!(gaps.before_packet(ORIGIN, FRAMES), 0);
        // Nothing played for 2.5 s after the first packet ended.
        let resumed = ORIGIN + PERIOD + 25 * TICKS_PER_SECOND / 10;
        assert_eq!(gaps.before_packet(resumed, FRAMES), 40_000);
        assert_eq!(gaps.placed, 160 + 40_000 + 160);
        assert_eq!(gaps.before_packet(resumed + PERIOD, FRAMES), 0);
    }

    #[test]
    fn a_stream_that_starts_in_silence_is_padded_from_its_start() {
        let mut gaps = GapFiller::new(ORIGIN, RATE);
        assert_eq!(
            gaps.before_packet(ORIGIN + 3 * TICKS_PER_SECOND, FRAMES),
            48_000
        );
    }

    #[test]
    fn jitter_under_the_tolerance_is_not_silence() {
        let mut gaps = GapFiller::new(ORIGIN, RATE);
        gaps.before_packet(ORIGIN, FRAMES);
        // 19 ms late: 304 frames ahead of the timeline, under 320.
        assert_eq!(gaps.before_packet(ORIGIN + PERIOD + 190_000, FRAMES), 0);
        // Early, and stamped before the stream started: appended as is.
        assert_eq!(gaps.before_packet(ORIGIN + PERIOD, FRAMES), 0);
        assert_eq!(
            GapFiller::new(ORIGIN, RATE).before_packet(ORIGIN - 5, FRAMES),
            0
        );
        assert_eq!(gaps.placed, 480);
    }

    #[test]
    fn idle_fills_up_to_the_clock_less_the_margin_without_double_counting() {
        let mut gaps = GapFiller::new(ORIGIN, RATE);
        // 1.3 s with no packet: silence up to 1.0 s.
        assert_eq!(gaps.idle(ORIGIN + 13 * TICKS_PER_SECOND / 10), 16_000);
        assert_eq!(gaps.idle(ORIGIN + 13 * TICKS_PER_SECOND / 10), 0);
        // Inside the margin, so it was not yet placed: 200 ms more silence.
        assert_eq!(
            gaps.before_packet(ORIGIN + 12 * TICKS_PER_SECOND / 10, FRAMES),
            3_200
        );
        assert_eq!(gaps.placed, 19_200 + 160);
        // The clock barely moved past that packet: nothing to fill.
        assert_eq!(gaps.idle(ORIGIN + 15 * TICKS_PER_SECOND / 10), 0);
    }

    #[test]
    fn hours_of_ticks_do_not_overflow() {
        let mut gaps = GapFiller::new(0, 48_000);
        let day = 24 * 3_600 * TICKS_PER_SECOND;
        assert_eq!(gaps.idle(day), 24 * 3_600 * 48_000 - 14_400);
    }
}

//! Timestamped PipeWire packets share one bounded 16 kHz mono timeline.
//! All calls run on the capture loop thread, never on a cpal callback.

use crate::ring::Producer;
use std::collections::{HashMap, VecDeque};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

const RATE: u64 = super::LOOPBACK_RATE as u64;
const BATCH: usize = 480;
/// A silent/disappearing source may delay the others by at most 250 ms.
const MAX_PENDING: usize = 4_000;

/// Header PTS is preferred. Older graphs without it still use the graph clock
/// for their origin/gaps and a per-stream sample cursor between reports; several
/// buffers dequeued in one callback must not all land on the same graph cycle.
#[derive(Default)]
pub(super) struct PacketClock {
    next_ns: Option<u64>,
}

impl PacketClock {
    pub fn start(&mut self, pts: Option<u64>, cycle_ns: u64, frames: usize) -> u64 {
        // Some clients supply stream-relative PTS. Only use a header on the
        // graph's monotonic clock; otherwise its unrelated epoch could shift
        // the recording by years (or discard every packet as ancient).
        let pts = pts.filter(|pts| pts.abs_diff(cycle_ns) <= 2_000_000_000);
        let start = pts.unwrap_or_else(|| match self.next_ns {
            Some(next) if cycle_ns <= next.saturating_add(125_000_000) => next,
            _ => cycle_ns,
        });
        self.next_ns = Some(start.saturating_add(frames as u64 * 1_000_000_000 / RATE));
        start
    }
}

pub(super) struct Mixer {
    producer: Producer,
    origin_ns: u64,
    emitted: u64,
    end: u64,
    pending: VecDeque<f32>,
    sources: HashMap<String, u64>,
    captured_frames: Arc<AtomicU64>,
}

impl Mixer {
    pub fn new(origin_ns: u64, producer: Producer, captured_frames: Arc<AtomicU64>) -> Self {
        Self {
            producer,
            origin_ns,
            emitted: 0,
            end: 0,
            pending: VecDeque::with_capacity(MAX_PENDING),
            sources: HashMap::new(),
            captured_frames,
        }
    }

    pub fn add_source(&mut self, source: &str) {
        self.sources
            .entry(source.to_owned())
            .or_insert(self.emitted);
    }

    pub fn remove_source(&mut self, source: &str) {
        self.sources.remove(source);
        self.flush_ready();
    }

    /// Return how many samples arrived after their output interval was already
    /// emitted. The caller counts this as a discontinuity, never shifts it later.
    pub fn push(&mut self, source: &str, start_ns: u64, samples: &[f32]) -> u64 {
        // A buffered packet can begin before capture was requested. Crop its
        // prefix rather than shifting those samples to recording time zero.
        let before_origin = frames_between(start_ns, self.origin_ns) as usize;
        let samples = &samples[before_origin.min(samples.len())..];
        let start = self.position(start_ns);
        let mut late = 0;
        for (i, samples) in samples.chunks(BATCH).enumerate() {
            let start = start + (i * BATCH) as u64;
            let end = start + samples.len() as u64;
            let Some(previous_end) = self.sources.get(source).copied() else {
                return late;
            };
            // Keep at most MAX_PENDING samples even when a source stops
            // delivering. Missing intervals are silence, not a shorter timeline.
            self.emit_until(end.saturating_sub(MAX_PENDING as u64));
            let keep_from = start.max(self.emitted).max(previous_end).min(end);
            late += self.emitted.saturating_sub(start).min(samples.len() as u64);
            if keep_from < end {
                self.pending
                    .resize(self.pending.len().max((end - self.emitted) as usize), 0.0);
                for (i, sample) in samples
                    .iter()
                    .enumerate()
                    .skip((keep_from - start) as usize)
                {
                    if sample.is_finite() {
                        self.pending[(start + i as u64 - self.emitted) as usize] += sample;
                    }
                }
            }
            *self.sources.get_mut(source).expect("source checked above") = previous_end.max(end);
            self.end = self.end.max(end);
            self.captured_frames.store(self.end, Ordering::Release);
            self.flush_ready();
        }
        late
    }

    pub fn flush_due(&mut self, now_ns: u64) {
        let due = self.position(now_ns).saturating_sub(MAX_PENDING as u64);
        self.emit_until(due.min(self.end));
    }

    pub fn finish(&mut self) {
        self.emit_until(self.end);
    }

    fn position(&self, ns: u64) -> u64 {
        frames_between(self.origin_ns, ns)
    }

    fn flush_ready(&mut self) {
        let ready = self.sources.values().copied().min().unwrap_or(self.end);
        self.emit_until(ready.min(self.end));
    }

    fn emit_until(&mut self, end: u64) {
        let mut output = [0.0; BATCH];
        while self.emitted < end {
            let count = (end - self.emitted).min(BATCH as u64) as usize;
            for sample in &mut output[..count] {
                *sample = self.pending.pop_front().unwrap_or(0.0).clamp(-1.0, 1.0);
            }
            self.producer.push(&output[..count]);
            self.emitted += count as u64;
        }
    }
}

fn frames_between(start_ns: u64, end_ns: u64) -> u64 {
    // At 16 kHz one sample is exactly 62,500 ns, so packet chunking cannot
    // accumulate rounding drift. Round the common-clock position once.
    ((u128::from(end_ns.saturating_sub(start_ns)) * u128::from(RATE) + 500_000_000) / 1_000_000_000)
        as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ring::ring;

    #[test]
    fn packet_clock_uses_pts_and_does_not_overlap_queued_buffers() {
        let mut clock = PacketClock::default();
        assert_eq!(clock.start(None, 1_000_000_000, 160), 1_000_000_000);
        assert_eq!(clock.start(None, 1_000_000_000, 160), 1_010_000_000);
        assert_eq!(
            clock.start(Some(1_030_000_000), 1_050_000_000, 160),
            1_030_000_000
        );
        assert_eq!(clock.start(None, 2_000_000_000, 160), 2_000_000_000);
        assert_eq!(clock.start(Some(123), 90_000_000_000, 160), 90_000_000_000);
    }

    #[test]
    fn simultaneous_streams_mix_without_multiplying_duration() {
        let (producer, consumer) = ring(32_000);
        let mut mixer = Mixer::new(1_000_000_000, producer, Arc::default());
        mixer.add_source("call");
        mixer.add_source("tab");
        for cycle in 0..100 {
            let time = 1_000_000_000 + cycle * 10_000_000;
            mixer.push("call", time, &[0.25; 160]);
            mixer.push("tab", time, &[0.5; 160]);
        }
        mixer.finish();
        assert_eq!(consumer.total_written(), 16_000);
        assert_eq!(consumer.read_range(0, 16_000).unwrap(), vec![0.75; 16_000]);
    }

    #[test]
    fn timestamps_align_sources_despite_callback_order_and_source_changes() {
        let (producer, consumer) = ring(32_000);
        let mut mixer = Mixer::new(1_000_000_000, producer, Arc::default());
        mixer.add_source("first");
        mixer.add_source("second");
        mixer.push("second", 1_010_000_000, &[0.5; 160]);
        mixer.push("first", 1_000_000_000, &[0.25; 320]);
        mixer.remove_source("second");
        mixer.add_source("new");
        mixer.push("new", 1_020_000_000, &[0.1; 160]);
        mixer.push("first", 1_020_000_000, &[0.25; 160]);
        mixer.finish();
        let output = consumer.read_range(0, 480).unwrap();
        assert_eq!(&output[..160], &[0.25; 160]);
        assert_eq!(&output[160..320], &[0.75; 160]);
        assert_eq!(&output[320..], &[0.35; 160]);
    }

    #[test]
    fn stalled_source_is_bounded_and_late_packets_do_not_extend_or_replay_audio() {
        let (producer, consumer) = ring(32_000);
        let captured_frames = Arc::new(AtomicU64::new(0));
        let mut mixer = Mixer::new(0, producer, captured_frames.clone());
        mixer.add_source("moving");
        mixer.add_source("stalled");
        mixer.push("moving", 0, &[0.25; 16_000]);
        assert!(mixer.pending.len() <= MAX_PENDING);
        assert_eq!(consumer.total_written(), 12_000);
        assert_eq!(captured_frames.load(Ordering::Acquire), 16_000);
        assert!(mixer.push("stalled", 0, &[0.5; 160]) > 0);
        mixer.flush_due(1_250_000_000);
        assert_eq!(consumer.total_written(), 16_000);
        mixer.finish();
        assert_eq!(consumer.total_written(), 16_000);
        assert_eq!(consumer.read_range(0, 16_000).unwrap(), vec![0.25; 16_000]);
    }

    #[test]
    fn finish_keeps_a_pending_ending_and_gaps_keep_their_duration() {
        let (producer, consumer) = ring(32_000);
        let mut mixer = Mixer::new(0, producer, Arc::default());
        mixer.add_source("moving");
        mixer.add_source("stalled");
        mixer.push("moving", 10_000_000, &[0.75; 137]);
        assert_eq!(consumer.total_written(), 0);
        mixer.finish();
        assert_eq!(consumer.total_written(), 297);
        let output = consumer.read_range(0, 297).unwrap();
        assert_eq!(&output[..160], &[0.0; 160]);
        assert_eq!(&output[160..], &[0.75; 137]);
    }

    #[test]
    fn packets_before_the_recording_origin_are_cropped_not_shifted() {
        let (producer, consumer) = ring(320);
        let mut mixer = Mixer::new(1_000_000_000, producer, Arc::default());
        mixer.add_source("source");
        mixer.push(
            "source",
            990_000_000,
            &[&[0.25; 160][..], &[0.5; 160]].concat(),
        );
        mixer.finish();
        assert_eq!(consumer.total_written(), 160);
        assert_eq!(consumer.read_range(0, 160).unwrap(), vec![0.5; 160]);
    }
}

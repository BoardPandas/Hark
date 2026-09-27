//! Cuts one channel's continuous 16 kHz audio into live-transcription chunks.
//!
//! A chunk ends at the middle of the quietest [`ChunkParams::quiet_window_ms`]
//! window whose cut point lies between [`ChunkParams::min_ms`] and
//! [`ChunkParams::max_ms`], so words are not split where a pause exists, and no
//! chunk is ever longer than the hard cut. Regular chunks are therefore at
//! least `min_ms` long (Groq bills 10 s minimum per request, so shorter ones
//! would be paid for anyway). Only the tail flushed at stop can be shorter.
//!
//! A chunk with nothing in it (the loopback while you talk, the mic while they
//! do) is dropped, judged by the same loudest-window gate as a dictation clip:
//! a whole-chunk mean would call a single short answer in 30 s of listening
//! "silent" (LL-G `mean-rms-gate-length-dependent`). Dropped chunks still
//! advance the timeline, so every kept chunk carries its true start offset.

use crate::SAMPLE_RATE;
use hark_audio::window::{gate_clip, ms_to_samples, GateVerdict, WindowParams};

/// The quiet-point search steps in blocks this long: 10 ms is well below a
/// syllable, and a 10 s search range stays ~1000 blocks.
const SCAN_BLOCK_MS: u32 = 10;

#[derive(Debug, Clone, Copy)]
pub struct ChunkParams {
    /// The earliest cut: the shortest regular chunk.
    pub min_ms: u32,
    /// The hard cut: no chunk is longer.
    pub max_ms: u32,
    /// The cut lands in the middle of the quietest window of this length.
    pub quiet_window_ms: u32,
    /// `[audio] silence_rms`, the gate's absolute threshold. The gate also
    /// admits a chunk whose loudest window stands well above its own floor.
    pub silence_rms: f32,
}

impl Default for ChunkParams {
    fn default() -> Self {
        ChunkParams {
            min_ms: 20_000,
            max_ms: 30_000,
            quiet_window_ms: 300,
            silence_rms: WindowParams::default().silence_rms,
        }
    }
}

/// A span of one channel to transcribe.
pub struct Chunk {
    /// The first sample's offset on the session timeline.
    pub start: u64,
    /// 16 kHz mono.
    pub samples: Vec<f32>,
}

impl Chunk {
    /// One past the last sample's offset on the session timeline.
    pub fn end(&self) -> u64 {
        self.start + self.samples.len() as u64
    }
}

// No derived Debug: a reflexive `{chunk:?}` must not dump raw audio.
impl std::fmt::Debug for Chunk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Chunk")
            .field("start", &self.start)
            .field("len", &self.samples.len())
            .finish()
    }
}

/// One channel's chunker. Feed it every sample in order with [`push`](Self::push);
/// call [`finish`](Self::finish) at stop for the tail.
pub struct Chunker {
    params: ChunkParams,
    /// Samples not yet cut, starting at `buf_start` on the timeline.
    buf: Vec<f32>,
    buf_start: u64,
    skipped: u64,
    // Sample counts derived from `params` once.
    min: usize,
    max: usize,
    block: usize,
    window_blocks: usize,
}

impl Chunker {
    /// A chunker whose first pushed sample is session sample 0. The caller
    /// aligns the stream to the session start (the drain pads a late-opening
    /// channel with silence), so offsets from both channels are comparable.
    ///
    /// Panics on parameters that leave no room to search for a cut: they are
    /// constants today, and a bad set is a programming error.
    pub fn new(params: ChunkParams) -> Self {
        let to_samples = |ms| ms_to_samples(ms, SAMPLE_RATE) as usize;
        let block = to_samples(SCAN_BLOCK_MS);
        let window_blocks = (params.quiet_window_ms / SCAN_BLOCK_MS).max(1) as usize;
        let (min, max) = (to_samples(params.min_ms), to_samples(params.max_ms));
        let window = window_blocks * block;
        assert!(
            min >= window && max >= min + window,
            "chunk params leave no quiet-point search range: {params:?}"
        );
        Chunker {
            params,
            buf: Vec::new(),
            buf_start: 0,
            skipped: 0,
            min,
            max,
            block,
            window_blocks,
        }
    }

    /// Append samples; returns every chunk that became ready, in order. Most
    /// pushes return none: a cut waits until a full `max_ms` is buffered, so
    /// the whole search range is known.
    pub fn push(&mut self, samples: &[f32]) -> Vec<Chunk> {
        self.buf.extend_from_slice(samples);
        let mut ready = Vec::new();
        while self.buf.len() >= self.max {
            let cut = self.find_cut();
            if let Some(chunk) = self.cut(cut) {
                ready.push(chunk);
            }
        }
        ready
    }

    /// Flush whatever is buffered as the final chunk (any length), unless it
    /// has nothing in it. The chunker is empty afterwards.
    pub fn finish(&mut self) -> Option<Chunk> {
        if self.buf.is_empty() {
            return None;
        }
        self.cut(self.buf.len())
    }

    /// Chunks dropped as empty so far, for the session log and cost estimate.
    pub fn skipped(&self) -> u64 {
        self.skipped
    }

    /// Split off `buf[..cut]` and return it if it passes the gate.
    fn cut(&mut self, cut: usize) -> Option<Chunk> {
        let rest = self.buf.split_off(cut);
        let samples = std::mem::replace(&mut self.buf, rest);
        let start = self.buf_start;
        self.buf_start += cut as u64;
        if self.has_content(&samples) {
            Some(Chunk { start, samples })
        } else {
            self.skipped += 1;
            None
        }
    }

    /// The cut point in `buf`: the middle of the quietest window whose middle
    /// lies in `[min, max)` and which ends by `max`. Ties go to the latest
    /// window, so a stretch of silence yields the longest chunk and the fewest
    /// requests. Requires `buf.len() >= max`.
    fn find_cut(&self) -> usize {
        let half = self.window_blocks * self.block / 2;
        let lo = self.min - half;
        let blocks: Vec<f64> = self.buf[lo..self.max]
            .chunks_exact(self.block)
            .map(|b| b.iter().map(|&s| s as f64 * s as f64).sum())
            .collect();
        let mut best = (f64::INFINITY, 0);
        for (j, window) in blocks.windows(self.window_blocks).enumerate() {
            // Summed in the same order every time, so equal blocks give
            // exactly equal windows and the tie rule is deterministic.
            let energy: f64 = window.iter().sum();
            if energy <= best.0 {
                best = (energy, j);
            }
        }
        lo + best.1 * self.block + half
    }

    fn has_content(&self, samples: &[f32]) -> bool {
        let gate = WindowParams {
            silence_rms: self.params.silence_rms,
            ..WindowParams::default()
        };
        gate_clip(samples, SAMPLE_RATE, &gate) == GateVerdict::Speech
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Samples in `s` seconds (or fractions) at 16 kHz.
    fn secs(s: f64) -> usize {
        (s * SAMPLE_RATE as f64).round() as usize
    }

    /// A constant (DC) signal: every block has exactly equal energy.
    fn level(len: usize, amp: f32) -> Vec<f32> {
        vec![amp; len]
    }

    fn sine(len: usize, amp: f32) -> Vec<f32> {
        (0..len)
            .map(|i| {
                (2.0 * std::f32::consts::PI * 220.0 * i as f32 / SAMPLE_RATE as f32).sin() * amp
            })
            .collect()
    }

    /// Replace `[at, at + len)` of `signal` with `with`.
    fn splice(signal: &mut [f32], at: usize, with: &[f32]) {
        signal[at..at + with.len()].copy_from_slice(with);
    }

    fn spans(chunks: &[Chunk]) -> Vec<(u64, usize)> {
        chunks.iter().map(|c| (c.start, c.samples.len())).collect()
    }

    fn run(signal: &[f32], piece: usize) -> (Vec<Chunk>, u64) {
        let mut chunker = Chunker::new(ChunkParams::default());
        let mut out = Vec::new();
        for p in signal.chunks(piece) {
            out.extend(chunker.push(p));
        }
        out.extend(chunker.finish());
        (out, chunker.skipped())
    }

    #[test]
    fn default_search_range_is_twenty_to_thirty_seconds() {
        let c = Chunker::new(ChunkParams::default());
        assert_eq!((c.min, c.max), (320_000, 480_000));
        assert_eq!((c.block, c.window_blocks), (160, 30));
    }

    #[test]
    fn cuts_in_the_middle_of_the_quietest_window() {
        let mut signal = sine(secs(35.0), 0.3);
        // A quiet-but-not-silent stretch at 22.0 s, and true silence at 24.0 s.
        splice(&mut signal, secs(22.0), &sine(secs(0.3), 0.01));
        splice(&mut signal, secs(24.0), &level(secs(0.3), 0.0));
        let mut chunker = Chunker::new(ChunkParams::default());
        let ready = chunker.push(&signal);
        // The silent window [24.0, 24.3) wins; the cut is its middle.
        assert_eq!(spans(&ready), vec![(0, secs(24.15))]);
    }

    #[test]
    fn a_pause_before_the_minimum_is_not_a_cut_point() {
        let mut signal = level(secs(31.0), 0.3);
        splice(&mut signal, secs(10.0), &level(secs(0.3), 0.0));
        let mut chunker = Chunker::new(ChunkParams::default());
        let ready = chunker.push(&signal);
        // Every window in range ties, so the latest one wins: 30 s minus half
        // a window. The 10 s pause is outside the range and ignored.
        assert_eq!(spans(&ready), vec![(0, secs(29.85))]);
    }

    #[test]
    fn continuous_speech_is_hard_cut_and_the_tail_is_flushed() {
        let (chunks, skipped) = run(&level(secs(70.0), 0.3), secs(70.0));
        assert_eq!(
            spans(&chunks),
            vec![
                (0, 477_600),
                (477_600, 477_600),
                (955_200, secs(70.0) - 955_200),
            ]
        );
        assert_eq!(skipped, 0);
    }

    #[test]
    fn silent_chunks_are_dropped_but_keep_the_timeline() {
        // 40 s of nothing, then 30 s of speech.
        let mut signal = level(secs(40.0), 0.0);
        signal.extend(level(secs(30.0), 0.3));
        let (chunks, skipped) = run(&signal, 1600);
        assert_eq!(skipped, 1, "the first 29.85 s held nothing");
        assert_eq!(
            spans(&chunks),
            vec![(477_600, 477_600), (955_200, secs(70.0) - 955_200)],
            "kept chunks carry their true offsets"
        );
    }

    #[test]
    fn small_pushes_cut_exactly_like_one_big_push() {
        let mut signal = sine(secs(95.0), 0.2);
        for (at, len) in [(21.3, 0.4), (47.9, 0.3), (58.0, 1.0), (80.2, 0.35)] {
            splice(&mut signal, secs(at), &level(secs(len), 0.0));
        }
        let (whole, _) = run(&signal, signal.len());
        assert_eq!(whole.len(), 4, "95 s cuts into three chunks plus a tail");
        for piece in [160, 1234, 16_000] {
            let (pieces, _) = run(&signal, piece);
            assert_eq!(spans(&pieces), spans(&whole), "piece size {piece}");
        }
    }

    #[test]
    fn chunks_tile_the_timeline_within_the_length_bounds() {
        // Five minutes of loud noise with no pause anywhere.
        let mut seed = 0x2545_f491_u32;
        let noise: Vec<f32> = (0..secs(300.0))
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                (seed as f32 / u32::MAX as f32 - 0.5) * 0.6
            })
            .collect();
        let (chunks, skipped) = run(&noise, 3200);
        assert_eq!(skipped, 0);
        assert_eq!(chunks[0].start, 0);
        for pair in chunks.windows(2) {
            assert_eq!(pair[1].start, pair[0].end(), "gap or overlap");
        }
        assert_eq!(chunks.last().map(Chunk::end), Some(secs(300.0) as u64));
        let (last, regular) = chunks.split_last().expect("chunks");
        for c in regular {
            let len = c.samples.len();
            assert!((320_000..=480_000).contains(&len), "regular chunk of {len}");
        }
        assert!(last.samples.len() <= 480_000);
    }

    #[test]
    fn finish_flushes_a_short_tail_once() {
        let mut chunker = Chunker::new(ChunkParams::default());
        assert!(chunker.push(&level(secs(5.0), 0.3)).is_empty());
        let tail = chunker.finish().expect("tail with speech");
        assert_eq!((tail.start, tail.samples.len()), (0, secs(5.0)));
        assert!(chunker.finish().is_none(), "nothing left after a flush");
    }

    #[test]
    fn finish_drops_a_silent_tail() {
        let mut chunker = Chunker::new(ChunkParams::default());
        chunker.push(&level(secs(5.0), 0.0));
        assert!(chunker.finish().is_none());
        assert_eq!(chunker.skipped(), 1);
    }

    #[test]
    fn one_short_answer_in_a_long_silence_is_kept() {
        // 400 ms "yes" in 30 s of digital silence: a whole-chunk mean would
        // be ~0.04x the speech level and call it empty.
        let mut signal = level(secs(30.0), 0.0);
        splice(&mut signal, secs(5.0), &sine(secs(0.4), 0.05));
        let (chunks, skipped) = run(&signal, signal.len());
        assert_eq!((chunks.len(), skipped), (1, 1));
        assert_eq!(chunks[0].start, 0, "the answer is in the first chunk");
    }

    #[test]
    fn quiet_speech_over_a_quiet_room_is_kept_and_steady_hiss_is_not() {
        // Hiss at -54 dBFS: nothing ever rises above the room.
        let hiss: Vec<f32> = (0..secs(10.0))
            .map(|i| if i % 2 == 0 { 0.002 } else { -0.002 })
            .collect();
        let (chunks, skipped) = run(&hiss, hiss.len());
        assert_eq!((chunks.len(), skipped), (0, 1));

        // The same hiss with speech below the absolute threshold (RMS 0.0095
        // vs 0.01) but well above the room: the gate's relative path keeps it.
        let mut talk = hiss.clone();
        splice(&mut talk, secs(3.0), &sine(secs(1.0), 0.0135));
        let (chunks, _) = run(&talk, talk.len());
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    #[should_panic(expected = "no quiet-point search range")]
    fn a_search_range_narrower_than_the_window_is_rejected() {
        Chunker::new(ChunkParams {
            min_ms: 20_000,
            max_ms: 20_100,
            ..ChunkParams::default()
        });
    }
}

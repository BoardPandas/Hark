//! Device-rate -> 16 kHz mono resampling.
//!
//! WASAPI shared mode does not resample for us and rarely offers 16 kHz:
//! devices commonly run at 48 kHz (exact 3:1) or 44.1 kHz (non-integer
//! ratio), so a general resampler is the safe default. rubato 4.0's
//! `process_all` is the whole-clip path: it trims the FFT startup delay and
//! returns exact output frame counts, unlike a single oversized `process()`
//! call which would leave leading silence and truncate the tail.

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::audioadapter_buffers::owned::InterleavedOwned;
use rubato::{Fft, FixedSync, Resampler};
use thiserror::Error;

/// The pipeline-wide capture contract: everything downstream of this module
/// (WAV encode, STT upload) speaks 16 kHz mono.
pub const TARGET_RATE: u32 = 16_000;

#[derive(Debug, Error)]
pub enum ResampleError {
    #[error("cannot construct resampler for {src_rate} Hz -> 16000 Hz: {detail}")]
    Construct { src_rate: u32, detail: String },
    #[error("resampling failed: {0}")]
    Process(String),
}

/// Downmix interleaved multi-channel samples to mono by averaging each frame.
/// A trailing partial frame is dropped. (The live capture path downmixes
/// allocation-free inside the ring producer; this is the offline/test path.)
pub fn downmix_interleaved(interleaved: &[f32], channels: usize) -> Vec<f32> {
    assert!(channels > 0, "channel count must be non-zero");
    if channels == 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

/// Resample a complete mono clip from `src_rate` to 16 kHz. Already-16 kHz
/// input is passed through untouched.
pub fn resample_to_16k(samples: &[f32], src_rate: u32) -> Result<Vec<f32>, ResampleError> {
    if src_rate == TARGET_RATE {
        return Ok(samples.to_vec());
    }
    if samples.is_empty() {
        return Ok(Vec::new());
    }
    // Chunk size trades startup cost vs. per-chunk efficiency; 1024 is the
    // recommended starting point. FixedSync::Both locks input and output to
    // the exact rational ratio (synchronous resampling, no drift adjustment).
    let mut resampler = Fft::<f32>::new(
        src_rate as usize,
        TARGET_RATE as usize,
        1024,
        1,
        FixedSync::Both,
    )
    .map_err(|e| ResampleError::Construct {
        src_rate,
        detail: e.to_string(),
    })?;
    // Mono: an interleaved view over the plain slice is the 1-channel adapter.
    let input = InterleavedSlice::new(samples, 1, samples.len())
        .map_err(|e| ResampleError::Process(e.to_string()))?;
    let output = resampler
        .process_all(&input, samples.len(), None)
        .map_err(|e| ResampleError::Process(e.to_string()))?;
    // InterleavedOwned with 1 channel: the raw data vec IS the mono clip.
    Ok(output.take_data())
}

/// A resampler that keeps its state across calls, for feeding a live STT
/// session while the user is still holding the key.
///
/// [`resample_to_16k`] cannot be used chunk-by-chunk: it builds a fresh
/// resampler per call and runs `process_all`, so every chunk would pay the FFT
/// startup delay again and the seams would land in the audio as periodic
/// glitches — quiet enough to miss by ear, loud enough to cost transcription
/// accuracy (LL-G Rust HIGH, `rubato-process-all-whole-clip`). This type holds
/// one resampler for the whole utterance instead, feeds it exactly the block
/// size it asks for, and trims the startup delay once rather than per chunk.
///
/// `push` returns only the samples that are ready; the remainder stays buffered
/// until enough input arrives to fill another block. `drain` flushes the tail
/// by zero-padding to one final block, and must be called exactly once, at the
/// end of the utterance.
pub struct StreamResampler {
    /// `None` when the device already runs at 16 kHz: passthrough, no work.
    inner: Option<Fft<f32>>,
    /// Device-rate samples not yet consumed by a full block.
    pending: Vec<f32>,
    /// Output frames of leading silence still to discard (the resampler's
    /// delay). Counted down once, across the first calls, never per chunk.
    delay_remaining: usize,
}

impl StreamResampler {
    pub fn new(src_rate: u32) -> Result<Self, ResampleError> {
        if src_rate == TARGET_RATE {
            return Ok(Self {
                inner: None,
                pending: Vec::new(),
                delay_remaining: 0,
            });
        }
        let resampler = Fft::<f32>::new(
            src_rate as usize,
            TARGET_RATE as usize,
            1024,
            1,
            FixedSync::Both,
        )
        .map_err(|e| ResampleError::Construct {
            src_rate,
            detail: e.to_string(),
        })?;
        let delay_remaining = resampler.output_delay();
        Ok(Self {
            inner: Some(resampler),
            pending: Vec::new(),
            delay_remaining,
        })
    }

    /// Feed device-rate samples; get back whatever 16 kHz output is ready.
    pub fn push(&mut self, samples: &[f32]) -> Result<Vec<f32>, ResampleError> {
        let Some(resampler) = self.inner.as_mut() else {
            return Ok(samples.to_vec());
        };
        self.pending.extend_from_slice(samples);
        let mut out = Vec::new();
        loop {
            let need = resampler.input_frames_next();
            if self.pending.len() < need {
                break;
            }
            let produced = resampler.output_frames_next();
            let input = InterleavedSlice::new(&self.pending[..need], 1, need)
                .map_err(|e| ResampleError::Process(e.to_string()))?;
            let mut output = InterleavedOwned::<f32>::new(0.0, 1, produced);
            let (_, written) = resampler
                .process_into_buffer(&input, &mut output, None)
                .map_err(|e| ResampleError::Process(e.to_string()))?;
            self.pending.drain(..need);
            let mut block = output.take_data();
            block.truncate(written);
            // Trim the startup silence once, spread over however many blocks
            // it takes; the rest of the stream is untouched.
            if self.delay_remaining > 0 {
                let drop = self.delay_remaining.min(block.len());
                block.drain(..drop);
                self.delay_remaining -= drop;
            }
            out.extend_from_slice(&block);
        }
        Ok(out)
    }

    /// Flush the tail: zero-pad to one last block so trailing audio is not
    /// silently dropped. Call once, after the final [`push`](Self::push).
    pub fn drain(&mut self) -> Result<Vec<f32>, ResampleError> {
        if self.inner.is_none() || self.pending.is_empty() {
            return Ok(Vec::new());
        }
        let need = self
            .inner
            .as_ref()
            .expect("checked above")
            .input_frames_next();
        let mut tail = std::mem::take(&mut self.pending);
        let real = tail.len();
        tail.resize(need.max(real), 0.0);
        let mut out = self.push(&tail[..])?;
        // Keep only the output the real samples earned; the padding's share is
        // silence we invented.
        let keep = resampled_len(real, self.src_rate_hint());
        out.truncate(keep.min(out.len()));
        Ok(out)
    }

    /// The configured input rate, recovered from the resampler ratio (only
    /// used by `drain` for its trim length).
    fn src_rate_hint(&self) -> u32 {
        match self.inner.as_ref() {
            None => TARGET_RATE,
            Some(r) => (TARGET_RATE as f64 / r.resample_ratio()).round() as u32,
        }
    }
}

/// The exact output length `resample_to_16k` produces for a given input
/// length: ceil(len * 16000 / src_rate), matching rubato's `process_all`
/// contract (`expected_output_len = ceil(ratio * input_len)`). Pure, for
/// window-budget math.
pub fn resampled_len(input_len: usize, src_rate: u32) -> usize {
    ((input_len as u64 * TARGET_RATE as u64).div_ceil(src_rate as u64)) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 440 Hz sine at a given rate: real signal content so resampler output
    /// is non-degenerate.
    fn sine(rate: u32, seconds: f32) -> Vec<f32> {
        let n = (rate as f32 * seconds) as usize;
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / rate as f32).sin() * 0.5)
            .collect()
    }

    /// Mean absolute difference over the overlapping prefix.
    fn mean_abs_diff(a: &[f32], b: &[f32]) -> f32 {
        let n = a.len().min(b.len());
        assert!(n > 0, "no overlap to compare");
        (0..n).map(|i| (a[i] - b[i]).abs()).sum::<f32>() / n as f32
    }

    #[test]
    fn streaming_in_small_chunks_matches_whole_clip_resampling() {
        // The whole point of StreamResampler: feeding the same signal in
        // 20 ms pieces must produce essentially the same 16 kHz audio as
        // resampling the finished clip in one go. If the resampler were
        // rebuilt per chunk (or the startup delay trimmed per chunk), the
        // seams would show up here as a large divergence.
        let clip = sine(48_000, 1.0);
        let whole = resample_to_16k(&clip, 48_000).unwrap();

        let mut streamer = StreamResampler::new(48_000).unwrap();
        let mut streamed = Vec::new();
        for chunk in clip.chunks(960) {
            streamed.extend(streamer.push(chunk).unwrap());
        }
        streamed.extend(streamer.drain().unwrap());

        assert!(
            streamed.len().abs_diff(whole.len()) <= 1024,
            "length drift: streamed {} vs whole {}",
            streamed.len(),
            whole.len()
        );
        let diff = mean_abs_diff(&streamed, &whole);
        assert!(
            diff < 0.01,
            "streamed output diverged from whole-clip: {diff}"
        );
    }

    #[test]
    fn chunk_size_does_not_change_the_result() {
        // The caller picks chunk sizes from whatever the ring happens to hold,
        // so output must not depend on how the input was sliced.
        let clip = sine(44_100, 0.5);
        let mut runs = Vec::new();
        for size in [441, 1000, 4410] {
            let mut streamer = StreamResampler::new(44_100).unwrap();
            let mut out = Vec::new();
            for chunk in clip.chunks(size) {
                out.extend(streamer.push(chunk).unwrap());
            }
            out.extend(streamer.drain().unwrap());
            runs.push(out);
        }
        for pair in runs.windows(2) {
            let diff = mean_abs_diff(&pair[0], &pair[1]);
            assert!(diff < 0.01, "chunk size changed the output: {diff}");
        }
    }

    #[test]
    fn streaming_at_16k_is_a_passthrough_with_no_buffering() {
        // Nothing to resample, so nothing may be withheld: a live session must
        // get every sample immediately on a 16 kHz device.
        let mut streamer = StreamResampler::new(16_000).unwrap();
        let out = streamer.push(&[0.1, 0.2, 0.3]).unwrap();
        assert_eq!(out, vec![0.1, 0.2, 0.3]);
        assert!(streamer.drain().unwrap().is_empty());
    }

    #[test]
    fn streaming_holds_back_a_partial_block_until_it_fills() {
        // A short push cannot produce output yet; the samples must be kept,
        // not dropped.
        let mut streamer = StreamResampler::new(48_000).unwrap();
        assert!(streamer.push(&[0.0; 16]).unwrap().is_empty());
        let later = streamer.push(&vec![0.0; 48_000]).unwrap();
        assert!(!later.is_empty(), "buffered samples never came out");
    }

    #[test]
    fn draining_recovers_the_tail_rather_than_dropping_it() {
        // Half a second in, with a deliberately unaligned length: whatever is
        // still pending has to come out, or the end of every dictation is lost.
        let clip = sine(48_000, 0.5);
        let mut streamer = StreamResampler::new(48_000).unwrap();
        let mut streamed: Vec<f32> = Vec::new();
        for chunk in clip.chunks(1234) {
            streamed.extend(streamer.push(chunk).unwrap());
        }
        let before = streamed.len();
        streamed.extend(streamer.drain().unwrap());
        assert!(streamed.len() > before, "drain produced nothing");
        // Within a block of the expected 8000 output samples.
        assert!(
            streamed.len().abs_diff(8_000) <= 1024,
            "tail length wrong: {}",
            streamed.len()
        );
    }

    #[test]
    fn passthrough_at_16k() {
        let clip = sine(16_000, 0.25);
        let out = resample_to_16k(&clip, 16_000).unwrap();
        assert_eq!(out, clip);
    }

    #[test]
    fn exact_3_to_1_from_48k() {
        // 1 second at 48 kHz must give exactly 16000 output samples.
        let clip = sine(48_000, 1.0);
        assert_eq!(clip.len(), 48_000);
        let out = resample_to_16k(&clip, 48_000).unwrap();
        assert_eq!(out.len(), 16_000);
    }

    #[test]
    fn non_integer_ratio_from_44_1k() {
        // 1 second at 44.1 kHz: round(44100 * 16000 / 44100) = 16000.
        let clip = sine(44_100, 1.0);
        assert_eq!(clip.len(), 44_100);
        let out = resample_to_16k(&clip, 44_100).unwrap();
        assert_eq!(out.len(), 16_000);

        // A non-second-aligned length exercises the rounding path:
        // 22050 samples -> round(22050 * 16000/44100) = 8000.
        let half = sine(44_100, 0.5);
        let out = resample_to_16k(&half, 44_100).unwrap();
        assert_eq!(out.len(), resampled_len(half.len(), 44_100));
        assert_eq!(out.len(), 8_000);
    }

    #[test]
    fn short_clip_smaller_than_one_chunk() {
        // Push-to-talk utterances can be tiny; shorter than the 1024-frame
        // internal chunk must still resample, not panic.
        let clip = sine(48_000, 0.005); // 240 samples
        let out = resample_to_16k(&clip, 48_000).unwrap();
        assert_eq!(out.len(), resampled_len(clip.len(), 48_000));
        assert_eq!(out.len(), 80);
    }

    #[test]
    fn empty_input_is_empty_output() {
        let out = resample_to_16k(&[], 48_000).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn signal_survives_resampling() {
        // A pure tone must come out with comparable energy (not silence, not
        // garbage): guards against the startup-delay-not-trimmed failure mode
        // where the head of the output is zeros.
        let clip = sine(48_000, 0.5);
        let out = resample_to_16k(&clip, 48_000).unwrap();
        let rms_in = (clip.iter().map(|s| s * s).sum::<f32>() / clip.len() as f32).sqrt();
        let rms_out = (out.iter().map(|s| s * s).sum::<f32>() / out.len() as f32).sqrt();
        assert!(
            (rms_in - rms_out).abs() < 0.05,
            "rms in {rms_in} vs out {rms_out}: output is not the same signal"
        );
        // The first few ms must carry signal, not startup-delay silence.
        let head = &out[..160]; // 10 ms
        let head_rms = (head.iter().map(|s| s * s).sum::<f32>() / head.len() as f32).sqrt();
        assert!(
            head_rms > 0.1,
            "head is silent ({head_rms}): startup delay not trimmed"
        );
    }

    #[test]
    fn downmix_stereo_averages_frames() {
        let out = downmix_interleaved(&[0.2, 0.4, 1.0, -1.0], 2);
        assert_eq!(out.len(), 2);
        assert!((out[0] - 0.3).abs() < 1e-6);
        assert!(out[1].abs() < 1e-6);
    }

    #[test]
    fn downmix_mono_is_identity() {
        assert_eq!(downmix_interleaved(&[0.1, 0.2], 1), vec![0.1, 0.2]);
    }

    #[test]
    fn downmix_drops_trailing_partial_frame() {
        assert_eq!(downmix_interleaved(&[0.5, 0.5, 0.9], 2).len(), 1);
    }

    #[test]
    fn resampled_len_matches_known_ratios() {
        assert_eq!(resampled_len(48_000, 48_000), 16_000);
        assert_eq!(resampled_len(44_100, 44_100), 16_000);
        assert_eq!(resampled_len(240, 48_000), 80);
        assert_eq!(resampled_len(0, 48_000), 0);
    }
}

//! Device-rate -> 16 kHz mono resampling.
//!
//! WASAPI shared mode does not resample for us and rarely offers 16 kHz:
//! devices commonly run at 48 kHz (exact 3:1) or 44.1 kHz (non-integer
//! ratio), so a general resampler is the safe default. rubato 4.0's
//! `process_all` is the whole-clip path: it trims the FFT startup delay and
//! returns exact output frame counts, unlike a single oversized `process()`
//! call which would leave leading silence and truncate the tail.

use rubato::audioadapter_buffers::direct::InterleavedSlice;
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

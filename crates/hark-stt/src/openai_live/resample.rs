//! The replay path's upsample from the stored 16 kHz clip to the rate the
//! realtime API accepts.

use super::wire::INPUT_RATE;
use crate::error::SttError;

/// Upsample a 16 kHz clip to [`INPUT_RATE`] for a replay.
///
/// One whole-clip `process_all`, which trims the FFT startup delay once and
/// returns exactly `ceil(len * 1.5)` samples (LL-G Rust HIGH,
/// `rubato-4-whole-clip-process-all`). Same configuration as hark-audio's
/// resampler, which this crate does not depend on.
pub fn upsample_16k_to_24k(samples: &[f32]) -> Result<Vec<f32>, SttError> {
    use rubato::audioadapter_buffers::direct::InterleavedSlice;
    use rubato::{Fft, FixedSync, Resampler};

    if samples.is_empty() {
        return Ok(Vec::new());
    }
    let bad = |what: &str, e: &dyn std::fmt::Display| {
        SttError::BadAudio(format!(
            "could not resample the clip to 24 kHz: {what} ({e})"
        ))
    };
    let mut resampler = Fft::<f32>::new(
        crate::wav::SAMPLE_RATE as usize,
        INPUT_RATE as usize,
        1024,
        1,
        FixedSync::Both,
    )
    .map_err(|e| bad("resampler", &e))?;
    let input = InterleavedSlice::new(samples, 1, samples.len()).map_err(|e| bad("input", &e))?;
    let output = resampler
        .process_all(&input, samples.len(), None)
        .map_err(|e| bad("process", &e))?;
    Ok(output.take_data())
}

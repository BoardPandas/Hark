//! Hark audio capture: cpal input stream into a lock-free ring buffer,
//! device-rate -> 16 kHz mono resampling, pre-roll/tail window assembly,
//! and the silence gate.
//!
//! Layering: `ring`, `resample`, and `window` are pure and unit-tested on
//! any machine; `capture_win` is the thin cpal glue that only real hardware
//! can validate. `assemble_window` composes the pure parts into the one
//! operation the pipeline worker needs.

pub mod capture_win;
pub mod gain;
pub mod level;
pub mod resample;
pub mod ring;
pub mod window;

pub use capture_win::{
    communications_default_device, list_input_devices, start, CaptureError, CaptureHandle,
};
pub use level::LevelMeter;
pub use resample::TARGET_RATE;
pub use ring::{Consumer, Producer, RangeError};
pub use window::{GateVerdict, WindowParams};

use std::time::{Duration, Instant};
use thiserror::Error;

/// One assembled dictation clip, ready for WAV encoding: 16 kHz mono.
pub struct AudioClip {
    pub samples_16k: Vec<f32>,
    /// The rate the audio was captured at (before resampling), for logs.
    pub source_rate: u32,
    /// The normalization gain applied to `samples_16k` (1.0 = untouched).
    /// Carried for logging: a clip that needed a large boost is the signature
    /// of a microphone the user should be told about.
    pub applied_gain: f32,
}

/// The outcome of assembling a window: either a clip to transcribe, or the
/// reason it was dropped. The reason is carried rather than collapsed to
/// `None` so callers can tell the user *which* gate stopped them — "you
/// tapped the key" and "we could not hear you" need different responses.
// Safe to derive: AudioClip's own Debug prints lengths and rates, never samples.
#[derive(Debug)]
pub enum Assembled {
    Clip(AudioClip),
    Gated(GateVerdict),
}

// Deliberately no Debug derive: a reflexive `{clip:?}` in some future log
// line must not dump raw audio. Lengths and rates only.
impl std::fmt::Debug for AudioClip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioClip")
            .field("samples_16k_len", &self.samples_16k.len())
            .field("source_rate", &self.source_rate)
            .field("applied_gain", &self.applied_gain)
            .finish()
    }
}

#[derive(Debug, Error)]
pub enum AssembleError {
    #[error(
        "audio stream stalled: waited {waited_ms} ms for sample {needed} \
         but capture stopped at {written} (cpal #970 or device lost)"
    )]
    StreamStalled {
        needed: u64,
        written: u64,
        waited_ms: u128,
    },
    #[error("ring read failed: {0}")]
    Ring(#[from] ring::RangeError),
    #[error(transparent)]
    Resample(#[from] resample::ResampleError),
}

/// Assemble `pre-roll + utterance + tail` for a chord press/release pair, run
/// the gates, and normalize. [`Assembled::Gated`] means "drop it, no request",
/// and carries which gate fired.
///
/// Blocks (bounded) until the ring has produced the tail samples: the tail
/// extends past the release instant, so those samples are still arriving
/// when the worker gets the Up event. Waiting is capped at the tail length
/// plus one second; a stalled stream is a hard error, not a silent hang.
pub fn assemble_window(
    consumer: &Consumer,
    source_rate: u32,
    down_abs: u64,
    up_abs: u64,
    params: &WindowParams,
) -> Result<Assembled, AssembleError> {
    // Misfire gate first: a too-short hold costs nothing, not even waiting
    // for the tail.
    if window::gate_hold(down_abs, up_abs, source_rate, params) == GateVerdict::TooShort {
        return Ok(Assembled::Gated(GateVerdict::TooShort));
    }

    let (start, end) = window::window_bounds(down_abs, up_abs, source_rate, params);

    // Wait for the tail to be produced.
    let budget = Duration::from_millis(params.tail_ms as u64 + 1_000);
    let began = Instant::now();
    while consumer.total_written() < end {
        if began.elapsed() > budget {
            return Err(AssembleError::StreamStalled {
                needed: end,
                written: consumer.total_written(),
                waited_ms: began.elapsed().as_millis(),
            });
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    // Clamp the pre-roll to what the ring still holds (startup edge: the ring
    // is sized so a maximal window fits, but a press within the first
    // pre-roll milliseconds after launch reaches before sample zero).
    let start = start.max(consumer.oldest_available());
    let device_samples = match consumer.read_range(start, end) {
        Ok(s) => s,
        // The producer lapped us mid-copy (pathological scheduling stall).
        // One retry with a freshly clamped start.
        Err(ring::RangeError::Overwritten { oldest, .. }) => {
            consumer.read_range(start.max(oldest), end)?
        }
        Err(e) => return Err(e.into()),
    };

    if window::gate_clip(&device_samples, source_rate, params) == GateVerdict::TooQuiet {
        // Log the numbers that decided it: "too quiet" without the measurement
        // cannot distinguish a muted mic from a threshold set too high, and
        // without the floor cannot show how close the clip came.
        log::info!(
            "clip gated as too quiet: loudest {} ms window {:.4} (noise floor {:.4}) \
             below threshold {:.4}; {} samples at {} Hz",
            window::GATE_WINDOW_MS,
            window::clip_loudness(&device_samples, source_rate),
            window::noise_floor(&device_samples, source_rate),
            params.silence_rms,
            device_samples.len(),
            source_rate,
        );
        return Ok(Assembled::Gated(GateVerdict::TooQuiet));
    }

    let mut samples_16k = resample::resample_to_16k(&device_samples, source_rate)?;
    // Normalize after resampling: same audio, a third of the samples to touch,
    // and it is the buffer that actually goes to the provider.
    let applied_gain = gain::normalize(&mut samples_16k, TARGET_RATE);
    if applied_gain > 1.0 {
        log::debug!("input normalized: gain {applied_gain:.2}x");
    }
    Ok(Assembled::Clip(AudioClip {
        samples_16k,
        source_rate,
        applied_gain,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ring::ring;

    const RATE: u32 = 48_000;

    fn params() -> WindowParams {
        WindowParams::default()
    }

    /// Unwrap an assembly that must have produced a clip.
    fn clip_of(a: Assembled) -> AudioClip {
        match a {
            Assembled::Clip(c) => c,
            Assembled::Gated(v) => panic!("expected a clip, was gated: {v:?}"),
        }
    }

    /// The verdict of an assembly that must have been gated.
    fn gated_as(a: Assembled) -> GateVerdict {
        match a {
            Assembled::Clip(_) => panic!("expected a gate, got a clip"),
            Assembled::Gated(v) => v,
        }
    }

    /// A synthetic ring pre-filled with a 440 Hz tone: assembly never waits.
    fn tone_ring(total_samples: u64) -> (crate::ring::Producer, Consumer) {
        let (p, c) = ring(window::ring_capacity(RATE, &params()));
        let tone: Vec<f32> = (0..total_samples)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / RATE as f32).sin() * 0.3)
            .collect();
        p.push(&tone);
        (p, c)
    }

    #[test]
    fn assembles_preroll_utterance_tail_and_resamples() {
        // 3 s of audio; press at 1 s, release at 2 s.
        let (_p, c) = tone_ring(3 * RATE as u64);
        let down = RATE as u64;
        let up = 2 * RATE as u64;
        let clip =
            clip_of(assemble_window(&c, RATE, down, up, &params()).expect("assembly succeeds"));
        // Window = 300 ms pre-roll + 1 s hold + 150 ms tail = 1.45 s at 48 kHz,
        // resampled 3:1 to 16 kHz.
        let device_len =
            window::ms_to_samples(300, RATE) + RATE as u64 + window::ms_to_samples(150, RATE);
        assert_eq!(
            clip.samples_16k.len(),
            resample::resampled_len(device_len as usize, RATE)
        );
        assert_eq!(clip.source_rate, RATE);
    }

    #[test]
    fn too_short_hold_is_gated_without_waiting() {
        // 100 ms hold < 250 ms minimum. The ring is EMPTY: if gating did not
        // happen first, assembly would wait for samples and time out.
        let (_p, c) = ring(1024);
        let outcome = assemble_window(&c, RATE, 0, window::ms_to_samples(100, RATE), &params())
            .expect("gating is not an error");
        assert_eq!(gated_as(outcome), GateVerdict::TooShort);
    }

    #[test]
    fn silent_clip_is_gated() {
        let (p, c) = ring(window::ring_capacity(RATE, &params()));
        p.push(&vec![0.0_f32; 3 * RATE as usize]);
        let outcome = assemble_window(&c, RATE, RATE as u64, 2 * RATE as u64, &params())
            .expect("assembly succeeds");
        assert_eq!(
            gated_as(outcome),
            GateVerdict::TooQuiet,
            "silence must not reach the network"
        );
    }

    #[test]
    fn press_at_startup_clamps_preroll() {
        // Press at 50 ms after launch: pre-roll reaches before sample zero.
        let (_p, c) = tone_ring(2 * RATE as u64);
        let down = window::ms_to_samples(50, RATE);
        let up = down + RATE as u64;
        let clip =
            clip_of(assemble_window(&c, RATE, down, up, &params()).expect("assembly succeeds"));
        // Only 50 ms of pre-roll exists; window = 50 ms + 1 s + 150 ms.
        let device_len = down + RATE as u64 + window::ms_to_samples(150, RATE);
        assert_eq!(
            clip.samples_16k.len(),
            resample::resampled_len(device_len as usize, RATE)
        );
    }

    #[test]
    fn stalled_stream_is_a_hard_error() {
        // Ring has 1 s of audio but the window needs the tail past it and the
        // producer is dead. Shrink the wait budget by using a tiny tail so
        // the test stays fast... the budget is tail + 1 s, so this test costs
        // ~1 s of wall time. Acceptable: it guards the cpal #970 failure mode.
        let (_p, c) = tone_ring(RATE as u64);
        let down = 0;
        let up = RATE as u64; // tail extends past everything produced
        let err = assemble_window(&c, RATE, down, up, &params())
            .expect_err("no producer: assembly must fail, not hang");
        assert!(matches!(err, AssembleError::StreamStalled { .. }));
    }
}

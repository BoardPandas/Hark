//! Meeting-worker echo cancellation for paired 10 ms, 16 kHz mono frames.
//!
//! The render reference is the remote side; only the microphone output changes.
//! Construct, process, reset, and drop this engine on the meeting worker. The
//! engine allocates and its graph is not `Send`; never call it from an audio
//! callback. No recording, network, device, or background-thread access occurs.

use aec3::graph::{GraphError, Packet, PacketMeta};
use aec3::nodes::audio::{AudioChunk, AudioFormat};
use aec3::pipelines::linear::{self, LinearPipeline};

pub const SAMPLE_RATE: u32 = 16_000;
pub const FRAME_SAMPLES: usize = 160;
/// Fixed 8 ms output latency for the pinned 16 kHz AEC3 configuration: 64
/// samples of frame transport plus 64 of suppression-window overlap. This is
/// separate from the estimated acoustic/render delay. Compensate on startup
/// and reset, retaining original mic samples for an unprocessed ending/fallback.
pub const OUTPUT_DELAY_SAMPLES: usize = 128;

#[derive(Debug, thiserror::Error)]
pub enum MeetingAecError {
    #[error("meeting AEC {stream} frame has {actual} samples; expected {FRAME_SAMPLES}")]
    FrameLength { stream: &'static str, actual: usize },
    #[error("meeting AEC {stream} contains a non-finite sample")]
    NonFinite { stream: &'static str },
    #[error("meeting AEC produced no microphone output")]
    MissingOutput,
    #[error("meeting AEC {stage} failed: {source}")]
    Engine {
        stage: &'static str,
        #[source]
        source: GraphError,
    },
}

/// A stateful acoustic echo canceller owned by one meeting worker.
///
/// Automatic delay estimation and the high-pass filter match the evaluated
/// bakeoff configuration. Noise suppression, gain control, and the additional
/// post-filter are disabled. The reference and microphone inputs are immutable.
pub struct MeetingAec {
    pipeline: LinearPipeline,
    sequence: u64,
}

impl MeetingAec {
    pub fn new() -> Result<Self, MeetingAecError> {
        let format = AudioFormat::ten_ms(SAMPLE_RATE, 1);
        let pipeline = linear::builder(format, format)
            .enable_high_pass_filter(true)
            .enable_noise_suppression(false)
            .enable_gain_controller2(false)
            .enable_post_filter(false)
            .build()
            .map_err(|source| MeetingAecError::Engine {
                stage: "initialization",
                source,
            })?;
        Ok(Self {
            pipeline,
            sequence: 0,
        })
    }

    /// Discard adaptation, filter history, and queued audio after a discontinuity.
    /// Rebuild on the owning worker. On failure the previous engine is retained;
    /// the caller must keep bypassing rather than process discontinuous audio.
    pub fn reset(&mut self) -> Result<(), MeetingAecError> {
        *self = Self::new()?;
        Ok(())
    }

    /// Process paired render/microphone frames without changing either input.
    ///
    /// All three slices must contain exactly 160 samples. Inputs must be finite
    /// PCM; finite overshoot is preserved for the spool's existing conversion.
    /// On any error, `output` is unchanged: preserve
    /// the original mic frame and reset or disable this engine before continuing.
    /// Feed complete, chronological pairs; buffer partial frames outside this
    /// wrapper. There is no synthetic final frame or padding here.
    pub fn process_frame(
        &mut self,
        render: &[f32],
        mic: &[f32],
        output: &mut [f32],
    ) -> Result<(), MeetingAecError> {
        validate_samples("render", render)?;
        validate_samples("microphone", mic)?;
        validate_length("output", output.len())?;

        self.sequence = self.sequence.wrapping_add(1);
        let meta = PacketMeta {
            sequence: Some(self.sequence),
            ..PacketMeta::default()
        };
        self.pipeline
            .handle_render_frame_with_meta(render, meta.clone())
            .map_err(|source| MeetingAecError::Engine {
                stage: "render processing",
                source,
            })?;

        // Pull the packet ourselves: the convenience capture method copies its
        // samples immediately and panics if a faulty node returns a wrong size.
        let capture = self.pipeline.handles().capture;
        let format = self.pipeline.capture_format();
        let runtime = self.pipeline.runtime_mut();
        runtime
            .push(
                capture,
                Packet {
                    meta,
                    payload: AudioChunk::from_interleaved(format, mic),
                },
            )
            .and_then(|()| runtime.run_until_stalled())
            .map_err(|source| MeetingAecError::Engine {
                stage: "microphone processing",
                source,
            })?;
        let packet = self
            .pipeline
            .try_pull_output()
            .map_err(|source| MeetingAecError::Engine {
                stage: "output retrieval",
                source,
            })?
            .ok_or(MeetingAecError::MissingOutput)?;
        copy_valid_output(packet.payload().samples(), output)
    }
}

fn validate_length(stream: &'static str, actual: usize) -> Result<(), MeetingAecError> {
    if actual != FRAME_SAMPLES {
        return Err(MeetingAecError::FrameLength { stream, actual });
    }
    Ok(())
}

fn validate_samples(stream: &'static str, samples: &[f32]) -> Result<(), MeetingAecError> {
    validate_length(stream, samples.len())?;
    if samples.iter().any(|sample| !sample.is_finite()) {
        return Err(MeetingAecError::NonFinite { stream });
    }
    Ok(())
}

fn copy_valid_output(samples: &[f32], output: &mut [f32]) -> Result<(), MeetingAecError> {
    validate_samples("processed microphone", samples)?;
    output.copy_from_slice(samples);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voice(frames: usize) -> Vec<f32> {
        (0..frames * FRAME_SAMPLES)
            .map(|i| {
                let t = i as f32 / SAMPLE_RATE as f32;
                0.12 * (std::f32::consts::TAU * 440.0 * t).sin()
                    + 0.07 * (std::f32::consts::TAU * 1100.0 * t).sin()
            })
            .collect()
    }

    fn energy(samples: &[f32]) -> f64 {
        samples
            .iter()
            .map(|sample| f64::from(*sample).powi(2))
            .sum()
    }

    #[test]
    fn processing_delay_matches_identical_high_pass_reference() {
        let mut state = 0x7261_3279_u32;
        let mic: Vec<f32> = (0..160 * 250)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                (state as f32 / u32::MAX as f32 * 2.0 - 1.0) * 0.2
            })
            .collect();
        let mut engine = MeetingAec::new().unwrap();
        let mut high_pass = MeetingAec::new().unwrap();
        let node = high_pass.pipeline.handles().aec3.node_id();
        high_pass
            .pipeline
            .set_node_state(node, aec3::graph::NodeControlState::Bypassed)
            .unwrap();
        let mut expected = Vec::new();
        let mut actual = Vec::new();
        for frame in mic.as_chunks::<FRAME_SAMPLES>().0 {
            let mut output = [0.0; FRAME_SAMPLES];
            high_pass
                .process_frame(&[0.0; FRAME_SAMPLES], frame, &mut output)
                .unwrap();
            expected.extend(output);
            engine
                .process_frame(&[0.0; FRAME_SAMPLES], frame, &mut output)
                .unwrap();
            actual.extend(output);
        }
        let best = (0..320)
            .map(|delay| {
                let a = &expected[1600..36_000];
                let b = &actual[1600 + delay..36_000 + delay];
                let dot: f64 = a
                    .iter()
                    .zip(b)
                    .map(|(a, b)| f64::from(*a) * f64::from(*b))
                    .sum();
                let corr = dot / (energy(a) * energy(b)).sqrt();
                (delay, corr)
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap();
        assert_eq!(best.0, OUTPUT_DELAY_SAMPLES);
        assert!(best.1 > 0.99);
    }

    #[test]
    fn fixed_delay_and_final_impulse_survive_after_echo_adaptation_and_reset() {
        for acoustic_delay in [640, 2_560] {
            let render = voice(600);
            let mut engine = MeetingAec::new().unwrap();
            let mut output = [0.0; FRAME_SAMPLES];
            for (frame_index, reference) in render.as_chunks::<FRAME_SAMPLES>().0.iter().enumerate()
            {
                let capture = std::array::from_fn::<_, FRAME_SAMPLES, _>(|i| {
                    (frame_index * FRAME_SAMPLES + i)
                        .checked_sub(acoustic_delay)
                        .map_or(0.0, |j| render[j] * 0.55)
                });
                engine
                    .process_frame(reference, &capture, &mut output)
                    .unwrap();
            }
            // Compare adapted and freshly reset state. A final isolated near
            // impulse exposes real buffered audio, which counts alone cannot.
            for reset in [false, true] {
                if reset {
                    engine.reset().unwrap();
                }
                let silence = [0.0; FRAME_SAMPLES];
                // Stop the far end and leave its acoustic tail behind first.
                for _ in 0..100 {
                    engine
                        .process_frame(&silence, &silence, &mut output)
                        .unwrap();
                }
                let mut final_frame = silence;
                final_frame[FRAME_SAMPLES - 1] = 0.5;
                let mut decoded = Vec::new();
                for frame in [final_frame, silence, silence] {
                    engine.process_frame(&silence, &frame, &mut output).unwrap();
                    decoded.extend(output);
                }
                let peak = decoded
                    .iter()
                    .enumerate()
                    .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
                    .unwrap();
                assert_eq!(
                    peak.0,
                    FRAME_SAMPLES - 1 + OUTPUT_DELAY_SAMPLES,
                    "acoustic delay {acoustic_delay}, reset {reset}"
                );
                assert!(peak.1.abs() > 0.1, "ending impulse must survive");
                // The extra zero input is only a diagnostic drain: trimming
                // startup latency recovers a complete original-length frame.
                let aligned = &decoded[OUTPUT_DELAY_SAMPLES..OUTPUT_DELAY_SAMPLES + FRAME_SAMPLES];
                assert!(aligned[FRAME_SAMPLES - 1].abs() > 0.1);
            }
        }
    }

    #[test]
    fn silence_produces_complete_finite_silent_frames() {
        let mut engine = MeetingAec::new().unwrap();
        let silence = [0.0; FRAME_SAMPLES];
        for _ in 0..100 {
            let mut output = [f32::NAN; FRAME_SAMPLES];
            engine
                .process_frame(&silence, &silence, &mut output)
                .unwrap();
            assert!(output.iter().all(|sample| sample.is_finite()));
            assert!(energy(&output) < 1e-12);
        }
    }

    #[test]
    fn near_only_audio_survives_without_gain_control() {
        let mut engine = MeetingAec::new().unwrap();
        let mic = voice(200);
        let render = [0.0; FRAME_SAMPLES];
        let mut processed = Vec::with_capacity(mic.len());
        for frame in mic.as_chunks::<FRAME_SAMPLES>().0 {
            let mut output = [0.0; FRAME_SAMPLES];
            engine.process_frame(&render, frame, &mut output).unwrap();
            processed.extend(output);
        }
        let score_start = SAMPLE_RATE as usize;
        let ratio = energy(&processed[score_start..]) / energy(&mic[score_start..]);
        assert!((0.8..1.2).contains(&ratio), "near-only power ratio {ratio}");
    }

    #[test]
    fn adapted_engine_reduces_delayed_synthetic_echo_without_mutating_reference() {
        let frames = 800;
        let mut state = 0x46a1_37c9_u32;
        let mut low = 0.0;
        let render: Vec<f32> = (0..frames * FRAME_SAMPLES)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                let noise = state as f32 / u32::MAX as f32 * 2.0 - 1.0;
                low = 0.55 * low + 0.45 * noise;
                0.2 * low
            })
            .collect();
        let mut mic = vec![0.0; render.len()];
        for (i, sample) in mic.iter_mut().enumerate() {
            for (delay, scale) in [(640, 0.55), (677, 0.28), (753, -0.13)] {
                if i >= delay {
                    *sample += render[i - delay] * scale;
                }
            }
        }
        let render_before = render.clone();
        let mic_before = mic.clone();
        let mut engine = MeetingAec::new().unwrap();
        let (mut before, mut after) = (0.0, 0.0);
        for (index, (reference, capture)) in render
            .as_chunks::<FRAME_SAMPLES>()
            .0
            .iter()
            .zip(mic.as_chunks::<FRAME_SAMPLES>().0)
            .enumerate()
        {
            let mut output = [0.0; FRAME_SAMPLES];
            engine
                .process_frame(reference, capture, &mut output)
                .unwrap();
            assert!(output.iter().all(|sample| sample.is_finite()));
            // Leave six seconds for automatic delay estimation/adaptation.
            if index >= 600 {
                before += energy(capture);
                after += energy(&output);
            }
        }
        assert_eq!(render, render_before, "remote archive channel is immutable");
        assert_eq!(
            mic, mic_before,
            "original microphone remains available for bypass"
        );
        assert!(before > 0.1, "fixture must contain measurable echo");
        assert!(after < before * 0.25, "echo power ratio {}", after / before);
    }

    #[test]
    fn invalid_inputs_leave_output_and_engine_state_unchanged() {
        let mut engine = MeetingAec::new().unwrap();
        let mut fresh = MeetingAec::new().unwrap();
        let valid = [0.0; FRAME_SAMPLES];
        let mut output = [0.25; FRAME_SAMPLES];
        for length in [0, FRAME_SAMPLES - 1, FRAME_SAMPLES + 1] {
            let invalid = vec![0.0; length];
            assert!(engine.process_frame(&invalid, &valid, &mut output).is_err());
            assert!(engine.process_frame(&valid, &invalid, &mut output).is_err());
            let mut wrong_output = vec![0.25; length];
            assert!(engine
                .process_frame(&valid, &valid, &mut wrong_output)
                .is_err());
            assert!(wrong_output.iter().all(|sample| *sample == 0.25));
        }
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut invalid = valid;
            invalid[FRAME_SAMPLES - 1] = bad;
            assert!(engine.process_frame(&invalid, &valid, &mut output).is_err());
            assert!(engine.process_frame(&valid, &invalid, &mut output).is_err());
        }
        assert_eq!(output, [0.25; FRAME_SAMPLES]);
        for mic in voice(30).as_chunks::<FRAME_SAMPLES>().0 {
            let mut expected = [0.0; FRAME_SAMPLES];
            fresh.process_frame(&valid, mic, &mut expected).unwrap();
            engine.process_frame(&valid, mic, &mut output).unwrap();
            assert_eq!(output, expected);
        }
    }

    #[test]
    fn malformed_engine_output_is_rejected_before_copy() {
        let mut output = [0.25; FRAME_SAMPLES];
        assert!(copy_valid_output(&[0.0; FRAME_SAMPLES - 1], &mut output).is_err());
        assert!(copy_valid_output(&[0.0; FRAME_SAMPLES + 1], &mut output).is_err());
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut samples = [0.0; FRAME_SAMPLES];
            samples[FRAME_SAMPLES - 1] = bad;
            assert!(copy_valid_output(&samples, &mut output).is_err());
        }
        assert_eq!(output, [0.25; FRAME_SAMPLES]);
        let overshoot = [1.1; FRAME_SAMPLES];
        copy_valid_output(&overshoot, &mut output).unwrap();
        assert_eq!(
            output, overshoot,
            "finite overshoot reaches normal spool clipping"
        );
        let mut engine = MeetingAec::new().unwrap();
        engine
            .process_frame(&overshoot, &overshoot, &mut output)
            .unwrap();
        assert!(output.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn missing_output_and_engine_errors_preserve_output_until_reset() {
        let mut engine = MeetingAec::new().unwrap();
        let aec_node = engine.pipeline.handles().aec3.node_id();
        engine
            .pipeline
            .set_node_state(aec_node, aec3::graph::NodeControlState::Suspended)
            .unwrap();
        let silence = [0.0; FRAME_SAMPLES];
        let mut output = [0.25; FRAME_SAMPLES];
        assert!(matches!(
            engine.process_frame(&silence, &silence, &mut output),
            Err(MeetingAecError::MissingOutput)
        ));
        assert_eq!(output, [0.25; FRAME_SAMPLES]);
        engine.reset().unwrap();

        // Inject a packet with the wrong audio format behind our input guard
        // to exercise a real engine-node error, not just boundary validation.
        let capture = engine.pipeline.handles().capture;
        engine
            .pipeline
            .runtime_mut()
            .push(
                capture,
                Packet {
                    meta: PacketMeta::default(),
                    payload: AudioChunk::silence(AudioFormat::ten_ms(48_000, 1)),
                },
            )
            .unwrap();
        assert!(matches!(
            engine.process_frame(&silence, &silence, &mut output),
            Err(MeetingAecError::Engine { .. })
        ));
        assert_eq!(output, [0.25; FRAME_SAMPLES]);
        engine.reset().unwrap();
        engine
            .process_frame(&silence, &silence, &mut output)
            .unwrap();
        assert!(
            energy(&output) < 1e-12,
            "reset clears queued samples and suspension"
        );
    }

    #[test]
    fn reset_discards_filter_history_and_matches_a_fresh_engine() {
        let mut engine = MeetingAec::new().unwrap();
        let render = [0.0; FRAME_SAMPLES];
        let mut output = [0.0; FRAME_SAMPLES];
        for mic in voice(30).as_chunks::<FRAME_SAMPLES>().0 {
            engine.process_frame(&render, mic, &mut output).unwrap();
        }
        engine.reset().unwrap();
        let mut fresh = MeetingAec::new().unwrap();
        for mic in voice(30).as_chunks::<FRAME_SAMPLES>().0 {
            let mut expected = [0.0; FRAME_SAMPLES];
            fresh.process_frame(&render, mic, &mut expected).unwrap();
            engine.process_frame(&render, mic, &mut output).unwrap();
            assert_eq!(output, expected);
        }
    }
}

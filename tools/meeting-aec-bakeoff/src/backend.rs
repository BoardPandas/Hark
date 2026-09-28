use crate::{Result, FRAME, RATE};

pub trait Backend {
    fn process(&mut self, render: &[f32], capture: &[f32], output: &mut [f32]) -> Result<()>;
}

pub fn names() -> Vec<&'static str> {
    let mut names = vec!["bypass"];
    if cfg!(feature = "rust-aec3") {
        names.push("aec3-0.4.0");
    }
    if cfg!(feature = "cpp-webrtc") {
        names.push("webrtc-2.1.0");
    }
    names
}

pub fn create(name: &str, delay: Option<u16>) -> Result<Box<dyn Backend>> {
    match name {
        "bypass" => Ok(Box::new(Bypass)),
        #[cfg(feature = "rust-aec3")]
        "aec3-0.4.0" => {
            use aec3::{nodes::audio::AudioFormat, pipelines::linear};
            let format = AudioFormat::ten_ms(RATE, 1);
            // Compare AEC + HPF only. The convenience pipeline otherwise also
            // enables NS and AGC, which would confound attenuation/preservation.
            let mut builder = linear::builder(format, format)
                .enable_high_pass_filter(true)
                .enable_noise_suppression(false)
                .enable_gain_controller2(false)
                .enable_post_filter(false);
            if let Some(ms) = delay {
                builder = builder.initial_delay_ms(i32::from(ms));
            }
            Ok(Box::new(RustAec(builder.build()?)))
        }
        #[cfg(feature = "cpp-webrtc")]
        "webrtc-2.1.0" => {
            use webrtc_audio_processing::{config, Config, Processor};
            let processor = Processor::new(RATE)?;
            processor.set_config(Config {
                echo_canceller: Some(config::EchoCanceller::Full {
                    stream_delay_ms: delay,
                }),
                high_pass_filter: Some(config::HighPassFilter::default()),
                ..Default::default()
            });
            Ok(Box::new(CppAec(processor)))
        }
        _ => {
            let _ = delay;
            Err(format!(
                "backend {name:?} unavailable; compiled: {}",
                names().join(", ")
            )
            .into())
        }
    }
}

struct Bypass;

impl Backend for Bypass {
    fn process(&mut self, _render: &[f32], capture: &[f32], output: &mut [f32]) -> Result<()> {
        output.copy_from_slice(capture);
        Ok(())
    }
}

#[cfg(feature = "rust-aec3")]
struct RustAec(aec3::pipelines::linear::LinearPipeline);

#[cfg(feature = "rust-aec3")]
impl Backend for RustAec {
    fn process(&mut self, render: &[f32], capture: &[f32], output: &mut [f32]) -> Result<()> {
        self.0.handle_render_frame(render)?;
        if !self.0.process_capture_frame(capture, output)? {
            return Err(
                "AEC3 produced no capture frame; refusing a fabricated silent output".into(),
            );
        }
        Ok(())
    }
}

#[cfg(feature = "cpp-webrtc")]
struct CppAec(webrtc_audio_processing::Processor);

#[cfg(feature = "cpp-webrtc")]
impl Backend for CppAec {
    fn process(&mut self, render: &[f32], capture: &[f32], output: &mut [f32]) -> Result<()> {
        // analyze_render_frame is the nonmutating render path. All allocations
        // inside the wrapper are included in the measured per-frame cost.
        self.0.analyze_render_frame([render])?;
        output.copy_from_slice(capture);
        self.0.process_capture_frame([output])?;
        Ok(())
    }
}

pub fn validate_frame(render: &[f32], capture: &[f32], output: &[f32]) -> Result<()> {
    if [render.len(), capture.len(), output.len()] != [FRAME; 3] {
        return Err(format!("each mono frame must hold {FRAME} samples at {RATE} Hz").into());
    }
    if render
        .iter()
        .chain(capture)
        .chain(output)
        .any(|v| !v.is_finite())
    {
        return Err("non-finite audio sample".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_backends_accept_continuous_ten_ms_frames() {
        for name in names() {
            let mut engine = create(name, None).unwrap();
            let render = [0.0; FRAME];
            for frame in 0..20 {
                let capture = std::array::from_fn::<_, FRAME, _>(|sample| {
                    ((frame * FRAME + sample) as f32 * 0.1).sin() * 0.1
                });
                let mut output = [0.0; FRAME];
                engine.process(&render, &capture, &mut output).unwrap();
                validate_frame(&render, &capture, &output).unwrap();
            }
        }
    }
}

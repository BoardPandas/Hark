mod backend;
mod fixture;
mod metrics;

use serde::Serialize;
use std::{fs, path::Path, time::Instant};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const RATE: u32 = 16_000;
const FRAME: usize = 160;

#[derive(Serialize)]
struct Measurement {
    backend: String,
    fixture: String,
    input_fingerprint: String,
    repetition: usize,
    sample_rate: u32,
    samples: usize,
    simulated_delay_ms: u16,
    simulated_drift_ppm: f64,
    delay_hint: &'static str,
    frame_p50_us: f64,
    frame_p95_us: f64,
    frame_p99_us: f64,
    frame_max_us: f64,
    processing_realtime_factor: f64,
    metrics: metrics::SignalMetrics,
}

#[derive(Serialize)]
struct Report {
    schema: u32,
    os: &'static str,
    arch: &'static str,
    profile: &'static str,
    scope: &'static str,
    measurements: Vec<Measurement>,
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("list") if args.len() == 1 => println!("{}", backend::names().join("\n")),
        Some("bench") if (2..=3).contains(&args.len()) => {
            let repetitions = args.get(2).map(|s| s.parse()).transpose()?.unwrap_or(3);
            if !(1..=20).contains(&repetitions) {
                return Err("repetitions must be 1..=20".into());
            }
            bench(Path::new(&args[1]), repetitions)?;
        }
        Some("pair") if args.len() == 4 => {
            let mut render = read_wav(Path::new(&args[1]))?;
            let mut capture = read_wav(Path::new(&args[2]))?;
            let dir = Path::new(&args[3]);
            if dir.exists() {
                return Err("pair output directory already exists; use a fresh directory".into());
            }
            let length = render.len().max(capture.len());
            let padding = (length - render.len(), length - capture.len());
            render.resize(length, 0.0);
            capture.resize(length, 0.0);
            fs::create_dir_all(dir)?;
            write_wav(&dir.join("render.wav"), &render)?;
            write_wav(&dir.join("mic.wav"), &capture)?;
            println!("paired {length} samples; trailing padding render={} mic={}", padding.0, padding.1);
        }
        Some("process") if (5..=6).contains(&args.len()) => {
            let delay = args.get(5).filter(|s| s.as_str() != "auto").map(|s| s.parse()).transpose()?;
            let render = read_wav(Path::new(&args[2]))?;
            let capture = read_wav(Path::new(&args[3]))?;
            if render.len() != capture.len() {
                return Err("render and capture lengths differ; align/pad explicitly before comparison".into());
            }
            let (output, _) = process(&args[1], &render, &capture, delay)?;
            write_wav(Path::new(&args[4]), &output)?;
            println!("processed {} samples with {}", output.len(), args[1]);
        }
        _ => return Err("usage: list | bench <new-output-dir> [repetitions] | pair <render.wav> <mic.wav> <new-output-dir> | process <backend> <render.wav> <mic.wav> <new-output.wav> [delay-ms|auto]".into()),
    }
    Ok(())
}

fn process(
    name: &str,
    render: &[f32],
    capture: &[f32],
    delay: Option<u16>,
) -> Result<(Vec<f32>, Vec<f64>)> {
    let mut engine = backend::create(name, delay)?;
    let mut output = Vec::with_capacity(capture.len());
    let mut timings = Vec::with_capacity(capture.len().div_ceil(FRAME));
    for start in (0..capture.len()).step_by(FRAME) {
        let count = FRAME.min(capture.len() - start);
        let mut far = [0.0; FRAME];
        let mut near = [0.0; FRAME];
        let mut processed = [0.0; FRAME];
        far[..count].copy_from_slice(&render[start..start + count]);
        near[..count].copy_from_slice(&capture[start..start + count]);
        backend::validate_frame(&far, &near, &processed)?;
        let began = Instant::now();
        engine.process(&far, &near, &mut processed)?;
        timings.push(began.elapsed().as_secs_f64() * 1_000_000.0);
        backend::validate_frame(&far, &near, &processed)?;
        output.extend_from_slice(&processed[..count]);
    }
    Ok((output, timings))
}

fn bench(dir: &Path, repetitions: usize) -> Result<()> {
    if dir.exists() {
        return Err(
            "output directory already exists; use a fresh directory to preserve previous evidence"
                .into(),
        );
    }
    fs::create_dir_all(dir)?;
    let mut measurements = Vec::new();
    for fixture in fixture::fixtures() {
        let identity = fixture::fingerprint(&fixture);
        // Persist exactly the same inputs for independent replay and listening.
        write_wav(
            &dir.join(format!("{}-render.wav", fixture.name)),
            &fixture.render,
        )?;
        write_wav(
            &dir.join(format!("{}-mic.wav", fixture.name)),
            &fixture.capture,
        )?;
        write_wav(
            &dir.join(format!("{}-near.wav", fixture.name)),
            &fixture.near,
        )?;
        for name in backend::names() {
            for repetition in 1..=repetitions {
                let (output, mut times) = process(name, &fixture.render, &fixture.capture, None)?;
                let realtime = times.iter().sum::<f64>()
                    / 1_000_000.0
                    / (fixture.capture.len() as f64 / f64::from(RATE));
                times.sort_by(f64::total_cmp);
                if repetition == 1 {
                    write_wav(&dir.join(format!("{}-{name}.wav", fixture.name)), &output)?;
                }
                measurements.push(Measurement {
                    backend: name.into(),
                    fixture: fixture.name.into(),
                    input_fingerprint: identity.clone(),
                    repetition,
                    sample_rate: RATE,
                    samples: output.len(),
                    simulated_delay_ms: fixture.delay_ms,
                    simulated_drift_ppm: fixture.drift_ppm,
                    delay_hint: "automatic; no oracle hint",
                    frame_p50_us: metrics::percentile(&times, 50),
                    frame_p95_us: metrics::percentile(&times, 95),
                    frame_p99_us: metrics::percentile(&times, 99),
                    frame_max_us: *times.last().unwrap(),
                    processing_realtime_factor: realtime,
                    metrics: metrics::score(&fixture, &output),
                });
                println!(
                    "finished backend={name} fixture={} repetition={repetition}",
                    fixture.name
                );
            }
        }
    }
    let report = Report {
        schema: 1, os: std::env::consts::OS, arch: std::env::consts::ARCH,
        profile: if cfg!(debug_assertions) { "debug" } else { "release" },
        scope: "Synthetic linear-room diagnostic; does not establish real-speaker quality or Windows support; AEC+HPF, NS/AGC off.",
        measurements,
    };
    fs::write(
        dir.join("results.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    let mut csv = String::from("backend,fixture,repetition,input_fingerprint,erle_db,input_snr_db,output_snr_db,near_gain_db,alignment_samples,frame_p50_us,frame_p95_us,frame_p99_us,frame_max_us,realtime_factor\n");
    for m in &report.measurements {
        let optional = |v: Option<f64>| v.map(|v| format!("{v:.6}")).unwrap_or_default();
        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{:.3},{:.3},{:.3},{:.3},{:.8}\n",
            m.backend,
            m.fixture,
            m.repetition,
            m.input_fingerprint,
            optional(m.metrics.erle_db),
            optional(m.metrics.input_snr_db),
            optional(m.metrics.output_snr_db),
            optional(m.metrics.near_gain_db),
            m.metrics
                .near_alignment_samples
                .map(|v| v.to_string())
                .unwrap_or_default(),
            m.frame_p50_us,
            m.frame_p95_us,
            m.frame_p99_us,
            m.frame_max_us,
            m.processing_realtime_factor
        ));
    }
    fs::write(dir.join("results.csv"), csv)?;
    Ok(())
}

fn read_wav(path: &Path) -> Result<Vec<f32>> {
    let mut wav = hound::WavReader::open(path)?;
    let spec = wav.spec();
    if spec.sample_rate != RATE || spec.channels != 1 {
        return Err(
            "expected 16 kHz mono WAV; do not silently resample or average channels".into(),
        );
    }
    let samples = match (spec.sample_format, spec.bits_per_sample) {
        (hound::SampleFormat::Float, 32) => wav
            .samples::<f32>()
            .collect::<std::result::Result<Vec<_>, _>>()?,
        (hound::SampleFormat::Int, 16) => wav
            .samples::<i16>()
            .map(|x| x.map(|x| f32::from(x) / 32768.0))
            .collect::<std::result::Result<Vec<_>, _>>()?,
        _ => return Err("expected PCM16 or float32 WAV".into()),
    };
    if samples.is_empty() || samples.iter().any(|x| !x.is_finite()) {
        return Err("empty or non-finite WAV input".into());
    }
    Ok(samples)
}

fn write_wav(path: &Path, samples: &[f32]) -> Result<()> {
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    let mut writer = hound::WavWriter::new(
        std::io::BufWriter::new(file),
        hound::WavSpec {
            channels: 1,
            sample_rate: RATE,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )?;
    for sample in samples {
        writer.write_sample(*sample)?;
    }
    writer.finalize()?;
    Ok(())
}

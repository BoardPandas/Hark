use crate::{fixture::Fixture, RATE};
use serde::Serialize;

#[derive(Serialize)]
pub struct SignalMetrics {
    pub erle_db: Option<f64>,
    pub input_snr_db: Option<f64>,
    pub output_snr_db: Option<f64>,
    pub near_gain_db: Option<f64>,
    pub near_alignment_samples: Option<usize>,
    pub output_peak: f64,
}

pub fn score(fixture: &Fixture, output: &[f32]) -> SignalMetrics {
    let begin = fixture.score_start;
    let end = fixture.score_end;
    let erle_db = fixture.far_only.then(|| {
        db_ratio(
            power(&fixture.capture[begin..end]),
            power(&output[begin..end]),
        )
    });
    let mut result = SignalMetrics {
        erle_db,
        input_snr_db: None,
        output_snr_db: None,
        near_gain_db: None,
        near_alignment_samples: None,
        output_peak: output
            .iter()
            .map(|s| f64::from(s.abs()))
            .fold(0.0, f64::max),
    };
    if fixture.far_only {
        return result;
    }
    // Report (do not hide) up to 20 ms of algorithmic output lag. This is a
    // diagnostic alignment against known synthetic near audio, not a real-call
    // quality score. Use the same lag search for both candidates.
    let limit = RATE as usize / 50;
    let end = end.min(output.len() - limit);
    let reference = &fixture.near[begin..end];
    let lag = (0..=limit)
        .max_by(|a, b| {
            correlation(reference, &output[begin + a..end + a])
                .total_cmp(&correlation(reference, &output[begin + b..end + b]))
        })
        .unwrap_or(0);
    let processed = &output[begin + lag..end + lag];
    let signal = power(reference);
    result.input_snr_db = Some(db_ratio(
        signal,
        difference(reference, &fixture.capture[begin..end]),
    ));
    result.output_snr_db = Some(db_ratio(signal, difference(reference, processed)));
    let gain = reference
        .iter()
        .zip(processed)
        .map(|(a, b)| f64::from(*a) * f64::from(*b))
        .sum::<f64>()
        / signal.max(1e-30);
    result.near_gain_db = Some(20.0 * gain.abs().max(1e-15).log10());
    result.near_alignment_samples = Some(lag);
    result
}

fn power(samples: &[f32]) -> f64 {
    samples.iter().map(|s| f64::from(*s).powi(2)).sum()
}

fn difference(a: &[f32], b: &[f32]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(a, b)| (f64::from(*a) - f64::from(*b)).powi(2))
        .sum()
}

fn db_ratio(a: f64, b: f64) -> f64 {
    10.0 * (a.max(1e-30) / b.max(1e-30)).log10()
}

fn correlation(a: &[f32], b: &[f32]) -> f64 {
    let dot = a
        .iter()
        .zip(b)
        .map(|(a, b)| f64::from(*a) * f64::from(*b))
        .sum::<f64>();
    dot / (power(a) * power(b)).sqrt().max(1e-30)
}

pub fn percentile(sorted: &[f64], percent: usize) -> f64 {
    sorted[((sorted.len() - 1) * percent) / 100]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn total_muting_cannot_win_near_speech_preservation() {
        let fixtures = crate::fixture::fixtures();
        let near = fixtures.iter().find(|f| f.name == "near_only").unwrap();
        let muted = score(near, &vec![0.0; near.capture.len()]);
        assert!(muted.output_snr_db.unwrap().abs() < 1e-6);
        assert!(muted.near_gain_db.unwrap() < -100.0);
        let passed = score(near, &near.capture);
        assert!(passed.output_snr_db.unwrap() > 100.0);
        assert!(passed.near_gain_db.unwrap().abs() < 1e-6);
        assert_eq!(passed.near_alignment_samples, Some(0));
    }

    #[test]
    fn amplitude_halving_is_six_db_echo_attenuation() {
        let fixtures = crate::fixture::fixtures();
        let far = &fixtures[0];
        let out: Vec<_> = far.capture.iter().map(|x| x * 0.5).collect();
        assert!((score(far, &out).erle_db.unwrap() - 6.0206).abs() < 0.001);
    }
}

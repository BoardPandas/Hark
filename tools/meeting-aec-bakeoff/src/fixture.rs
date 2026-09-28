use crate::RATE;

pub struct Fixture {
    pub name: &'static str,
    pub render: Vec<f32>,
    pub capture: Vec<f32>,
    pub near: Vec<f32>,
    pub delay_ms: u16,
    pub drift_ppm: f64,
    pub score_start: usize,
    pub score_end: usize,
    pub far_only: bool,
}

pub fn fixtures() -> Vec<Fixture> {
    vec![
        make("far_only_40ms", 30, 40, 0.0, false, false),
        make("far_only_160ms", 30, 160, 0.0, false, false),
        make("near_only", 30, 40, 0.0, true, true),
        make("double_talk_40ms", 30, 40, 0.0, true, false),
        make("double_talk_160ms", 30, 160, 0.0, true, false),
        make("far_only_drift_plus31ppm", 120, 40, 31.0, false, false),
        make("far_only_drift_minus31ppm", 120, 40, -31.0, false, false),
    ]
}

fn make(
    name: &'static str,
    seconds: usize,
    delay_ms: u16,
    drift_ppm: f64,
    has_near: bool,
    near_only: bool,
) -> Fixture {
    let len = RATE as usize * seconds;
    let render = if near_only {
        vec![0.0; len]
    } else {
        voice(len, 0x46a1_37c9, 137.0)
    };
    let mut near = if has_near {
        voice(len, 0xf286_1b03, 211.0)
    } else {
        vec![0.0; len]
    };
    // Far-only adaptation first; scored double-talk excludes both transitions.
    if has_near && !near_only {
        near[..8 * RATE as usize].fill(0.0);
        near[24 * RATE as usize..].fill(0.0);
    }
    let delay = f64::from(delay_ms) * f64::from(RATE) / 1000.0;
    let mut capture = Vec::with_capacity(len);
    for (i, local) in near.iter().enumerate() {
        // A deterministic four-tap room response with gradual clock drift.
        // No oracle delay is given to either backend in the default benchmark.
        let position = i as f64 * (1.0 + drift_ppm / 1_000_000.0) - delay;
        let echo = 0.55 * interpolate(&render, position)
            + 0.28 * interpolate(&render, position - 37.0)
            - 0.13 * interpolate(&render, position - 113.0)
            + 0.07 * interpolate(&render, position - 251.0);
        capture.push(*local + echo);
    }
    let (score_start, score_end) = if has_near && !near_only {
        (12 * RATE as usize, 23 * RATE as usize)
    } else {
        ((seconds - 10) * RATE as usize, len)
    };
    Fixture {
        name,
        render,
        capture,
        near,
        delay_ms,
        drift_ppm,
        score_start,
        score_end,
        far_only: !has_near,
    }
}

fn interpolate(samples: &[f32], index: f64) -> f32 {
    if index < 0.0 {
        return 0.0;
    }
    let i = index as usize;
    let Some(a) = samples.get(i) else { return 0.0 };
    let b = samples.get(i + 1).copied().unwrap_or(0.0);
    *a + (b - *a) * (index - i as f64) as f32
}

fn voice(len: usize, mut state: u32, fundamental: f64) -> Vec<f32> {
    let mut low = 0.0_f64;
    let mut dc = 0.0_f64;
    let mut phase = 0.0_f64;
    (0..len)
        .map(|i| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let noise = f64::from(state) / f64::from(u32::MAX) * 2.0 - 1.0;
            low = 0.55 * low + 0.45 * noise;
            dc = 0.985 * dc + 0.015 * low;
            let t = i as f64 / f64::from(RATE);
            phase += std::f64::consts::TAU * fundamental * (1.0 + 0.12 * (t * 2.1).sin())
                / f64::from(RATE);
            let harmonics = phase.sin() + 0.45 * (phase * 2.0).sin() + 0.2 * (phase * 3.0).sin();
            let envelope = 0.25 + 0.75 * (t * 9.7).sin().powi(2);
            (envelope * (0.11 * harmonics + 0.09 * (low - dc))) as f32
        })
        .collect()
}

pub fn fingerprint(fixture: &Fixture) -> String {
    // Reproducibility checksum, not a security hash. Includes the exact inputs.
    let mut value = 0xcbf2_9ce4_8422_2325_u64;
    for sample in fixture
        .render
        .iter()
        .chain(&fixture.capture)
        .chain(&fixture.near)
    {
        for byte in sample.to_bits().to_le_bytes() {
            value ^= u64::from(byte);
            value = value.wrapping_mul(0x100_0000_01b3);
        }
    }
    format!("fnv1a64:{value:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_delay_and_near_only_are_independent() {
        let far = make("test", 30, 40, 0.0, false, false);
        assert!(far.capture[..640].iter().all(|s| *s == 0.0));
        assert!(far.capture[640..].iter().any(|s| *s != 0.0));
        let near = make("test", 30, 40, 0.0, true, true);
        assert_eq!(near.capture, near.near);
        assert!(near.render.iter().all(|s| *s == 0.0));
        assert_eq!(
            fingerprint(&near),
            fingerprint(&make("test", 30, 40, 0.0, true, true))
        );
    }
}

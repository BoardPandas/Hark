//! Pre-roll/tail window math and the silence gate. Everything here is pure
//! sample-index arithmetic: no clock, no I/O, fully unit-testable.

/// Window parameters in wall-clock units; converted to sample counts against
/// the live device rate. Mirrors `hark-config`'s `[audio]` section without
/// depending on that crate.
#[derive(Debug, Clone, Copy)]
pub struct WindowParams {
    pub preroll_ms: u32,
    pub tail_ms: u32,
    pub max_hold_s: u32,
    pub min_speech_ms: u32,
    pub silence_rms: f32,
}

impl Default for WindowParams {
    fn default() -> Self {
        WindowParams {
            preroll_ms: 300,
            tail_ms: 150,
            max_hold_s: 120,
            min_speech_ms: 250,
            silence_rms: 0.01,
        }
    }
}

/// Convert milliseconds to a sample count at `rate`.
pub fn ms_to_samples(ms: u32, rate: u32) -> u64 {
    ms as u64 * rate as u64 / 1000
}

/// The absolute sample range [start, end) to extract for a dictation whose
/// chord went down at `down_abs` and up at `up_abs`.
///
/// - Pre-roll reaches back before the key registered (words started early).
///   It clamps at absolute zero for presses right after startup.
/// - Tail reaches past release (trailing word endings).
/// - Holds longer than `max_hold_s` keep the MOST RECENT audio ("transcribe
///   what we have"): the effective down moves forward, matching what the
///   (identically sized) ring buffer still holds anyway.
pub fn window_bounds(down_abs: u64, up_abs: u64, rate: u32, params: &WindowParams) -> (u64, u64) {
    debug_assert!(up_abs >= down_abs, "release before press");
    let preroll = ms_to_samples(params.preroll_ms, rate);
    let tail = ms_to_samples(params.tail_ms, rate);
    let max_hold = params.max_hold_s as u64 * rate as u64;

    let hold = up_abs.saturating_sub(down_abs);
    let effective_down = if hold > max_hold {
        up_abs - max_hold
    } else {
        down_abs
    };
    let start = effective_down.saturating_sub(preroll);
    let end = up_abs + tail;
    (start, end)
}

/// The ring capacity (in samples at `rate`) needed so a maximal window is
/// always still resident: max hold + pre-roll + tail, plus one second of
/// slack for the samples that keep arriving while the worker copies.
pub fn ring_capacity(rate: u32, params: &WindowParams) -> usize {
    let window = params.max_hold_s as u64 * rate as u64
        + ms_to_samples(params.preroll_ms, rate)
        + ms_to_samples(params.tail_ms, rate);
    (window + rate as u64) as usize
}

/// Root-mean-square amplitude of a clip. Empty clips are 0.0 (silent).
pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

/// Why a clip was dropped, or `Speech` to proceed. Gating happens before any
/// network request: silence and misfires must cost nothing (and Groq bills a
/// 10 s minimum per request).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateVerdict {
    Speech,
    /// The hold (down to up) was shorter than `min_speech_ms`: a misfire.
    TooShort,
    /// The assembled clip's RMS is below `silence_rms`: no speech present.
    TooQuiet,
}

/// Gate on hold duration. Checked against the down->up distance (NOT the
/// assembled window, whose pre-roll + tail padding always exceeds the
/// threshold and would make the check vacuous).
pub fn gate_hold(down_abs: u64, up_abs: u64, rate: u32, params: &WindowParams) -> GateVerdict {
    let hold = up_abs.saturating_sub(down_abs);
    if hold < ms_to_samples(params.min_speech_ms, rate) {
        GateVerdict::TooShort
    } else {
        GateVerdict::Speech
    }
}

/// Gate on clip loudness, applied to the assembled window.
pub fn gate_clip(samples: &[f32], params: &WindowParams) -> GateVerdict {
    if rms(samples) < params.silence_rms {
        GateVerdict::TooQuiet
    } else {
        GateVerdict::Speech
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn params() -> WindowParams {
        WindowParams::default()
    }

    #[test]
    fn ms_conversion_is_exact_at_48k() {
        assert_eq!(ms_to_samples(300, RATE), 14_400);
        assert_eq!(ms_to_samples(150, RATE), 7_200);
        assert_eq!(ms_to_samples(0, RATE), 0);
    }

    #[test]
    fn window_extends_preroll_and_tail() {
        // Press at 100_000, release at 200_000.
        let (start, end) = window_bounds(100_000, 200_000, RATE, &params());
        assert_eq!(start, 100_000 - 14_400); // 300 ms pre-roll
        assert_eq!(end, 200_000 + 7_200); // 150 ms tail
    }

    #[test]
    fn preroll_clamps_at_absolute_zero() {
        // Press 100 ms after startup: pre-roll wants 300 ms, gets 100.
        let down = ms_to_samples(100, RATE);
        let (start, end) = window_bounds(down, down + 48_000, RATE, &params());
        assert_eq!(start, 0);
        assert_eq!(end, down + 48_000 + 7_200);
    }

    #[test]
    fn over_max_hold_keeps_most_recent_audio() {
        let p = params();
        let max_hold_samples = p.max_hold_s as u64 * RATE as u64;
        let down = 1_000_000;
        let up = down + max_hold_samples + 480_000; // held 10 s past the cap
        let (start, end) = window_bounds(down, up, RATE, &p);
        // The effective window is anchored at the release, not the press.
        assert_eq!(end, up + 7_200);
        assert_eq!(start, (up - max_hold_samples) - 14_400);
        assert!(start > down, "capped window must drop the oldest audio");
    }

    #[test]
    fn exactly_max_hold_is_not_truncated() {
        let p = params();
        let max_hold_samples = p.max_hold_s as u64 * RATE as u64;
        let down = 1_000_000;
        let up = down + max_hold_samples;
        let (start, _) = window_bounds(down, up, RATE, &p);
        assert_eq!(start, down - 14_400);
    }

    #[test]
    fn ring_capacity_holds_a_maximal_window() {
        let p = params();
        let cap = ring_capacity(RATE, &p) as u64;
        let max_hold_samples = p.max_hold_s as u64 * RATE as u64;
        let window = max_hold_samples + 14_400 + 7_200;
        assert!(cap >= window + RATE as u64, "one second of copy slack");
    }

    #[test]
    fn gate_hold_drops_misfires() {
        let p = params(); // min_speech 250 ms = 12_000 samples at 48 kHz
        assert_eq!(gate_hold(0, 11_999, RATE, &p), GateVerdict::TooShort);
        assert_eq!(gate_hold(0, 12_000, RATE, &p), GateVerdict::Speech);
        // Zero-length hold (down and up in the same instant).
        assert_eq!(gate_hold(500, 500, RATE, &p), GateVerdict::TooShort);
    }

    #[test]
    fn gate_clip_drops_silence() {
        let p = params(); // silence_rms 0.01
        let quiet = vec![0.001_f32; 48_000];
        let loud: Vec<f32> = (0..48_000)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / RATE as f32).sin() * 0.3)
            .collect();
        assert_eq!(gate_clip(&quiet, &p), GateVerdict::TooQuiet);
        assert_eq!(gate_clip(&loud, &p), GateVerdict::Speech);
    }

    #[test]
    fn gate_clip_empty_is_quiet() {
        assert_eq!(gate_clip(&[], &params()), GateVerdict::TooQuiet);
    }

    #[test]
    fn rms_of_known_signal() {
        // RMS of a full-scale square wave is 1.0.
        let square: Vec<f32> = (0..1000)
            .map(|i| if i % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        assert!((rms(&square) - 1.0).abs() < 1e-6);
        assert_eq!(rms(&[]), 0.0);
    }
}

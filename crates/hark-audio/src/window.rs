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

/// `ring_capacity` expressed in whole seconds, for callers that must size
/// the ring BEFORE the device rate is known (capture computes
/// `seconds * live_rate` once the stream config is resolved). Always covers
/// `ring_capacity` at any rate.
pub fn ring_seconds(params: &WindowParams) -> u32 {
    params.max_hold_s + (params.preroll_ms + params.tail_ms).div_ceil(1000) + 1
}

/// Root-mean-square amplitude of a clip. Empty clips are 0.0 (silent).
///
/// Accumulates in `f64`: a maximal hold is millions of samples, and an `f32`
/// accumulator loses the small squares once the running sum grows.
pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
    (sum / samples.len() as f64).sqrt() as f32
}

/// The length of the sliding window the loudness gate judges a clip by.
/// Roughly one syllable: long enough to average out a glottal pulse, short
/// enough that a single word fills it.
pub const GATE_WINDOW_MS: u32 = 100;

/// The RMS of the loudest `window_ms` window in the clip, scanning at half-window
/// hops so no burst straddles a boundary and gets split.
///
/// This is the loudness statistic the gate needs, and a whole-clip [`rms`] is
/// not. RMS is a mean, so it falls as the *proportion* of the clip that is
/// silence rises — and an assembled window is always padded with pre-roll and
/// tail, plus whatever pause the user left between pressing the chord and
/// speaking. Under a mean, a quiet "yes" and a long sentence spoken at the very
/// same level score differently, and the short one scores lower: the gate ends
/// up strictest on exactly the short commands push-to-talk exists to serve.
/// A peak window asks "did this clip ever reach speaking level", which is
/// length- and pause-independent.
///
/// Runs in 2n multiply-adds with no allocation (each sample falls in at most
/// two windows), so a maximal hold stays cheap on the release-to-inject path.
pub fn peak_window_rms(samples: &[f32], rate: u32, window_ms: u32) -> f32 {
    window_rms_extremes(samples, rate, window_ms).1
}

/// The clip's noise floor: the RMS of its *quietest* window.
///
/// Taken across the whole clip rather than just the pre-roll, because the
/// pre-roll's whole purpose is to catch words the user started before the
/// chord registered — so it is exactly the region that cannot be assumed
/// silent. The quietest window anywhere is a far safer estimate of the room.
pub fn noise_floor(samples: &[f32], rate: u32) -> f32 {
    window_rms_extremes(samples, rate, GATE_WINDOW_MS).0
}

/// `(quietest, loudest)` window RMS in one scan. See [`peak_window_rms`] for
/// why windows rather than a whole-clip mean.
fn window_rms_extremes(samples: &[f32], rate: u32, window_ms: u32) -> (f32, f32) {
    let win = ms_to_samples(window_ms, rate) as usize;
    // A clip shorter than one window has no window to slide: judge it whole.
    if win == 0 || samples.len() <= win {
        let whole = rms(samples);
        return (whole, whole);
    }
    let hop = (win / 2).max(1);
    let (mut min, mut max) = (f32::INFINITY, 0.0f32);
    let mut start = 0;
    while start + win <= samples.len() {
        let sum: f64 = samples[start..start + win]
            .iter()
            .map(|&s| (s as f64) * (s as f64))
            .sum();
        let window_rms = (sum / win as f64).sqrt() as f32;
        min = min.min(window_rms);
        max = max.max(window_rms);
        start += hop;
    }
    (min, max)
}

/// Why a clip was dropped, or `Speech` to proceed. Gating happens before any
/// network request: silence and misfires must cost nothing (and Groq bills a
/// 10 s minimum per request).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateVerdict {
    Speech,
    /// The hold (down to up) was shorter than `min_speech_ms`: a misfire.
    TooShort,
    /// No window of the assembled clip reached `silence_rms`: no speech present.
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

/// How far the loudest window must rise above the room for a clip to count as
/// speech on the quiet-microphone path: 4x amplitude, about +12 dB.
const SPEECH_OVER_ROOM: f32 = 4.0;

/// The dead-microphone backstop, about -55 dBFS. Nothing below this is speech
/// at any signal-to-noise ratio; it is the one floor the relative path cannot
/// argue its way under.
const DEAD_MIC_RMS: f32 = 0.0018;

/// Gate on clip loudness, applied to the assembled window. A clip is speech if
/// **either** its loudest [`GATE_WINDOW_MS`] window reaches `silence_rms`, or
/// that window stands clearly above the clip's own noise floor.
///
/// The second path is what rescues a quiet microphone. `silence_rms` is an
/// absolute level, so on its own it encodes an assumption about how hot the
/// user's hardware is — and users whose gear sits below that assumption
/// experience the app simply not responding. Comparing against the room
/// instead asks the question that actually matters: did this person speak?
///
/// The two tests are OR'd, never AND'd, so this can only ever admit clips the
/// absolute threshold would have dropped. Deliberately biased toward passing:
/// a false pass costs one transcription request, while a false drop is the app
/// silently doing nothing, which the user cannot diagnose. The cheap failure is
/// the one to prefer.
pub fn gate_clip(samples: &[f32], rate: u32, params: &WindowParams) -> GateVerdict {
    let (floor, peak) = window_rms_extremes(samples, rate, GATE_WINDOW_MS);
    let loud_enough = peak >= params.silence_rms;
    let above_the_room = peak >= DEAD_MIC_RMS && peak >= floor * SPEECH_OVER_ROOM;
    if loud_enough || above_the_room {
        GateVerdict::Speech
    } else {
        GateVerdict::TooQuiet
    }
}

/// The loudness the gate measured, for logging beside a verdict. Callers log
/// this so a user report carries the number that decided the outcome.
pub fn clip_loudness(samples: &[f32], rate: u32) -> f32 {
    peak_window_rms(samples, rate, GATE_WINDOW_MS)
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
    fn ring_seconds_covers_ring_capacity_at_any_rate() {
        let p = params();
        for rate in [16_000_u32, 44_100, 48_000, 96_000] {
            let by_seconds = window_seconds_to_capacity(rate, ring_seconds(&p));
            assert!(
                by_seconds >= ring_capacity(rate, &p),
                "ring_seconds under-sizes the ring at {rate} Hz"
            );
        }
    }

    fn window_seconds_to_capacity(rate: u32, seconds: u32) -> usize {
        seconds as usize * rate as usize
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
        assert_eq!(gate_clip(&quiet, RATE, &p), GateVerdict::TooQuiet);
        assert_eq!(gate_clip(&tone(48_000, 0.3), RATE, &p), GateVerdict::Speech);
    }

    #[test]
    fn gate_clip_empty_is_quiet() {
        assert_eq!(gate_clip(&[], RATE, &params()), GateVerdict::TooQuiet);
    }

    /// A 440 Hz tone of `len` samples at `amp` (RMS = amp / sqrt(2)).
    fn tone(len: usize, amp: f32) -> Vec<f32> {
        (0..len)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / RATE as f32).sin() * amp)
            .collect()
    }

    /// `speech_ms` of tone embedded in a `total_ms` clip of digital silence,
    /// the shape `assemble_window` produces: pre-roll, then speech, then tail.
    fn padded(speech_ms: u32, total_ms: u32, amp: f32) -> Vec<f32> {
        let speech = ms_to_samples(speech_ms, RATE) as usize;
        let total = ms_to_samples(total_ms, RATE) as usize;
        let lead = (total - speech) / 2;
        let mut clip = vec![0.0f32; lead];
        clip.extend(tone(speech, amp));
        clip.resize(total, 0.0);
        clip
    }

    /// The regression test for the sensitivity bug: speech at ONE level must
    /// get ONE verdict, however much silence surrounds it. Under the old
    /// whole-clip mean, the 250 ms utterance scored ~0.54x the 3 s one purely
    /// because of padding, so quiet short commands were dropped while identical
    /// long sentences passed.
    #[test]
    fn gate_verdict_is_independent_of_utterance_length() {
        let p = params();
        // Quiet speech: 0.02 amplitude, RMS 0.0141 -- above the threshold, but
        // low enough that padding dilution would sink the short clip.
        let short = padded(250, 850, 0.02); // "yes"
        let long = padded(3000, 3450, 0.02); // a sentence
        assert_eq!(gate_clip(&short, RATE, &p), GateVerdict::Speech);
        assert_eq!(gate_clip(&long, RATE, &p), GateVerdict::Speech);

        // The measured loudness is the same for both, not merely the verdict.
        let (a, b) = (clip_loudness(&short, RATE), clip_loudness(&long, RATE));
        assert!(
            (a - b).abs() < 1e-3,
            "loudness differs by length: {a} vs {b}"
        );

        // And the old statistic really would have split them.
        assert!(
            rms(&short) < p.silence_rms && rms(&long) > p.silence_rms,
            "this test no longer reproduces the original defect"
        );
    }

    /// Pausing after pressing the chord must not make the gate stricter.
    #[test]
    fn a_lead_in_pause_does_not_gate_speech() {
        let p = params();
        let mut clip = vec![0.0f32; ms_to_samples(700, RATE) as usize];
        clip.extend(tone(ms_to_samples(300, RATE) as usize, 0.02));
        clip.extend(vec![0.0f32; ms_to_samples(150, RATE) as usize]);
        assert_eq!(gate_clip(&clip, RATE, &p), GateVerdict::Speech);
    }

    #[test]
    fn peak_window_finds_a_burst_a_mean_would_bury() {
        // One 100 ms burst in 5 s of silence.
        let clip = padded(100, 5000, 0.5);
        let peak = peak_window_rms(&clip, RATE, GATE_WINDOW_MS);
        assert!(peak > 0.3, "peak window should see the burst, got {peak}");
        assert!(rms(&clip) < 0.06, "and a mean should not");
    }

    #[test]
    fn peak_window_of_a_steady_tone_matches_its_rms() {
        let t = tone(48_000, 0.3);
        let expected = 0.3 / 2.0_f32.sqrt();
        assert!((peak_window_rms(&t, RATE, GATE_WINDOW_MS) - expected).abs() < 1e-3);
    }

    #[test]
    fn peak_window_falls_back_to_whole_clip_when_shorter_than_a_window() {
        // 50 ms clip, 100 ms window: nothing to slide.
        let t = tone(ms_to_samples(50, RATE) as usize, 0.3);
        assert!((peak_window_rms(&t, RATE, GATE_WINDOW_MS) - rms(&t)).abs() < 1e-6);
        assert_eq!(peak_window_rms(&[], RATE, GATE_WINDOW_MS), 0.0);
    }

    #[test]
    fn room_tone_alone_is_still_gated() {
        // Steady -54 dBFS hiss across a 2 s clip: no window reaches speech.
        let p = params();
        let hiss: Vec<f32> = (0..2 * RATE as usize)
            .map(|i| if i % 2 == 0 { 0.002 } else { -0.002 })
            .collect();
        assert_eq!(gate_clip(&hiss, RATE, &p), GateVerdict::TooQuiet);
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

//! Per-utterance input normalization: one static gain factor per clip.
//!
//! Push-to-talk hands us the whole utterance at once, so there is no reason to
//! run a streaming AGC or a compressor. A single factor computed from the whole
//! clip and applied uniformly cannot pump, cannot breathe, is deterministic for
//! a given input, and is trivially testable — all things a time-varying gain
//! would cost us for no benefit here.
//!
//! Boost-only by design. Users whose levels already work must see byte-identical
//! audio; the whole point is to lift quiet microphones up to what the
//! transcription provider expects, not to relevel everyone.

use crate::window::{noise_floor, peak_window_rms, GATE_WINDOW_MS};

/// Target for the clip's loudest window, about -20 dBFS. A common level for
/// speech handed to a recognizer: well clear of the noise floor, with plenty of
/// headroom left for peaks.
const TARGET_WINDOW_RMS: f32 = 0.1;

/// No sample may be pushed past about -1 dBFS. Clipping destroys exactly the
/// consonant transients a recognizer needs, so a quiet clip that would have to
/// clip to reach the target simply stops short of it.
const PEAK_CEILING: f32 = 0.89;

/// Nor may the room be amplified past about -34 dBFS. Without this, a clip
/// recorded in near-silence gets an enormous gain and arrives as a wall of
/// hiss, which recognizers happily hallucinate words onto — a worse and much
/// harder-to-diagnose failure than the quiet audio we set out to fix.
const NOISE_CEILING: f32 = 0.02;

/// Hard cap, about +26 dB, in case every other limit is somehow permissive.
const MAX_GAIN: f32 = 20.0;

/// The largest absolute sample in the clip.
pub fn true_peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, &s| m.max(s.abs()))
}

/// The gain to apply to a clip: enough to bring its loudest window toward
/// [`TARGET_WINDOW_RMS`], less whatever the clipping, noise, and hard-cap
/// limits allow. Never below 1.0 — see the module note on boost-only.
pub fn normalization_gain(peak_window: f32, true_peak: f32, floor: f32) -> f32 {
    if peak_window <= 0.0 {
        return 1.0;
    }
    let wanted = TARGET_WINDOW_RMS / peak_window;
    let clip_limit = if true_peak > 0.0 {
        PEAK_CEILING / true_peak
    } else {
        MAX_GAIN
    };
    // Only trust the floor as *noise* when it sits clearly below the signal.
    // In a uniform clip the quietest window IS the speech, and treating it as
    // room tone would cap the gain at nothing — the exact clip that most needs
    // lifting would be left alone. The loudness gate runs before this and has
    // already ruled such a clip speech, so there is no floor to protect.
    let floor_is_distinguishable = floor > 0.0 && floor * 2.0 < peak_window;
    let noise_limit = if floor_is_distinguishable {
        NOISE_CEILING / floor
    } else {
        MAX_GAIN
    };
    // The tightest limit wins, but never drags the result below unity: a clip
    // already at or above target is left alone rather than attenuated.
    let ceiling = clip_limit.min(noise_limit).clamp(1.0, MAX_GAIN);
    wanted.clamp(1.0, ceiling)
}

/// Normalize a clip in place and return the gain applied (1.0 when the clip
/// already sits at or above target, in which case the samples are untouched).
pub fn normalize(samples: &mut [f32], rate: u32) -> f32 {
    let peak_window = peak_window_rms(samples, rate, GATE_WINDOW_MS);
    let gain = normalization_gain(peak_window, true_peak(samples), noise_floor(samples, rate));
    if gain > 1.0 {
        for s in samples.iter_mut() {
            *s *= gain;
        }
    }
    gain
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    /// A 440 Hz tone at `amp` (window RMS = amp / sqrt(2)).
    fn tone(ms: u32, amp: f32) -> Vec<f32> {
        let len = (ms as u64 * RATE as u64 / 1000) as usize;
        (0..len)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / RATE as f32).sin() * amp)
            .collect()
    }

    #[test]
    fn a_quiet_clip_is_lifted_toward_target() {
        let mut clip = tone(1000, 0.02);
        let gain = normalize(&mut clip, RATE);
        assert!(gain > 1.0, "quiet clip should be boosted, got {gain}");
        let after = peak_window_rms(&clip, RATE, GATE_WINDOW_MS);
        assert!(
            (after - TARGET_WINDOW_RMS).abs() < 0.02,
            "should land near target, got {after}"
        );
    }

    #[test]
    fn an_already_loud_clip_is_left_byte_identical() {
        let original = tone(1000, 0.5);
        let mut clip = original.clone();
        let gain = normalize(&mut clip, RATE);
        assert_eq!(gain, 1.0);
        assert_eq!(
            clip, original,
            "audio that already works must not be touched"
        );
    }

    #[test]
    fn gain_never_pushes_samples_into_clipping() {
        // A clip whose window RMS is low but which carries one near-full-scale
        // transient: chasing the RMS target would clip it.
        let mut clip = tone(1000, 0.01);
        clip[500] = 0.95;
        normalize(&mut clip, RATE);
        assert!(
            true_peak(&clip) <= 1.0,
            "normalization must not clip: {}",
            true_peak(&clip)
        );
    }

    /// The realistic shape: quiet speech padded with pre-roll and tail room
    /// tone. The floor is genuinely below the signal here, so the noise ceiling
    /// governs and stops the room being lifted with the voice.
    #[test]
    fn a_loud_room_caps_the_boost_on_a_padded_clip() {
        let mut clip = vec![0.01f32; (0.3 * RATE as f32) as usize]; // room tone
        clip.extend(tone(600, 0.05)); // quiet speech
        clip.extend(vec![0.01f32; (0.15 * RATE as f32) as usize]);
        let floor_before = crate::window::noise_floor(&clip, RATE);
        let gain = normalize(&mut clip, RATE);
        assert!(
            floor_before * gain <= NOISE_CEILING * 1.05,
            "room lifted to {}, ceiling {NOISE_CEILING}",
            floor_before * gain
        );
    }

    #[test]
    fn a_near_silent_room_is_not_amplified_into_hiss() {
        // Steady very quiet hiss, no speech: the noise ceiling must cap the
        // gain long before the RMS target is reached.
        let mut clip: Vec<f32> = (0..RATE as usize)
            .map(|i| if i % 2 == 0 { 0.0008 } else { -0.0008 })
            .collect();
        normalize(&mut clip, RATE);
        let after = peak_window_rms(&clip, RATE, GATE_WINDOW_MS);
        assert!(after <= NOISE_CEILING * 1.05, "hiss amplified to {after}");
    }

    #[test]
    fn silence_gets_unity_gain() {
        let mut clip = vec![0.0f32; RATE as usize];
        assert_eq!(normalize(&mut clip, RATE), 1.0);
        assert!(clip.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn gain_is_never_below_unity() {
        // A hot clip: the target is below where it already sits.
        assert_eq!(normalization_gain(0.5, 0.9, 0.001), 1.0);
    }

    #[test]
    fn an_empty_clip_is_handled() {
        let mut clip: Vec<f32> = Vec::new();
        assert_eq!(normalize(&mut clip, RATE), 1.0);
    }
}

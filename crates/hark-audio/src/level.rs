//! A lock-free input-level meter for UI feedback (the recording overlay's
//! audio-reactive pulse).
//!
//! The meter is written from the cpal input callback and read from the UI
//! thread. Like the ring [`Producer`](crate::ring::Producer), the write path
//! does only arithmetic plus one relaxed atomic store: no allocation, no
//! locks, no syscalls (cpal #970). It is deliberately advisory and lossy:
//! a dropped or stale sample block only nudges a cosmetic animation, never
//! the dictation audio (that path is the ring buffer, untouched).
//!
//! Smoothing lives on the reader side (the overlay eases the displayed value
//! toward this raw peak), so the callback stays as cheap as possible and the
//! meter carries no time-dependent state.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

/// A single-value peak meter shared between the audio callback and the UI.
/// Stores the most recent block's peak absolute amplitude (0.0 ..= 1.0) as an
/// `f32` bit pattern.
#[derive(Debug, Default)]
pub struct LevelMeter {
    /// Peak absolute amplitude of the most recent observed block, as
    /// `f32::to_bits`. Relaxed on both ends: this is advisory UI data, never
    /// ordered against the ring's audio.
    peak: AtomicU32,
}

impl LevelMeter {
    /// A new meter, reading zero until the first [`observe`](Self::observe).
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Record the peak absolute amplitude of one callback block. Safe on the
    /// cpal hot path: a bounded scan plus one relaxed store, nothing else.
    /// Channel layout is irrelevant to a level reading, so interleaved frames
    /// are scanned as-is (the loudest channel wins, which is what a level
    /// indicator should show).
    pub fn observe(&self, samples: &[f32]) {
        let mut peak = 0.0f32;
        for &s in samples {
            let a = s.abs();
            if a > peak {
                peak = a;
            }
        }
        // Clamp: a misbehaving device can hand back out-of-range floats, and
        // the reader treats this as a 0..=1 fraction.
        self.peak.store(peak.min(1.0).to_bits(), Ordering::Relaxed);
    }

    /// The most recent block's peak amplitude (0.0 ..= 1.0). Zero before the
    /// first block, and after capture goes quiet the last block simply lingers
    /// until the next one overwrites it (the reader's easing hides the step).
    pub fn level(&self) -> f32 {
        f32::from_bits(self.peak.load(Ordering::Relaxed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_at_zero() {
        assert_eq!(LevelMeter::new().level(), 0.0);
    }

    #[test]
    fn reports_the_block_peak_absolute_amplitude() {
        let m = LevelMeter::new();
        m.observe(&[0.1, -0.6, 0.3, 0.2]);
        assert!((m.level() - 0.6).abs() < 1e-6);
    }

    #[test]
    fn a_later_quieter_block_replaces_the_peak() {
        let m = LevelMeter::new();
        m.observe(&[0.9, -0.9]);
        assert!((m.level() - 0.9).abs() < 1e-6);
        m.observe(&[0.05, -0.02]);
        assert!((m.level() - 0.05).abs() < 1e-6);
    }

    #[test]
    fn out_of_range_samples_are_clamped() {
        let m = LevelMeter::new();
        m.observe(&[3.0, -2.0]);
        assert_eq!(m.level(), 1.0);
    }

    #[test]
    fn an_empty_block_reads_zero() {
        let m = LevelMeter::new();
        m.observe(&[]);
        assert_eq!(m.level(), 0.0);
    }
}

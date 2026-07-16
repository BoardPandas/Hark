//! Lock-free SPSC ring buffer with an absolute sample counter.
//!
//! The producer is the cpal input callback: `push*` performs only relaxed
//! atomic stores plus one release store of the counter. No allocation, no
//! locks, no syscalls (cpal #970: blocking or allocating in the callback can
//! silently stop the stream). Samples are stored as `AtomicU32` bit patterns
//! so concurrent overwrite during a consumer copy is a detected stale read,
//! never undefined behavior.
//!
//! The absolute counter (total samples ever written) lets any subsystem ask
//! for "samples [start_abs .. end_abs)" regardless of wrapping. The consumer
//! detects both "not produced yet" and "already overwritten".

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RangeError {
    #[error("range start is after range end")]
    Backwards,
    #[error("samples not yet produced (requested end {requested_end}, written {written})")]
    NotYetProduced { requested_end: u64, written: u64 },
    #[error("samples already overwritten (requested start {requested_start}, oldest {oldest})")]
    Overwritten { requested_start: u64, oldest: u64 },
}

struct Ring {
    data: Box<[AtomicU32]>,
    /// Total samples ever pushed. The ring holds [written - capacity, written).
    written: AtomicU64,
}

/// Create a ring holding `capacity` mono f32 samples. All storage is
/// allocated up front; the producer never allocates.
pub fn ring(capacity: usize) -> (Producer, Consumer) {
    assert!(capacity > 0, "ring capacity must be non-zero");
    let data: Box<[AtomicU32]> = (0..capacity).map(|_| AtomicU32::new(0)).collect();
    let ring = Arc::new(Ring {
        data,
        written: AtomicU64::new(0),
    });
    (Producer { ring: ring.clone() }, Consumer { ring })
}

/// Write half. Exactly one thread (the audio callback) may hold this; it is
/// deliberately neither `Clone` nor constructible elsewhere.
pub struct Producer {
    ring: Arc<Ring>,
}

impl Producer {
    /// Push mono samples. Allocation-free and lock-free.
    pub fn push(&self, samples: &[f32]) {
        let cap = self.ring.data.len() as u64;
        let start = self.ring.written.load(Ordering::Relaxed);
        for (i, &s) in samples.iter().enumerate() {
            let idx = ((start + i as u64) % cap) as usize;
            self.ring.data[idx].store(s.to_bits(), Ordering::Relaxed);
        }
        self.ring
            .written
            .store(start + samples.len() as u64, Ordering::Release);
    }

    /// Downmix interleaved multi-channel frames to mono by averaging, then
    /// push. Allocation-free: averaging happens frame-by-frame on the way in.
    /// A trailing partial frame (malformed input) is dropped.
    pub fn push_interleaved(&self, interleaved: &[f32], channels: usize) {
        assert!(channels > 0, "channel count must be non-zero");
        if channels == 1 {
            return self.push(interleaved);
        }
        let cap = self.ring.data.len() as u64;
        let start = self.ring.written.load(Ordering::Relaxed);
        let mut frames = 0u64;
        for frame in interleaved.chunks_exact(channels) {
            let avg = frame.iter().sum::<f32>() / channels as f32;
            let idx = ((start + frames) % cap) as usize;
            self.ring.data[idx].store(avg.to_bits(), Ordering::Relaxed);
            frames += 1;
        }
        self.ring.written.store(start + frames, Ordering::Release);
    }
}

/// Read half. Exactly one thread (the pipeline worker) may hold this.
pub struct Consumer {
    ring: Arc<Ring>,
}

impl Consumer {
    /// Total samples ever produced (the absolute counter).
    pub fn total_written(&self) -> u64 {
        self.ring.written.load(Ordering::Acquire)
    }

    /// The oldest absolute index still guaranteed present.
    pub fn oldest_available(&self) -> u64 {
        let written = self.total_written();
        written.saturating_sub(self.ring.data.len() as u64)
    }

    /// Copy samples [start_abs, end_abs). Fails cleanly when the range is not
    /// yet produced, already overwritten, or overwritten mid-copy (the
    /// producer lapped us while we read; detected by re-checking the counter).
    pub fn read_range(&self, start_abs: u64, end_abs: u64) -> Result<Vec<f32>, RangeError> {
        if start_abs > end_abs {
            return Err(RangeError::Backwards);
        }
        let cap = self.ring.data.len() as u64;
        let written = self.total_written();
        if end_abs > written {
            return Err(RangeError::NotYetProduced {
                requested_end: end_abs,
                written,
            });
        }
        if start_abs < written.saturating_sub(cap) {
            return Err(RangeError::Overwritten {
                requested_start: start_abs,
                oldest: written - cap,
            });
        }
        let mut out = Vec::with_capacity((end_abs - start_abs) as usize);
        for abs in start_abs..end_abs {
            let idx = (abs % cap) as usize;
            out.push(f32::from_bits(self.ring.data[idx].load(Ordering::Relaxed)));
        }
        // The producer may have wrapped past our start while we copied; the
        // copy would then contain a mix of old and new audio. Detect and fail.
        let written_after = self.total_written();
        if start_abs < written_after.saturating_sub(cap) {
            return Err(RangeError::Overwritten {
                requested_start: start_abs,
                oldest: written_after - cap,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fill helper: absolute index i stores the value i as f32 so reads are
    /// verifiable by position.
    fn indexed(n: u64) -> Vec<f32> {
        (0..n).map(|i| i as f32).collect()
    }

    #[test]
    fn read_back_before_wrap() {
        let (p, c) = ring(64);
        p.push(&indexed(10));
        let got = c.read_range(2, 7).unwrap();
        assert_eq!(got, vec![2.0, 3.0, 4.0, 5.0, 6.0]);
        assert_eq!(c.total_written(), 10);
        assert_eq!(c.oldest_available(), 0);
    }

    #[test]
    fn read_across_wrap_boundary() {
        let (p, c) = ring(16);
        p.push(&indexed(40)); // wraps 2.5 times; ring holds [24, 40)
        assert_eq!(c.total_written(), 40);
        assert_eq!(c.oldest_available(), 24);
        // A range straddling the physical wrap point must read logically.
        let got = c.read_range(30, 36).unwrap();
        assert_eq!(got, vec![30.0, 31.0, 32.0, 33.0, 34.0, 35.0]);
    }

    #[test]
    fn full_capacity_read_is_available() {
        let (p, c) = ring(8);
        p.push(&indexed(8));
        let got = c.read_range(0, 8).unwrap();
        assert_eq!(got, indexed(8));
    }

    #[test]
    fn not_yet_produced_is_detected() {
        let (p, c) = ring(16);
        p.push(&indexed(5));
        let err = c.read_range(0, 6).unwrap_err();
        assert_eq!(
            err,
            RangeError::NotYetProduced {
                requested_end: 6,
                written: 5
            }
        );
    }

    #[test]
    fn overwritten_is_detected() {
        let (p, c) = ring(8);
        p.push(&indexed(20)); // oldest available is 12
        let err = c.read_range(11, 15).unwrap_err();
        assert_eq!(
            err,
            RangeError::Overwritten {
                requested_start: 11,
                oldest: 12
            }
        );
    }

    #[test]
    fn backwards_range_is_rejected() {
        let (p, c) = ring(8);
        p.push(&indexed(4));
        assert_eq!(c.read_range(3, 2).unwrap_err(), RangeError::Backwards);
    }

    #[test]
    fn empty_range_is_empty() {
        let (p, c) = ring(8);
        p.push(&indexed(4));
        assert_eq!(c.read_range(2, 2).unwrap(), Vec::<f32>::new());
    }

    #[test]
    fn interleaved_stereo_downmixes_by_averaging() {
        let (p, c) = ring(8);
        // Frames: (0.2, 0.4) -> 0.3; (1.0, -1.0) -> 0.0; (0.5, 0.5) -> 0.5.
        p.push_interleaved(&[0.2, 0.4, 1.0, -1.0, 0.5, 0.5], 2);
        assert_eq!(c.total_written(), 3);
        let got = c.read_range(0, 3).unwrap();
        assert!((got[0] - 0.3).abs() < 1e-6);
        assert!(got[1].abs() < 1e-6);
        assert!((got[2] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn interleaved_trailing_partial_frame_is_dropped() {
        let (p, c) = ring(8);
        p.push_interleaved(&[0.1, 0.1, 0.2], 2); // one full frame + a stray
        assert_eq!(c.total_written(), 1);
    }

    #[test]
    fn mono_interleave_is_plain_push() {
        let (p, c) = ring(8);
        p.push_interleaved(&[1.0, 2.0, 3.0], 1);
        assert_eq!(c.read_range(0, 3).unwrap(), vec![1.0, 2.0, 3.0]);
    }
}

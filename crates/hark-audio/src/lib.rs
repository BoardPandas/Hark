//! Hark audio capture: cpal input stream into a lock-free ring buffer,
//! device-rate -> 16 kHz mono resampling, pre-roll/tail window assembly,
//! and the silence gate.
//!
//! Populated in Phase 1 checkpoint 2. The cpal callback must never allocate
//! or block (cpal #970: doing so can silently stop the stream).

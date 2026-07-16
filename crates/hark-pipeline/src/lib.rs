//! Hark dictation pipeline: the worker-thread state machine that glues
//! audio capture, push-to-talk edges, cloud STT, and injection.
//!
//! Populated in Phase 1 checkpoint 5. Owns the one long-lived pre-warmed
//! HTTP client; at most one retry per utterance, on timeout/connect only.

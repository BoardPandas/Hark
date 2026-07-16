//! Hark BYOK key resolution: env override (`HARK_STT_KEY`) first, then the
//! OS keychain via `keyring`. No type in this crate may ever Debug/Display
//! the key material.
//!
//! Populated in Phase 1 checkpoint 1.

//! Hark push-to-talk source. A low-level key hook on a dedicated
//! message-loop thread emits clean key-down/key-up edges for the configured
//! chord (default: Left Ctrl + Left Win held together).
//!
//! Populated in Phase 1 checkpoint 3 (Windows) and checkpoint 7 (macOS).
//! The hook thread's entire body must be a GetMessage/DispatchMessage loop,
//! and our own injected Ctrl+V (LLKHF_INJECTED) must never re-trigger PTT.

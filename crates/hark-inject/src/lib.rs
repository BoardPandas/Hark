//! Hark text injection: clipboard stash -> set -> verify -> Ctrl+V -> restore,
//! with an enigo char-typing fallback for paste-hostile fields.
//!
//! Populated in Phase 1 checkpoint 4. The clipboard is a global object
//! (bounded retry on ClipboardOccupied) and set->paste->restore is a race
//! (tunable delays, read-back verify).

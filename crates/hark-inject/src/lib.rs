//! Hark text injection: clipboard stash -> set -> verify -> Ctrl+V ->
//! restore, with an enigo char-typing fallback for paste-hostile fields.
//!
//! Strategy selection and the fallback decision are pure logic (tested);
//! the clipboard sequence and key synthesis are I/O glue verifiable only on
//! real Windows/macOS (run-on-real-HW).

mod clipboard;
mod keys;

pub use clipboard::ClipboardError;

use thiserror::Error;

/// How transcribed text reaches the cursor. Mirrors `hark-config`'s enum
/// without depending on that crate (the pipeline maps between them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Strategy {
    /// Clipboard paste with typing fallback (fast, default).
    #[default]
    Clipboard,
    /// Character typing only (paste-hostile fields; never touches the
    /// clipboard).
    Type,
}

/// Injection knobs. Delay values have no OS-guaranteed minimum; the defaults
/// are the community rule of thumb, tuned on real hardware (spec §8).
#[derive(Debug, Clone)]
pub struct InjectSettings {
    pub strategy: Strategy,
    pub set_paste_delay_ms: u64,
    pub paste_restore_delay_ms: u64,
    pub clipboard_retries: u32,
}

impl Default for InjectSettings {
    fn default() -> Self {
        InjectSettings {
            strategy: Strategy::Clipboard,
            set_paste_delay_ms: 50,
            paste_restore_delay_ms: 50,
            clipboard_retries: 8,
        }
    }
}

#[derive(Debug, Error)]
pub enum InjectError {
    #[error("clipboard injection failed: {0}")]
    Clipboard(ClipboardError),
    #[error("typing injection failed: {0}")]
    Typing(String),
}

/// The plan for one injection: which path to try first, and whether typing
/// is the fallback. Pure so strategy selection is unit-testable.
#[derive(Debug, PartialEq, Eq)]
enum Plan {
    ClipboardThenType,
    TypeOnly,
}

fn plan_for(strategy: Strategy) -> Plan {
    match strategy {
        Strategy::Clipboard => Plan::ClipboardThenType,
        Strategy::Type => Plan::TypeOnly,
    }
}

/// Inject `text` at the cursor of the foreground app. Empty text is a no-op
/// (an empty transcript must not clobber the clipboard or type anything).
pub fn inject(text: &str, settings: &InjectSettings) -> Result<(), InjectError> {
    if text.is_empty() {
        return Ok(());
    }
    match plan_for(settings.strategy) {
        Plan::TypeOnly => keys::type_text(text).map_err(InjectError::Typing),
        Plan::ClipboardThenType => match clipboard::paste_via_clipboard(text, settings) {
            Ok(()) => Ok(()),
            Err(e) if e.should_fallback_to_typing() => {
                log::warn!("clipboard paste failed ({e}); falling back to char typing");
                keys::type_text(text).map_err(InjectError::Typing)
            }
            Err(e) => Err(InjectError::Clipboard(e)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strategy_maps_to_plan() {
        assert_eq!(plan_for(Strategy::Clipboard), Plan::ClipboardThenType);
        assert_eq!(plan_for(Strategy::Type), Plan::TypeOnly);
    }

    #[test]
    fn default_strategy_is_clipboard() {
        assert_eq!(Strategy::default(), Strategy::Clipboard);
        let s = InjectSettings::default();
        assert_eq!(s.strategy, Strategy::Clipboard);
        assert_eq!(s.set_paste_delay_ms, 50);
        assert_eq!(s.paste_restore_delay_ms, 50);
        assert_eq!(s.clipboard_retries, 8);
    }

    #[test]
    fn empty_text_is_a_noop() {
        // Must not touch the clipboard or synthesize keys: succeeds even on
        // a machine with no input synthesis available (this test runner).
        inject("", &InjectSettings::default()).expect("empty inject is Ok");
    }
}

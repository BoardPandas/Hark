//! Clipboard paste path: stash -> set -> verify -> Ctrl+V -> restore.
//!
//! The clipboard is a global object: any open can fail with "occupied" while
//! another process holds it, so every operation runs in a bounded retry
//! loop. set -> paste -> restore is a race with no OS-guaranteed timing
//! (pasting immediately after set can paste the OLD content): the tunable
//! delays plus a read-back verify mitigate it; tune on real hardware.
//!
//! Accepted v1 limitation (documented, spec §12): arboard round-trips TEXT
//! only. `set_text` clears all other formats, so an image/RTF/HTML clipboard
//! present before dictation is not preserved. Full fidelity would need
//! per-format EnumClipboardFormats handling; revisit only if it hurts.

use crate::InjectSettings;
use std::time::Duration;
use thiserror::Error;

/// Clipboard-path failures, pre-classified so the caller's fallback decision
/// is pure logic (tested) rather than string matching.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ClipboardError {
    #[error("clipboard busy: another process held it through {attempts} attempts")]
    Busy { attempts: u32 },
    #[error("clipboard set did not take (read-back verify mismatched)")]
    VerifyMismatch,
    #[error("clipboard backend error: {0}")]
    Backend(String),
    #[error("paste synthesis failed: {0}")]
    Paste(String),
}

impl ClipboardError {
    /// Whether char-typing is a sensible fallback. True for every
    /// clipboard-side failure (typing does not touch the clipboard); false
    /// when key synthesis itself failed, because typing rides the same
    /// synthesis machinery and would fail the same way.
    pub(crate) fn should_fallback_to_typing(&self) -> bool {
        !matches!(self, ClipboardError::Paste(_))
    }
}

/// Run `op` up to `1 + retries` times, sleeping `spacing` between attempts,
/// retrying only while `retryable` says the error is transient.
/// Generic so the policy is unit-testable with closures, no clipboard needed.
pub(crate) fn with_retries<T, E>(
    retries: u32,
    spacing: Duration,
    mut op: impl FnMut() -> Result<T, E>,
    retryable: impl Fn(&E) -> bool,
) -> Result<T, (E, u32)> {
    let mut attempts = 0;
    loop {
        attempts += 1;
        match op() {
            Ok(v) => return Ok(v),
            Err(e) => {
                if attempts > retries || !retryable(&e) {
                    return Err((e, attempts));
                }
                std::thread::sleep(spacing);
            }
        }
    }
}

/// Spacing between clipboard-occupied retries. Short: the holder is usually
/// another app finishing its own copy, gone within milliseconds.
const RETRY_SPACING: Duration = Duration::from_millis(15);

fn is_occupied(e: &arboard::Error) -> bool {
    matches!(e, arboard::Error::ClipboardOccupied)
}

/// The full clipboard paste sequence. I/O glue over arboard + enigo:
/// verifiable only on real Windows/macOS (run-on-real-HW).
pub(crate) fn paste_via_clipboard(
    text: &str,
    settings: &InjectSettings,
) -> Result<(), ClipboardError> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|e| ClipboardError::Backend(format!("cannot open clipboard: {e}")))?;

    // 1. Stash the current TEXT content. "No text available" (image-only or
    //    empty clipboard) stashes None; that content is lost on restore, the
    //    accepted v1 clobber limitation.
    let stashed: Option<String> = clipboard.get_text().ok();

    // 2. Set our text, retrying while the clipboard is occupied.
    with_retries(
        settings.clipboard_retries,
        RETRY_SPACING,
        || clipboard.set_text(text.to_string()),
        is_occupied,
    )
    .map_err(|(e, attempts)| {
        if is_occupied(&e) {
            ClipboardError::Busy { attempts }
        } else {
            ClipboardError::Backend(e.to_string())
        }
    })?;

    // 3. Read-back verify: if the set did not take (clipboard managers and
    //    sync tools can interfere), pasting would inject stale content.
    let now = clipboard.get_text().ok();
    if now.as_deref() != Some(text) {
        return Err(ClipboardError::VerifyMismatch);
    }

    // 4. Let the set settle before pasting (no OS-guaranteed timing).
    std::thread::sleep(Duration::from_millis(settings.set_paste_delay_ms));

    // 5. Synthesize the paste chord.
    crate::keys::send_paste().map_err(ClipboardError::Paste)?;

    // 6. Let the foreground app read the clipboard before we restore.
    std::thread::sleep(Duration::from_millis(settings.paste_restore_delay_ms));

    // 7. Restore the stash. The dictation text IS already pasted at this
    //    point: restore failure is a warning, not a failed dictation.
    if let Some(old) = stashed {
        let restore = with_retries(
            settings.clipboard_retries,
            RETRY_SPACING,
            || clipboard.set_text(old.clone()),
            is_occupied,
        );
        if let Err((e, attempts)) = restore {
            log::warn!("could not restore clipboard after {attempts} attempt(s): {e}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[derive(Debug, PartialEq)]
    enum FakeErr {
        Transient,
        Fatal,
    }

    fn no_sleep_spacing() -> Duration {
        Duration::from_millis(0)
    }

    #[test]
    fn retries_transient_errors_up_to_budget() {
        let calls = Cell::new(0);
        let result: Result<(), (FakeErr, u32)> = with_retries(
            3,
            no_sleep_spacing(),
            || {
                calls.set(calls.get() + 1);
                Err(FakeErr::Transient)
            },
            |e| *e == FakeErr::Transient,
        );
        let (err, attempts) = result.unwrap_err();
        assert_eq!(err, FakeErr::Transient);
        assert_eq!(attempts, 4, "1 initial + 3 retries");
        assert_eq!(calls.get(), 4);
    }

    #[test]
    fn succeeds_mid_retry() {
        let calls = Cell::new(0);
        let result = with_retries(
            5,
            no_sleep_spacing(),
            || {
                calls.set(calls.get() + 1);
                if calls.get() < 3 {
                    Err(FakeErr::Transient)
                } else {
                    Ok(42)
                }
            },
            |e| *e == FakeErr::Transient,
        );
        assert_eq!(result.unwrap(), 42);
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn fatal_errors_do_not_retry() {
        let calls = Cell::new(0);
        let result: Result<(), (FakeErr, u32)> = with_retries(
            5,
            no_sleep_spacing(),
            || {
                calls.set(calls.get() + 1);
                Err(FakeErr::Fatal)
            },
            |e| *e == FakeErr::Transient,
        );
        let (err, attempts) = result.unwrap_err();
        assert_eq!(err, FakeErr::Fatal);
        assert_eq!(attempts, 1, "fatal error must fail immediately");
    }

    #[test]
    fn zero_retries_means_one_attempt() {
        let calls = Cell::new(0);
        let _: Result<(), _> = with_retries(
            0,
            no_sleep_spacing(),
            || {
                calls.set(calls.get() + 1);
                Err::<(), _>(FakeErr::Transient)
            },
            |e| *e == FakeErr::Transient,
        );
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn fallback_decision_per_error_kind() {
        assert!(ClipboardError::Busy { attempts: 9 }.should_fallback_to_typing());
        assert!(ClipboardError::VerifyMismatch.should_fallback_to_typing());
        assert!(ClipboardError::Backend("x".into()).should_fallback_to_typing());
        // Key synthesis broke: typing rides the same machinery, no fallback.
        assert!(!ClipboardError::Paste("x".into()).should_fallback_to_typing());
    }
}

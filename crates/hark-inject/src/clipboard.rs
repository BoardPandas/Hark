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

/// The clipboard transaction can be exercised without owning the desktop's
/// real clipboard or synthesizing input in a test runner.
trait ClipboardAccess {
    fn get_text(&mut self) -> Result<String, arboard::Error>;
    fn set_text(&mut self, text: String) -> Result<(), arboard::Error>;
}

impl ClipboardAccess for arboard::Clipboard {
    fn get_text(&mut self) -> Result<String, arboard::Error> {
        arboard::Clipboard::get_text(self)
    }

    fn set_text(&mut self, text: String) -> Result<(), arboard::Error> {
        arboard::Clipboard::set_text(self, text)
    }
}

fn clipboard_error((error, attempts): (arboard::Error, u32)) -> ClipboardError {
    if is_occupied(&error) {
        ClipboardError::Busy { attempts }
    } else {
        ClipboardError::Backend(error.to_string())
    }
}

fn read_text(
    clipboard: &mut impl ClipboardAccess,
    retries: u32,
) -> Result<Option<String>, ClipboardError> {
    match with_retries(retries, RETRY_SPACING, || clipboard.get_text(), is_occupied) {
        Ok(text) => Ok(Some(text)),
        // An empty or non-text clipboard is the accepted text-only limitation;
        // a failed read is not evidence that there was nothing to preserve.
        Err((arboard::Error::ContentNotAvailable, _)) => Ok(None),
        Err(error) => Err(clipboard_error(error)),
    }
}

/// Installed only after replacement succeeds. Drop also covers verification,
/// key-synthesis errors and unwinding; restoration never replaces their cause
/// or turns an already-completed paste into another typing attempt.
struct ClipboardRestore<'a, C: ClipboardAccess> {
    clipboard: &'a mut C,
    stashed: Option<String>,
    retries: u32,
}

impl<C: ClipboardAccess> Drop for ClipboardRestore<'_, C> {
    fn drop(&mut self) {
        let Some(old) = &self.stashed else {
            return;
        };
        let restore = with_retries(
            self.retries,
            RETRY_SPACING,
            || self.clipboard.set_text(old.clone()),
            is_occupied,
        );
        if let Err((e, attempts)) = restore {
            log::warn!("could not restore clipboard after {attempts} attempt(s): {e}");
        }
    }
}

/// The full clipboard paste sequence. I/O glue over arboard + enigo:
/// verifiable only on real Windows/macOS (run-on-real-HW).
pub(crate) fn paste_via_clipboard(
    text: &str,
    settings: &InjectSettings,
) -> Result<(), ClipboardError> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|e| ClipboardError::Backend(format!("cannot open clipboard: {e}")))?;

    paste_with(
        &mut clipboard,
        text,
        settings,
        crate::keys::send_paste,
        std::thread::sleep,
    )
}

fn paste_with(
    clipboard: &mut impl ClipboardAccess,
    text: &str,
    settings: &InjectSettings,
    send_paste: impl FnOnce() -> Result<(), String>,
    mut wait: impl FnMut(Duration),
) -> Result<(), ClipboardError> {
    // 1. Stash the current TEXT content. "No text available" (image-only or
    //    empty clipboard) stashes None; that content is lost on restore, the
    //    accepted v1 clobber limitation.
    let stashed = read_text(clipboard, settings.clipboard_retries)?;

    // 2. Set our text, retrying while the clipboard is occupied.
    with_retries(
        settings.clipboard_retries,
        RETRY_SPACING,
        || clipboard.set_text(text.to_string()),
        is_occupied,
    )
    .map_err(clipboard_error)?;
    let restore = ClipboardRestore {
        clipboard,
        stashed,
        retries: settings.clipboard_retries,
    };

    // 3. Read-back verify: if the set did not take (clipboard managers and
    //    sync tools can interfere), pasting would inject stale content.
    let now = read_text(restore.clipboard, settings.clipboard_retries)?;
    if now.as_deref() != Some(text) {
        return Err(ClipboardError::VerifyMismatch);
    }

    // 4. Let the set settle before pasting (no OS-guaranteed timing).
    wait(Duration::from_millis(settings.set_paste_delay_ms));

    // 5. Synthesize the paste chord.
    let pasted = send_paste().map_err(ClipboardError::Paste);

    // 6. Even an error may follow a delivered V click (for example, modifier
    //    release failed). Let the target read before restoring in either case.
    wait(Duration::from_millis(settings.paste_restore_delay_ms));

    // 7. The guard restores the stash on this success path and every error
    //    after replacement. Restore failure is only a warning.
    drop(restore);
    pasted
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::collections::VecDeque;

    struct FakeClipboard {
        text: Option<String>,
        reads: VecDeque<Result<String, arboard::Error>>,
        read_count: usize,
        write_errors: VecDeque<Option<arboard::Error>>,
        writes: Vec<String>,
    }

    impl FakeClipboard {
        fn new(text: Option<&str>) -> Self {
            Self {
                text: text.map(str::to_string),
                reads: VecDeque::new(),
                read_count: 0,
                write_errors: VecDeque::new(),
                writes: Vec::new(),
            }
        }
    }

    impl ClipboardAccess for FakeClipboard {
        fn get_text(&mut self) -> Result<String, arboard::Error> {
            self.read_count += 1;
            self.reads
                .pop_front()
                .unwrap_or_else(|| self.text.clone().ok_or(arboard::Error::ContentNotAvailable))
        }

        fn set_text(&mut self, text: String) -> Result<(), arboard::Error> {
            self.writes.push(text.clone());
            if let Some(Some(error)) = self.write_errors.pop_front() {
                return Err(error);
            }
            self.text = Some(text);
            Ok(())
        }
    }

    fn transaction_settings(retries: u32) -> InjectSettings {
        InjectSettings {
            set_paste_delay_ms: 0,
            paste_restore_delay_ms: 0,
            clipboard_retries: retries,
            ..InjectSettings::default()
        }
    }

    #[test]
    fn busy_stash_is_retried_and_original_text_is_restored() {
        let mut clipboard = FakeClipboard::new(Some("old text"));
        clipboard
            .reads
            .push_back(Err(arboard::Error::ClipboardOccupied));
        let pastes = Cell::new(0);
        let result = paste_with(
            &mut clipboard,
            "dictation",
            &transaction_settings(1),
            || {
                pastes.set(pastes.get() + 1);
                Ok(())
            },
            |_| {},
        );
        assert_eq!(result, Ok(()));
        assert_eq!(clipboard.text.as_deref(), Some("old text"));
        assert_eq!(clipboard.read_count, 3);
        assert_eq!(pastes.get(), 1);
    }

    #[test]
    fn unreadable_stash_falls_back_without_replacing_clipboard() {
        let mut clipboard = FakeClipboard::new(Some("old text"));
        clipboard.reads.push_back(Err(arboard::Error::Unknown {
            description: "clipboard unavailable".into(),
        }));
        let error = paste_with(
            &mut clipboard,
            "dictation",
            &transaction_settings(2),
            || panic!("an unreadable stash must never reach paste"),
            |_| {},
        )
        .unwrap_err();
        assert!(matches!(error, ClipboardError::Backend(_)));
        assert!(error.should_fallback_to_typing());
        assert_eq!(clipboard.text.as_deref(), Some("old text"));
        assert_eq!(clipboard.read_count, 1);
        assert!(clipboard.writes.is_empty());
    }

    #[test]
    fn busy_stash_exhausts_its_budget_without_replacing_clipboard() {
        let mut clipboard = FakeClipboard::new(Some("old text"));
        clipboard.reads = (0..3)
            .map(|_| Err(arboard::Error::ClipboardOccupied))
            .collect();
        let error = paste_with(
            &mut clipboard,
            "dictation",
            &transaction_settings(2),
            || panic!("an unreadable stash must never reach paste"),
            |_| {},
        )
        .unwrap_err();
        assert_eq!(error, ClipboardError::Busy { attempts: 3 });
        assert!(error.should_fallback_to_typing());
        assert_eq!(clipboard.text.as_deref(), Some("old text"));
        assert_eq!(clipboard.read_count, 3);
        assert!(clipboard.writes.is_empty());
    }

    #[test]
    fn busy_verification_is_retried_and_pastes_once() {
        let mut clipboard = FakeClipboard::new(Some("old text"));
        clipboard.reads = [
            Ok("old text".into()),
            Err(arboard::Error::ClipboardOccupied),
        ]
        .into();
        let pastes = Cell::new(0);
        let result = paste_with(
            &mut clipboard,
            "dictation",
            &transaction_settings(1),
            || {
                pastes.set(pastes.get() + 1);
                Ok(())
            },
            |_| {},
        );
        assert_eq!(result, Ok(()));
        assert_eq!(clipboard.text.as_deref(), Some("old text"));
        assert_eq!(clipboard.read_count, 3);
        assert_eq!(pastes.get(), 1);
    }

    #[test]
    fn failed_verification_restores_before_typing_fallback() {
        let mut clipboard = FakeClipboard::new(Some("old text"));
        clipboard.reads = [
            Ok("old text".into()),
            Err(arboard::Error::ClipboardOccupied),
        ]
        .into();
        let error = paste_with(
            &mut clipboard,
            "dictation",
            &transaction_settings(0),
            || panic!("failed verification must never reach paste"),
            |_| {},
        )
        .unwrap_err();
        assert_eq!(clipboard.text.as_deref(), Some("old text"));
        assert_eq!(error, ClipboardError::Busy { attempts: 1 });
        assert!(error.should_fallback_to_typing());
    }

    #[test]
    fn verification_mismatch_restores_the_stash() {
        let mut clipboard = FakeClipboard::new(Some("old text"));
        clipboard.reads = [Ok("old text".into()), Ok("mismatched text".into())].into();
        let error = paste_with(
            &mut clipboard,
            "dictation",
            &transaction_settings(0),
            || panic!("mismatched verification must never reach paste"),
            |_| {},
        )
        .unwrap_err();
        assert_eq!(clipboard.text.as_deref(), Some("old text"));
        assert_eq!(error, ClipboardError::VerifyMismatch);
        assert!(error.should_fallback_to_typing());
    }

    #[test]
    fn failed_paste_restores_the_stash_without_typing_fallback() {
        let mut clipboard = FakeClipboard::new(Some("old text"));
        let error = paste_with(
            &mut clipboard,
            "dictation",
            &transaction_settings(0),
            || Err("key synthesis denied".into()),
            |_| {},
        )
        .unwrap_err();
        assert_eq!(clipboard.text.as_deref(), Some("old text"));
        assert_eq!(error, ClipboardError::Paste("key synthesis denied".into()));
        assert!(!error.should_fallback_to_typing());
    }

    #[test]
    fn restore_failure_preserves_the_paste_error() {
        let mut clipboard = FakeClipboard::new(Some("old text"));
        clipboard.write_errors = [None, Some(arboard::Error::ClipboardOccupied)].into();
        let error = paste_with(
            &mut clipboard,
            "dictation",
            &transaction_settings(0),
            || Err("key synthesis denied".into()),
            |_| {},
        )
        .unwrap_err();
        assert_eq!(clipboard.writes, ["dictation", "old text"]);
        assert_eq!(error, ClipboardError::Paste("key synthesis denied".into()));
        assert!(!error.should_fallback_to_typing());
    }

    #[test]
    fn restore_failure_after_success_does_not_trigger_another_insertion() {
        let mut clipboard = FakeClipboard::new(Some("old text"));
        clipboard.write_errors = [None, Some(arboard::Error::ClipboardOccupied)].into();
        let pastes = Cell::new(0);
        let result = paste_with(
            &mut clipboard,
            "dictation",
            &transaction_settings(0),
            || {
                pastes.set(pastes.get() + 1);
                Ok(())
            },
            |_| {},
        );
        assert_eq!(result, Ok(()));
        assert_eq!(clipboard.writes, ["dictation", "old text"]);
        assert_eq!(pastes.get(), 1);
    }

    #[test]
    fn no_text_stash_preserves_the_accepted_text_only_limitation() {
        let mut clipboard = FakeClipboard::new(None);
        let pastes = Cell::new(0);
        let result = paste_with(
            &mut clipboard,
            "dictation",
            &transaction_settings(0),
            || {
                pastes.set(pastes.get() + 1);
                Ok(())
            },
            |_| {},
        );
        assert_eq!(result, Ok(()));
        assert_eq!(clipboard.text.as_deref(), Some("dictation"));
        assert_eq!(clipboard.writes, ["dictation"]);
        assert_eq!(pastes.get(), 1);
    }

    #[test]
    fn a_paste_error_still_waits_before_restoring_in_case_v_was_emitted() {
        let mut clipboard = FakeClipboard::new(Some("old text"));
        let settings = InjectSettings {
            set_paste_delay_ms: 23,
            paste_restore_delay_ms: 41,
            ..transaction_settings(0)
        };
        let mut waits = Vec::new();
        let error = paste_with(
            &mut clipboard,
            "dictation",
            &settings,
            || Err("modifier release failed after V click".into()),
            |duration| waits.push(duration),
        )
        .unwrap_err();
        assert_eq!(
            waits,
            [Duration::from_millis(23), Duration::from_millis(41)]
        );
        assert_eq!(clipboard.text.as_deref(), Some("old text"));
        assert!(!error.should_fallback_to_typing());
    }

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

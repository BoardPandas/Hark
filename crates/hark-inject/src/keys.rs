//! Key synthesis via enigo: the Ctrl+V paste chord and the char-typing
//! fallback. I/O glue: run-on-real-HW only.
//!
//! enigo is pinned at 0.6.1: its synthesized events must carry the
//! injected flag (LLKHF_INJECTED on Windows) that `hark-hotkey` filters on,
//! and that contract has regressed across enigo versions before (RustDesk
//! #14667). The real-HW integration check in checkpoint 4's gate asserts
//! our own hook ignores the paste chord; re-run it on every enigo bump.

use enigo::{Direction, Enigo, Key, Keyboard, Settings};

fn new_enigo() -> Result<Enigo, String> {
    Enigo::new(&Settings::default()).map_err(|e| format!("cannot initialize key synthesis: {e}"))
}

/// Synthesize the platform paste chord (Ctrl+V; Cmd+V on macOS).
pub(crate) fn send_paste() -> Result<(), String> {
    let mut enigo = new_enigo()?;
    #[cfg(target_os = "macos")]
    let modifier = Key::Meta;
    #[cfg(not(target_os = "macos"))]
    let modifier = Key::Control;

    enigo
        .key(modifier, Direction::Press)
        .map_err(|e| format!("modifier press failed: {e}"))?;
    let result = enigo
        .key(Key::Unicode('v'), Direction::Click)
        .map_err(|e| format!("V click failed: {e}"));
    // Always release the modifier, even if the click failed: a stuck Ctrl
    // key is worse than a failed paste.
    let release = enigo
        .key(modifier, Direction::Release)
        .map_err(|e| format!("modifier release failed: {e}"));
    result.and(release)
}

/// Type the text character by character. Slower than pasting but touches no
/// clipboard: the fallback for paste-hostile fields.
pub(crate) fn type_text(text: &str) -> Result<(), String> {
    let mut enigo = new_enigo()?;
    enigo
        .text(text)
        .map_err(|e| format!("typing text failed: {e}"))
}

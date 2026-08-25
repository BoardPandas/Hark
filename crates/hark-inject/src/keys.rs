//! Key synthesis: the Ctrl+V paste chord and the char-typing fallback. I/O
//! glue: run-on-real-HW only.
//!
//! Two backends, chosen by session type rather than by trying one and seeing:
//!
//! - **enigo** everywhere except a Wayland session. It resolves keysyms
//!   against the active keyboard layout, so Ctrl+V is the paste chord and not
//!   whatever glyph sits on the V *position* — which matters on Dvorak and
//!   Colemak. On Windows and macOS it is the only backend.
//! - **uinput** on Wayland (`uinput_linux`). enigo drives X11's XTEST, which
//!   a Wayland compositor does not implement; worse, under XWayland the call
//!   can succeed and reach nothing, so "try enigo and fall back on error"
//!   would silently paste into the void. Deciding up front from
//!   `WAYLAND_DISPLAY` is the only way to be sure.
//!
//! enigo is pinned at 0.6.1: its synthesized events must carry the injected
//! flag (LLKHF_INJECTED on Windows) that `hark-hotkey` filters on, and that
//! contract has regressed across enigo versions before (RustDesk #14667). The
//! Linux equivalent of that flag is a device *name*: `hark-hotkey` skips
//! devices called `Hark*`, which is what `uinput_linux` names its keyboard.
//! The real-HW integration check in checkpoint 4's gate asserts our own hook
//! ignores the paste chord; re-run it on every enigo bump, on both sessions.

use enigo::{Direction, Enigo, Key, Keyboard, Settings};

fn new_enigo() -> Result<Enigo, String> {
    Enigo::new(&Settings::default()).map_err(|e| format!("cannot initialize key synthesis: {e}"))
}

/// Synthesize the platform paste chord (Ctrl+V; Cmd+V on macOS).
pub(crate) fn send_paste() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    if crate::uinput_linux::is_wayland_session() {
        return crate::uinput_linux::send_paste();
    }
    let result = send_paste_enigo();
    #[cfg(target_os = "linux")]
    if let Err(e) = &result {
        // An X11 session whose XTEST is unavailable (a locked-down server, a
        // remote display). uinput sits below the display server, so it is
        // worth one try before giving up on the paste entirely.
        log::warn!("X11 paste synthesis failed ({e}); trying the virtual keyboard");
        return crate::uinput_linux::send_paste();
    }
    result
}

fn send_paste_enigo() -> Result<(), String> {
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
    #[cfg(target_os = "linux")]
    if crate::uinput_linux::is_wayland_session() {
        // ASCII only, and it says so rather than dropping what it cannot type;
        // see `uinput_linux::type_text`.
        return crate::uinput_linux::type_text(text);
    }
    let mut enigo = new_enigo()?;
    enigo
        .text(text)
        .map_err(|e| format!("typing text failed: {e}"))
}

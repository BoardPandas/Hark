//! Key synthesis through a uinput virtual keyboard. I/O glue: run-on-real-HW.
//!
//! **Why this exists alongside enigo.** enigo drives X11's XTEST extension,
//! which a Wayland compositor does not implement -- and under XWayland an
//! XTEST event may be accepted and then reach nothing, which is the worst
//! failure shape there is: a paste that reports success and never lands. A
//! uinput device is a kernel input device, so its events enter the same path
//! real hardware does and every session type sees them. `keys.rs` picks
//! between the two by session, never by trial and error.
//!
//! **The device is created once and kept.** Creating it is not instant from
//! the compositor's point of view: udev has to notice the new node and the
//! session has to add it as an input source, and events emitted before that
//! completes are dropped on the floor. So the first caller pays
//! [`SETTLE`] once and every later injection reuses the open device.
//!
//! **Layout caveat, deliberate and documented.** uinput speaks key *positions*
//! (`KEY_V` is the key where V sits on a US board), and the session's keymap
//! decides what that position produces. On a US/UK/AZERTY layout `KEY_V` is
//! still `v`, so Ctrl+V pastes; on Dvorak it is `k`, and the paste chord would
//! be Ctrl+K. Nothing a uinput client can do about it -- the keymap is not
//! exposed to us -- and it is why X11 sessions go through enigo, which resolves
//! keysyms properly. This is the same trade `ydotool` and `wtype` make.

use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, EventType, InputEvent, KeyCode};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// The device name. **Load-bearing:** `hark-hotkey`'s evdev listener skips
/// devices whose name starts with `Hark`, which is the only thing standing
/// between our own synthesized Ctrl+V and the chord tracker that would read it
/// back. On a Ctrl-containing chord that is a dictation that re-triggers
/// itself forever. Keep the `Hark` prefix (see `hook_linux.rs`).
pub const VIRTUAL_KEYBOARD_NAME: &str = "Hark Virtual Keyboard";

/// How long to wait after creating the device before emitting into it, so the
/// compositor has added it as an input source. Paid once per process.
const SETTLE: Duration = Duration::from_millis(250);

/// Gap between synthesized edges. Real keyboards never emit a press and its
/// release in the same microsecond, and toolkits that debounce or that read
/// modifier state on a timer can miss a chord delivered all at once.
const EDGE_GAP: Duration = Duration::from_millis(4);

/// The process-wide device, created on first use. `Mutex` because two
/// injections must not interleave their edges -- a Ctrl left down between
/// another injection's press and release is a stuck modifier.
static DEVICE: OnceLock<Mutex<VirtualDevice>> = OnceLock::new();

/// Is this a Wayland session? Decides which synthesis backend `keys.rs` uses.
///
/// `WAYLAND_DISPLAY` is set by the compositor for its own clients and is the
/// check every Wayland-aware tool uses. It is tested BEFORE `DISPLAY` because
/// a Wayland session almost always runs XWayland too, so both are set and
/// only the order distinguishes them.
pub(crate) fn is_wayland_session() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty())
}

/// Every key the virtual device may ever emit. A uinput device can only send
/// codes it declared at creation, so this must cover the paste chord and the
/// whole typing table or those events are silently discarded.
fn declared_keys() -> AttributeSet<KeyCode> {
    let mut keys = AttributeSet::<KeyCode>::new();
    keys.insert(KeyCode::KEY_LEFTCTRL);
    keys.insert(KeyCode::KEY_LEFTSHIFT);
    for (_, code, _) in ASCII_KEYS {
        keys.insert(*code);
    }
    keys
}

/// The actionable form of a `/dev/uinput` permission failure. Users cannot act
/// on "Permission denied (os error 13)"; they can act on this.
fn open_error(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::PermissionDenied => format!(
            "cannot open /dev/uinput ({e}). Hark pastes through a virtual keyboard \
             and needs write access to it: install Hark's udev rule \
             (/usr/lib/udev/rules.d/70-hark-uinput.rules), add yourself to the \
             \"input\" group (sudo usermod -aG input $USER), then log out and back in."
        ),
        std::io::ErrorKind::NotFound => format!(
            "/dev/uinput does not exist ({e}). Load the kernel module with \
             \"sudo modprobe uinput\" (Hark's package configures this to load at boot)."
        ),
        _ => format!("cannot create the virtual keyboard: {e}"),
    }
}

/// The shared device, created on first call.
///
/// A creation failure is not cached: it is almost always a permission the user
/// can grant without restarting Hark, and a cached failure would keep telling
/// them it is still broken after they fixed it.
fn device() -> Result<&'static Mutex<VirtualDevice>, String> {
    if let Some(device) = DEVICE.get() {
        return Ok(device);
    }
    let keys = declared_keys();
    let built = VirtualDevice::builder()
        .map_err(|e| open_error(&e))?
        .name(VIRTUAL_KEYBOARD_NAME)
        .with_keys(&keys)
        .map_err(|e| open_error(&e))?
        .build()
        .map_err(|e| open_error(&e))?;

    // Only the thread that wins the race sleeps; a loser's device is dropped
    // and it reuses the winner's, which by then is at least as settled.
    match DEVICE.set(Mutex::new(built)) {
        Ok(()) => {
            log::info!("hark-inject: created the uinput virtual keyboard");
            std::thread::sleep(SETTLE);
        }
        Err(_) => log::debug!("hark-inject: another thread created the virtual keyboard first"),
    }
    DEVICE
        .get()
        .ok_or_else(|| "the virtual keyboard vanished after creation".to_string())
}

/// Emit one key edge. `value`: 1 = press, 0 = release.
fn edge(device: &mut VirtualDevice, code: KeyCode, value: i32) -> Result<(), String> {
    device
        .emit(&[InputEvent::new(EventType::KEY.0, code.0, value)])
        .map_err(|e| format!("cannot emit a key event: {e}"))?;
    std::thread::sleep(EDGE_GAP);
    Ok(())
}

/// Synthesize Ctrl+V.
///
/// The modifier is released whatever happens to the V edges: a stuck Ctrl
/// makes every subsequent keystroke on the machine a shortcut, which is far
/// worse than a paste that did not land.
pub(crate) fn send_paste() -> Result<(), String> {
    let mut device = lock()?;
    edge(&mut device, KeyCode::KEY_LEFTCTRL, 1)?;
    let tapped =
        edge(&mut device, KeyCode::KEY_V, 1).and_then(|()| edge(&mut device, KeyCode::KEY_V, 0));
    let released = edge(&mut device, KeyCode::KEY_LEFTCTRL, 0);
    tapped.and(released)
}

/// Type `text` character by character.
///
/// Fails on the first character outside the US-ASCII table rather than
/// dropping it: a transcript that silently loses its em dashes is a wrong
/// transcript, and the caller can still fall back to a clipboard paste, which
/// carries any character at all.
pub(crate) fn type_text(text: &str) -> Result<(), String> {
    let mut device = lock()?;
    for ch in text.chars() {
        let (code, shifted) = ascii_key(ch).ok_or_else(|| {
            format!(
                "cannot type {ch:?} through a virtual keyboard: uinput sends key positions, \
                 and only the US-ASCII positions have a known mapping. Use the clipboard \
                 injection strategy for text outside ASCII."
            )
        })?;
        if shifted {
            edge(&mut device, KeyCode::KEY_LEFTSHIFT, 1)?;
        }
        let tapped = edge(&mut device, code, 1).and_then(|()| edge(&mut device, code, 0));
        if shifted {
            // Same reasoning as the paste modifier: never leave Shift down.
            let released = edge(&mut device, KeyCode::KEY_LEFTSHIFT, 0);
            tapped.and(released)?;
        } else {
            tapped?;
        }
    }
    Ok(())
}

/// Take the device lock, recovering from a panic in another injection. The
/// device itself has no invariant a panic could break -- it is a file
/// descriptor -- so refusing to paste for the rest of the session because some
/// unrelated thread unwound would be the worse outcome.
fn lock() -> Result<std::sync::MutexGuard<'static, VirtualDevice>, String> {
    Ok(device()?.lock().unwrap_or_else(|e| e.into_inner()))
}

/// `(character, key position, is the shift level)` on a US layout.
///
/// Deliberately a flat table rather than arithmetic over char ranges: the
/// digits, the shifted symbols and the letters each follow different rules,
/// and three clever range branches are harder to check by eye than one list.
#[rustfmt::skip]
const ASCII_KEYS: &[(char, KeyCode, bool)] = &[
    (' ',  KeyCode::KEY_SPACE, false),      ('\t', KeyCode::KEY_TAB, false),
    ('\n', KeyCode::KEY_ENTER, false),
    ('a', KeyCode::KEY_A, false), ('A', KeyCode::KEY_A, true),
    ('b', KeyCode::KEY_B, false), ('B', KeyCode::KEY_B, true),
    ('c', KeyCode::KEY_C, false), ('C', KeyCode::KEY_C, true),
    ('d', KeyCode::KEY_D, false), ('D', KeyCode::KEY_D, true),
    ('e', KeyCode::KEY_E, false), ('E', KeyCode::KEY_E, true),
    ('f', KeyCode::KEY_F, false), ('F', KeyCode::KEY_F, true),
    ('g', KeyCode::KEY_G, false), ('G', KeyCode::KEY_G, true),
    ('h', KeyCode::KEY_H, false), ('H', KeyCode::KEY_H, true),
    ('i', KeyCode::KEY_I, false), ('I', KeyCode::KEY_I, true),
    ('j', KeyCode::KEY_J, false), ('J', KeyCode::KEY_J, true),
    ('k', KeyCode::KEY_K, false), ('K', KeyCode::KEY_K, true),
    ('l', KeyCode::KEY_L, false), ('L', KeyCode::KEY_L, true),
    ('m', KeyCode::KEY_M, false), ('M', KeyCode::KEY_M, true),
    ('n', KeyCode::KEY_N, false), ('N', KeyCode::KEY_N, true),
    ('o', KeyCode::KEY_O, false), ('O', KeyCode::KEY_O, true),
    ('p', KeyCode::KEY_P, false), ('P', KeyCode::KEY_P, true),
    ('q', KeyCode::KEY_Q, false), ('Q', KeyCode::KEY_Q, true),
    ('r', KeyCode::KEY_R, false), ('R', KeyCode::KEY_R, true),
    ('s', KeyCode::KEY_S, false), ('S', KeyCode::KEY_S, true),
    ('t', KeyCode::KEY_T, false), ('T', KeyCode::KEY_T, true),
    ('u', KeyCode::KEY_U, false), ('U', KeyCode::KEY_U, true),
    ('v', KeyCode::KEY_V, false), ('V', KeyCode::KEY_V, true),
    ('w', KeyCode::KEY_W, false), ('W', KeyCode::KEY_W, true),
    ('x', KeyCode::KEY_X, false), ('X', KeyCode::KEY_X, true),
    ('y', KeyCode::KEY_Y, false), ('Y', KeyCode::KEY_Y, true),
    ('z', KeyCode::KEY_Z, false), ('Z', KeyCode::KEY_Z, true),
    ('1', KeyCode::KEY_1, false), ('!', KeyCode::KEY_1, true),
    ('2', KeyCode::KEY_2, false), ('@', KeyCode::KEY_2, true),
    ('3', KeyCode::KEY_3, false), ('#', KeyCode::KEY_3, true),
    ('4', KeyCode::KEY_4, false), ('$', KeyCode::KEY_4, true),
    ('5', KeyCode::KEY_5, false), ('%', KeyCode::KEY_5, true),
    ('6', KeyCode::KEY_6, false), ('^', KeyCode::KEY_6, true),
    ('7', KeyCode::KEY_7, false), ('&', KeyCode::KEY_7, true),
    ('8', KeyCode::KEY_8, false), ('*', KeyCode::KEY_8, true),
    ('9', KeyCode::KEY_9, false), ('(', KeyCode::KEY_9, true),
    ('0', KeyCode::KEY_0, false), (')', KeyCode::KEY_0, true),
    ('-',  KeyCode::KEY_MINUS, false),      ('_', KeyCode::KEY_MINUS, true),
    ('=',  KeyCode::KEY_EQUAL, false),      ('+', KeyCode::KEY_EQUAL, true),
    ('[',  KeyCode::KEY_LEFTBRACE, false),  ('{', KeyCode::KEY_LEFTBRACE, true),
    (']',  KeyCode::KEY_RIGHTBRACE, false), ('}', KeyCode::KEY_RIGHTBRACE, true),
    ('\\', KeyCode::KEY_BACKSLASH, false),  ('|', KeyCode::KEY_BACKSLASH, true),
    (';',  KeyCode::KEY_SEMICOLON, false),  (':', KeyCode::KEY_SEMICOLON, true),
    ('\'', KeyCode::KEY_APOSTROPHE, false), ('"', KeyCode::KEY_APOSTROPHE, true),
    ('`',  KeyCode::KEY_GRAVE, false),      ('~', KeyCode::KEY_GRAVE, true),
    (',',  KeyCode::KEY_COMMA, false),      ('<', KeyCode::KEY_COMMA, true),
    ('.',  KeyCode::KEY_DOT, false),        ('>', KeyCode::KEY_DOT, true),
    ('/',  KeyCode::KEY_SLASH, false),      ('?', KeyCode::KEY_SLASH, true),
];

/// The key position and shift level that produce `ch` on a US layout, or
/// `None` when nothing does.
fn ascii_key(ch: char) -> Option<(KeyCode, bool)> {
    ASCII_KEYS
        .iter()
        .find(|(c, _, _)| *c == ch)
        .map(|(_, code, shifted)| (*code, *shifted))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn the_device_name_is_one_hark_hotkey_will_skip() {
        // hark-hotkey filters devices whose name starts with "Hark". If this
        // name ever stops matching, our own Ctrl+V feeds the chord tracker and
        // a Ctrl-containing chord dictates in an endless loop.
        assert!(VIRTUAL_KEYBOARD_NAME.starts_with("Hark"));
    }

    #[test]
    fn every_printable_ascii_character_is_typable() {
        // The gap this closes: a missing row is not a compile error, it is a
        // transcript that refuses to type one character at run time.
        for byte in 0x20u8..=0x7e {
            let ch = byte as char;
            assert!(ascii_key(ch).is_some(), "no key position for {ch:?}");
        }
        assert!(ascii_key('\n').is_some());
        assert!(ascii_key('\t').is_some());
    }

    #[test]
    fn characters_are_unique_in_the_table() {
        // A duplicate row is a silent wrong answer: `find` takes the first,
        // so the second spelling of a character would never be used and could
        // disagree with it.
        let mut seen = HashSet::new();
        for (ch, _, _) in ASCII_KEYS {
            assert!(seen.insert(*ch), "{ch:?} appears twice in the table");
        }
    }

    #[test]
    fn shifted_and_unshifted_pairs_share_a_key_position() {
        // The pairing is the whole point of the shift column; a mismatch here
        // types the wrong glyph rather than failing.
        for (lower, upper) in [('a', 'A'), ('z', 'Z'), ('1', '!'), ('/', '?')] {
            let (lo, lo_shift) = ascii_key(lower).expect("lower");
            let (up, up_shift) = ascii_key(upper).expect("upper");
            assert_eq!(lo, up, "{lower:?} and {upper:?} are the same key");
            assert!(!lo_shift, "{lower:?} is the unshifted level");
            assert!(up_shift, "{upper:?} is the shifted level");
        }
    }

    #[test]
    fn characters_outside_ascii_are_refused_not_dropped() {
        // Exactly the characters the cleanup pass likes to produce. Refusing
        // is what lets the caller fall back to a clipboard paste; dropping
        // would corrupt the transcript silently.
        for ch in ['—', '“', '”', '’', 'é', '€'] {
            assert!(ascii_key(ch).is_none(), "{ch:?} must not claim a mapping");
        }
    }

    #[test]
    fn the_declared_key_set_covers_the_paste_chord_and_the_table() {
        // A uinput device silently discards codes it did not declare, so a key
        // missing here is an injection that reports success and types nothing.
        let keys = declared_keys();
        assert!(keys.contains(KeyCode::KEY_LEFTCTRL));
        assert!(keys.contains(KeyCode::KEY_LEFTSHIFT));
        assert!(keys.contains(KeyCode::KEY_V), "the paste chord needs V");
        for (ch, code, _) in ASCII_KEYS {
            assert!(keys.contains(*code), "{ch:?} needs its key declared");
        }
    }

    #[test]
    fn wayland_is_detected_from_the_compositor_socket_only() {
        // Both variables are set in a Wayland session that runs XWayland, so
        // the answer must come from WAYLAND_DISPLAY alone. Asserting on the
        // real environment would make this test depend on the dev machine's
        // session, so it checks the rule the function encodes instead.
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty());
        assert_eq!(is_wayland_session(), wayland);
    }
}

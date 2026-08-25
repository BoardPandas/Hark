# hark-inject rules

- **set -> paste -> restore is a race with no OS-guaranteed timing.** Pasting
  immediately after a clipboard set can paste the OLD content. The tunable
  delays (`set_paste_delay_ms`, `paste_restore_delay_ms`) plus the read-back
  verify are the mitigation; tune them on real hardware and never remove the
  verify.
- **The clipboard is a global object.** Any open can fail with
  ClipboardOccupied while another process holds it: every clipboard operation
  runs inside the bounded `with_retries` loop. On exhaustion, fall back to
  char typing rather than failing the dictation.
- **Text-only round-trip is the accepted v1 limitation.** arboard's
  `set_text` clears all other clipboard formats: images/RTF/HTML present
  before dictation are NOT preserved by stash/restore. Documented behavior,
  not a bug; full fidelity needs per-format EnumClipboardFormats work that is
  out of scope until it hurts.
- **enigo stays pinned (0.6.1).** Its synthesized events must carry the
  injected flag (`LLKHF_INJECTED`) that `hark-hotkey` filters on, and that
  contract has regressed across enigo versions before (RustDesk #14667). On
  any enigo bump, re-run the real-HW check that our own hook ignores our own
  Ctrl+V.
- **Restore failure is a warning, not a failed dictation**: by that point the
  text is already pasted. Key-synthesis failure never falls back to typing
  (typing rides the same machinery).
- **Never log injected text content** at info level or above; lengths only.

## Linux (`uinput_linux.rs`)

- **The backend is chosen by SESSION, never by trial and error.** enigo drives
  X11's XTEST, which a Wayland compositor does not implement — and under
  XWayland the call can succeed and reach nothing. "Try enigo, fall back on
  error" would therefore paste into the void and report success, which is worse
  than failing. `keys.rs` reads `WAYLAND_DISPLAY` first (a Wayland session
  almost always runs XWayland too, so both variables are set and only the order
  distinguishes them) and commits.
- **The device name is load-bearing.** `hark-hotkey` skips devices named
  `Hark*`; that is the Linux stand-in for `LLKHF_INJECTED`, and it is the only
  thing keeping our own Ctrl+V out of the chord tracker. Both crates key off
  the bare app name rather than sharing a constant, and both pin it in a test.
- **Create the device once, and pay the settle delay once.** udev has to notice
  the new node and the session has to add it as an input source; events emitted
  before that are dropped. The `OnceLock` device plus `SETTLE` is what makes
  the *first* paste land, not just the second.
- **Always release the modifier.** A stuck Ctrl or Shift turns every subsequent
  keystroke on the machine into a shortcut — far worse than a paste that did
  not land. Both paths release unconditionally, then combine the results.
- **Typing is US-ASCII only, and it REFUSES the rest.** uinput sends key
  positions and Wayland exposes no keymap to clients, so anything outside the
  table has no honest mapping. Dropping those characters would silently corrupt
  a transcript — and they are exactly what the cleanup pass emits (em dashes,
  curly quotes). Failing lets the caller fall back to a clipboard paste, which
  carries any character. Do not "improve" this by substituting look-alikes.
- **Ctrl+V is a key position, not a keysym, on Wayland.** On Dvorak the paste
  chord lands on the wrong key and there is nothing a uinput client can do
  about it. Documented in packaging/LINUX.md; it is why X11 keeps using enigo.

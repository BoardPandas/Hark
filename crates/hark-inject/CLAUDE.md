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

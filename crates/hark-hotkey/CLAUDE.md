# hark-hotkey rules

- **The message pump IS the hook.** `WH_KEYBOARD_LL` delivers callbacks only
  while the installing thread runs its `GetMessageW`/`DispatchMessageW` loop.
  The hook thread's entire body must be that loop: it can never sleep, park,
  do other work, or be shared with the cpal or pipeline threads.
- **Always feed `LLKHF_INJECTED` through as `injected`.** enigo's synthesized
  Ctrl+V IS seen by our own hook; the tracker drops injected events or
  dictation paste-injects into an infinite PTT loop. The injected-flag
  contract has regressed across enigo versions before (RustDesk #14667):
  enigo stays pinned and the real-HW check that our hook ignores our own
  Ctrl+V guards every enigo upgrade.
- **Keep the callback lean.** Windows silently removes low-level hooks that
  exceed `LowLevelHooksTimeout`: map the VK, feed the tracker, send, return.
  Never block, never do I/O in the callback.
- **Observe, never swallow.** Always `CallNextHookEx`. The Ctrl+Win default
  chord needs no swallowing: Windows marks a Win press "used in a chord"
  when another key goes down while it is held, so the Start menu does not
  fire on release.
- **Platform seam:** `spawn_listener(chord, tx)` is the only entry point.
  `hook_mac.rs` (CGEventTap, checkpoint 7, NEEDS MAC) must implement the same
  signature and feed the same `edges.rs` tracker; the tap thread owns its own
  `CFRunLoop` and must not fight the egui/winit main loop.
- **Edge semantics live in `edges.rs` only** (pure, exhaustively tested):
  engage on last chord member down, disengage on first up, auto-repeat
  filtered, non-chord keys ignored.

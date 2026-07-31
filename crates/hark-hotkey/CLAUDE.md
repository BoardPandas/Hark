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
- **A hook does not see every release, so the tracker is polled while a chord
  is held.** Releases on another desktop (lock screen, UAC, Ctrl+Alt+Del),
  across a sleep/resume, or after Windows quietly unhooks a slow callback never
  arrive, and one lost release wedges the tracker `engaged` forever: the
  recording never ends, the pill never goes away, and no later press produces
  an edge either. A `SetTimer` thread timer, armed only between engage and
  disengage, calls `ChordTracker::resync_released` (`GetAsyncKeyState`) and
  emits `PttEvent::UpMissed`. It runs inline in the pump — a thread timer has
  no window, so `DispatchMessageW` would drop `WM_TIMER` — and must stay a
  handful of key-state reads: the pump is still the hook's lifeline.
- **Heal releases only, never presses.** Synthesizing a press from a poll would
  let a chord the user was already holding start a dictation nobody asked for.
- **Observe, never swallow — with exactly one exception.** Default: always
  `CallNextHookEx`. Skipping it starves every earlier-registered hook, the
  target window proc, `RegisterHotKey` hotkeys and Raw Input, in every app on
  the machine. `swallow` is a stack local initialised `false`, so every path
  that is not the exception — an unmapped VK, an armed capture tap, a lost
  receiver — falls through unchanged. The Ctrl+Win default needs no
  swallowing: Windows marks a Win press "used in a chord" when another key
  goes down while it is held, so the Start menu does not fire on release.
  **The exception:** a Caps Lock or Scroll Lock key-down, when it is a member
  of the running chord and the tracker is engaged, so a dictation does not
  also flip the lock. `ChordTracker::swallow` is the whole policy and it is
  pure. Every clause is load-bearing:
  - **Key-down only.** The toggle rides the make code; swallowing a release
    buys nothing and is the only way to leave a lock key stuck from the
    system's point of view. Down-only makes that state unrepresentable.
  - **Derived from `engaged`, never from a physical poll.** A poll is strictly
    weaker than the engage condition and would fire with no dictation running.
    The invariant that makes this defensible is that *every swallowed
    keystroke has a visible dictation attached to it*.
  - **Never a 1-key chord.** "All the other members are held" is vacuously
    true when there are none, which would kill Caps Lock globally, forever.
  - **Never with Alt or Win in the chord.** A swallowed press is invisible to
    Windows, so it cannot mark them "used in a chord" and the Start menu or
    menu bar pops on every dictation.
  - **Never Num Lock.** Windows applies its toggle above the hook, so
    swallowing eats the keystroke and the lock flips anyway.
  - **Never two locks in one chord.** The suppressed member reads "up" all
    hold, so the chord needs a member the watchdog can still read truthfully.
  - **`resync_released` skips the suppressed member.** It never reaches the
    system, so polling it would fire a bogus `UpMissed` at the first tick and
    paste a quarter second of audio at the cursor on every dictation.
  Known and accepted: the lock key must be pressed **last** (press it first
  and it toggles), and screen readers that use Caps Lock as their modifier
  lose it for that chord. `[hotkey] swallow_lock_keys = false` restores the
  observe-only hook exactly.
- **Platform seam:** `spawn_listener(chord, swallow_locks, tx)` is the only entry point.
  `hook_mac.rs` (CGEventTap, checkpoint 7, NEEDS MAC) must implement the same
  signature and feed the same `edges.rs` tracker; the tap thread owns its own
  `CFRunLoop` and must not fight the egui/winit main loop.
- **Edge semantics live in `edges.rs` only** (pure, exhaustively tested):
  engage on last chord member down, disengage on first up, auto-repeat
  filtered, non-chord keys ignored.

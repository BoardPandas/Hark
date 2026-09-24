# Plan: detect a push-to-talk key that another program intercepts

Date: 2026-09-24. Trigger: HeikaBlade's report ("can't hold my button to talk,
it flicker records") on COLT-SHINE, Hark 0.45.4, chord `LAlt+F12`.

## Diagnosis (from the machine's logs and config)

- PowerToys Keyboard Manager remapped `F12 -> Ctrl+F13` at 15:08 local. The
  flicker started at 15:01 on 0.45.2, before the update; the update only
  restarted Hark and changed which hook runs first.
- PowerToys' hook swallows the physical F12 and injects its replacement, so
  Windows never registers F12 as down. Hark's hook (first in the chain after a
  restart) still sees the physical press and engages the chord.
- The watchdog polls `GetAsyncKeyState`, reads F12 "up", and emits `UpMissed`
  every 250 ms tick. Auto-repeat re-engages; the loop is the flicker. 91
  "release never arrived" warnings in 76 presses.

## Approach

1. **Detect** (`edges.rs`, pure): an auto-repeat through the hook proves a key
   is held. On a repeat of a chord member that the key state has not yet
   confirmed, read the key state once. Up means a later hook swallowed the
   press: emit `PttEvent::Intercepted(key)`, once per member per tracker.
   Never for Hark's own swallowed lock member. Never on the first press (the
   hook runs before Windows registers it; LL-G `chord-tracker-missed-release`).
2. **Tolerate** (`resync_released`): a member the key state has never read
   down is released by the watchdog only once the hook has gone quiet on it
   for `HELD_EVIDENCE` (1.5 s, longer than the slowest keyboard repeat delay).
   A member the poll has confirmed heals on the next tick, exactly as today.
3. **Report**: hook -> worker (log) -> `PipelineEvent::ShortcutIntercepted` ->
   the UI keeps it as a persistent footer warning while idle, with the
   Settings jump. Cleared when the pipeline restarts.

Not addressed: a remapper whose hook runs *before* Hark's. Hark then never sees
the physical key at all, only injected replacements it ignores by design, and
there is nothing in the event stream to attribute.

## Files

`hark-hotkey/src/{edges,hook_win,hook_linux}.rs`, `hark-hotkey/CLAUDE.md`,
`hark-pipeline/src/{events,worker}.rs`, `hark-app/src/{pipeline,overlay/feedback,ui/footer,ui/shell}.rs`.

## Lessons Learned / Gotchas

- **A key remapper makes `GetAsyncKeyState` lie about a held key.** Any
  low-level hook later in the chain that swallows a press stops Windows from
  registering it, so a watchdog that trusts the key state over the hook
  declares the release missed on every tick. Auto-repeat through the hook is
  the tie-breaker: a released key cannot repeat.
- **A regression that lines up with an update is not proof the update did
  it.** The flicker began nine minutes before 0.45.4 installed; only the
  per-version log counts and the remap file's timestamp showed that.
- Route both to LL-G (`kb/rust/` or `kb/windows/`) via `/add-lesson`.

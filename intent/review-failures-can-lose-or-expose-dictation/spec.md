# Review repair specification

Status: Authorized implementation of the nine reviewed defects; no new product scope.
Source: owner approval in the September 30, 2026 chat; reviewed baseline `9a7cfdb`.

## Required behavior

1. Retiring a pipeline invalidates its pending work. It must not begin injection,
   cleanup, retry, or fallback after retirement. An injection already underway
   completes clipboard restoration; shutdown remains bounded.
2. A failed audio deletion preserves the meeting record and exposes a retryable
   error. Missing audio may still permit record deletion. Unknown folders remain
   protected from automatic deletion.
3. Provider/parser errors never include user content or secrets. Keep useful
   status, category, and structural location diagnostics.
4. Simultaneous Linux playback sources feed one aligned, bounded mono timeline;
   source count cannot multiply its duration. A missing app source activates and
   attempts default-sink capture before reporting failure.
5. Streaming resampling returns the exact total frame count and preserves the
   ending signal, including block-aligned and short input.
6. Clipboard reads use bounded retries. An unreadable stash must not be
   overwritten. Every failure after replacement attempts restoration, preserving
   typing fallback and non-text-format limitations.
7. Shortcut cycles begun while busy are rejected, including a release after the
   worker becomes idle. An accepted cycle uses its actual observation position
   rather than a delayed queue-consumption position.
8. Any attempted live session consumes the first cloud attempt even if its pump
   fails during the hold. At most one finished-clip replay remains.

## Verification

Add failing regressions before implementing each repair where the existing seam
permits it; introduce narrow injectable seams for hardware boundaries. Exercise
cancellation, busy-cycle ordering, retry counts, deletion failure/retry, safe
errors, clipboard transaction failures, mixed audio, fallback state, and exact
resampler ending output. Run both npm guards, formatting, strict all-targets
Clippy, and workspace tests. Native OS behavior remains explicitly unverified.

## Scope

Update mapped documentation and changelog. The owner subsequently requested:
“Commit and push to main and tag a new build.” Release 0.61.1 includes the patch
version bump, a commit on main, and a matching build tag. Native validation
limits remain unchanged.

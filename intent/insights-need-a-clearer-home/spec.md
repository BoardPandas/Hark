# Hark Home, Insights, and appearance

Status: Approved direction; implementation specification derived from the owner's September 30, 2026 approval.

## Contract

Recreate the approved preview in the existing native Rust/egui application. No webview or hosted service. Preserve all existing editors, meeting functionality, onboarding, provider controls, draft/save/discard behavior, deletion confirmations, status/footer, and nonactivating recording overlay.

## Foundation and Home

- Replace top page tabs with a labeled left sidebar: Home, Insights, History, Meetings; Spellbook and Invocations; Settings. Use a compact layout when the window is narrow. Home becomes the running-pipeline landing page; onboarding/errors continue to open Settings.
- Home displays the actual configured shortcut and engine/voice state, recent real history and real totals, with navigation to full History and Insights. No decorative recording button or sample transcript in production.
- Centralize typography, spacing, surfaces, and chart tokens in the existing theme system. Embed fonts locally with their license. Preserve keyboard navigation, visible focus, bounded scroll areas, and readable text contrast.
- Persist System, Light, Dark, Solarized Light, and Solarized Dark. The four explicit choices match the approved preview; System preserves automatic OS appearance and existing preferences. Use the published Solarized base colors and accents, with clearly derived borders and accessible text pairs.

## Insights and collection

- Overview offers 7/30/90-day ranges, words/dictations, estimated pace and time saved against the existing 40 WPM baseline, median release-to-inject latency, dated output bars, local-calendar activity/streaks, and optional app distribution.
- Your voice offers local, opt-in word/phrase analysis of retained transcripts, peak usage time, configured vocabulary size, measured correction totals and invocation usage/expanded words. Describe scope and missing data honestly. No personality judgments, population rankings, inferred accuracy, or transcript upload for insights.
- Performance offers median/p95 recorded completion latency and provider breakdown, clearly measuring completed dictations rather than a fabricated success/failure rate.
- Keep numeric timestamped dictation facts separate from transcript capture. Record clip duration, counted words, latency, provider/voice labels, invocation metadata, actual correction counts, and optional app label after injection. Bound retained details to 366 days. History clear/disable does not erase numeric stats; explicit stats reset does. Explain this in privacy settings and reset confirmation.
- Backfill only fields genuinely recoverable from retained history. Clip duration and correction counts stay NULL for legacy entries. Duration-based metrics only use measured rows, with coverage shown; never multiply a lifetime average into fake historical measurements.
- App tracking and transcript word/phrase analysis default off. App tracking collects application identity only at dictation, never window/document titles or continuous activity. Platform limitations, including compositor-restricted Wayland detection, are surfaced as unavailable/unknown rather than guessed. No extra permissions or provider requests merely to populate charts.
- Execute database queries and text aggregation on the storage worker; refresh through generation/range changes and local date rollover, never per UI frame. Never add I/O to audio callbacks, hooks, or the release-to-inject wait.

## Acceptance

- Schema migrations preserve existing history and lifetime counters; legacy unknowns, disabled capture, retention, history clear, reset, local dates/DST, invocation counting, percentiles, and opt-in boundaries have meaningful tests.
- Four explicit palettes plus System persist and satisfy text contrast tests. Empty/loading/error and unavailable-data states display without sample values or NaN/division errors.
- Run repository wiring and documentation guards, format, strict all-targets Clippy, workspace tests and platform cross-checks where available. Native rendering, input, audio, and OS app-detection runtime require real target-platform validation; report that boundary explicitly.

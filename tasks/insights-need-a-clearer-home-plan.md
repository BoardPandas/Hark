# Hark Home and Insights implementation

Source: [approved intent](../intent/insights-need-a-clearer-home/intent.md) and [spec](../intent/insights-need-a-clearer-home/spec.md).

The prior chat produced and validated the design preview; this follow-up implements the approved direction. User approval already authorizes the new stats metadata and native UI work. No further approval gate is needed for reversible implementation.

## Foundation

- [x] Add centralized warm Light/Dark and official Solarized palettes, persistent appearance selection, readable chart tokens and licensed editorial typography.
- [x] Add bounded numeric insights storage/migration, trustworthy legacy coverage, local-calendar aggregates and storage-worker query API.

## Core

- [x] Capture actual correction and optional app metadata without delaying injection; add opt-in privacy controls.
- [x] Replace the shell navigation, add real Home content and implement Overview, Your voice, and Performance using asynchronous cached queries.

## Polish

- [x] Preserve existing editors, settings save/discard, onboarding, meeting navigation, confirmation and empty/error states.
- [x] Update changelog and mapped documentation with retention/privacy semantics and native-validation limits.

## Verification

- [x] Run focused regressions, then both npm guards, formatting, strict all-targets Clippy, and workspace tests. Cross-check available platform targets and inspect the final diff.

- Workspace: 1,075 passed, 0 failed, 1 existing local-model-dependent test ignored; includes 131 application tests. Both npm guards, formatting, strict Clippy, and diff whitespace checks passed. Linux debug build passed.
- Windows GNU and Apple Silicon macOS: strict Clippy passed for an isolated harness importing the exact new foreground-app module, not the full app.
- Independent review identified and fixed short-window sidebar clipping, early Insights metric hiding, idle midnight refresh, and Home partial-day coverage. Headless layout checks cover all palettes at 720×480, 760×480, 900×480, and 960×640, with footer space reserved.
- Native window composition, microphone/hook/injection, and OS app detection still require real target-platform smoke testing. No release or live dictation was run here.

## Delivery

The owner subsequently requested: “Commit and push to main and tag a new build.” This authorizes the main-branch commit and tag-triggered release. Prepare v0.61.0 with matching package/Cargo versions and versioned changelog; GitHub Actions builds and publishes the platform installers after the tag push.

## Lessons Learned / Gotchas

- Existing history omits clip duration; pace/time-saved history cannot be faithfully reconstructed.
- Invocation expansions must not inflate dictated-word totals. Expanded words are a separate measure.
- Theme restoration must preserve existing egui appearance choices; Solarized is a palette family as well as light/dark preference.
- All metrics run off the latency-critical path. A missing target app is unknown, not evidence of “other” or permission to scrape window titles.
- This Fedora build host needed clang-libs plus `BINDGEN_EXTRA_CLANG_ARGS='-isystem /usr/lib/clang/22/include'` for PipeWire bindgen. The application itself needs no workaround.

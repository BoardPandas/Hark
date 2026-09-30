# macOS parity

Requested 2026-09-29: “Build the Mac version to full parity with the Windows version please.”

## Foundation

Preserve the existing shared Rust UI, providers, storage, processing, and Windows/Linux implementations. Implement native seams behind macOS cfgs. All AppKit objects remain on the main thread. Use macOS 14.2+ for Core Audio process taps. Preserve the user's existing AGENTS.md edit.

## Core

- Global held-key dictation, shortcut recording, meeting toggle, release recovery, and injection permission checks through Quartz.
- Core Audio process taps for system/app audio, genuine microphone activity detection, and shared meeting pipeline, AEC, final passes, notes, and retention.
- Permission status/request/recovery in setup and Settings; native save, Word export, share sheet and Finder actions; monitor-aware overlays.
- Native launch at login and signed-bundle update verification/replacement.

## Polish

Platform-appropriate labels, keyboard names and defaults, actionable permission errors, and Mac usage/build documentation. Keep platform limitations explicit rather than silently substituting unrelated behavior.

## Ship

Build the full default-feature Mac app and bundle/DMG; add Mac CI alongside Windows/Linux. Run formatting, workspace tests, clippy with warnings denied, and repository wiring/documentation checks. Signing and notarization require the owner's Apple credentials; live permissions, keyboard, microphone, meeting apps, display placement and share sheet require interactive hardware validation.

## Lessons Learned / Gotchas

- Core Audio process taps require NSAudioCaptureUsageDescription and macOS 14.2+; microphone permission is separate.
- A running process is not evidence of a call. Detection must inspect actual microphone activity; browser titles may additionally require Screen Recording permission.
- Quartz modifier changes and injected events need explicit handling; Caps Lock toggle notifications are not held-key edges.
- Never substitute an unsigned executable for a signed .app during update: verify bundle identity and signer, preserve replacement rollback, and relaunch after the outgoing process exits.
- Compilation and synthetic tests cannot establish audible capture or interactive permission success.

## Implementation and validation result

The native backends and application integration are implemented. Apple Silicon `cargo build --release -p hark-app`, 1,035 workspace tests (one pre-existing downloaded-model test ignored), strict workspace Clippy, formatting, wiring and documentation checks pass. `dist/Hark.app` and `dist/Hark-0.58.0-macos-arm64.dmg` are local ad-hoc signed artifacts. The packaged executable starts with `--version`; signature structure, framework closure and DMG checksums pass.

Production signing/notarization requires Apple credentials. Interactive permission, microphone, real-call capture, focus/Spaces, login and signed-update acceptance remain unverified. Intel CI is configured but was not executed locally. Caps Lock and keys without Quartz held-key mappings are explicitly rejected on Mac; use Control + Command or another supported chord.

# Generation Metadata

## macOS implementation parity — 2026-09-29

- **Source reviewed:** `1c1384c` plus the current macOS native backend, app, export, permission, update, login-item and packaging changes. Existing Windows and Linux implementations are preserved behind their platform gates.
- **Scope:** Mac CGEventTap and Core Audio process taps, real microphone-use detection, native AppKit dialogs/share/overlays, SMAppService, signed-bundle updates and both Mac architecture CI jobs. New native source files are included in the documentation map.
- **Baseline policy:** global baseline `784272c` remains unchanged.
- **Validation:** native Apple Silicon default-feature release build; 1,035 workspace tests pass, one existing local-model fixture test ignored because it requires the ~670 MB downloaded model; workspace Clippy with warnings denied; formatting; wiring; documentation drift and guard tests. Ad-hoc signed Hark.app and arm64 DMG built, signature structure and system-library closure verified, packaged executable `--version` succeeds, DMG checksum verified.
- **Not yet validated:** interactive TCC approvals, actual microphone/system-audio capture, dictation insertion and focus/Spaces behavior, native login registration, Intel executable runtime, Developer ID notarization and a signed update/relaunch. Intel CI and signed release jobs are configured; those jobs were not run in this local session.

## Scoped Linux meetings parity — 2026-09-29 (0.60.0)

- **Source reviewed:** `1c1384c` (0.58.0) plus the new Linux seams: `crates/hark-audio/src/loopback/{mod,linux}.rs`, `crates/hark-meeting/src/probe/{mod,linux,watch_linux}.rs`, `crates/hark-hotkey/src/hook_linux.rs` (Shortcuts mode), the `meetings_supported()` centralization, the Share-menu un-gating, and the CI/packaging dependency additions. Rebased over the macOS parity work (`5413853`, `6414856`): the loopback and probe facades now dispatch all three platforms.
- **Evidence:** on a private `PIPEWIRE_RUNTIME_DIR` stack (PipeWire 1.6 + WirePlumber + a null sink) the production loopback smoke captured ~16,000 frames/s of 16 kHz mono f32 in both modes — default-sink monitor for `ExcludeTree`, and the targeted stream node for `IncludeTree` (a sine routed to a non-default sink arrived only via its stream node) — and the detection smoke observed a fake `zoom` mic holder, debounced, and issued `Prompt("zoom")` with graph-event watcher wakes. Real-mic-in-a-real-call behavior remains native-user validation, as it was for Windows.
- **Scope:** Meetings (platform paragraph, operational bullets, hand-check bullet), Audio Capture (hook sharing and the loopback platform sections), Overview (platform rows), Release and Packaging (libpipewire build/runtime dependency and gates), packaging/LINUX.md (known differences), notices (PipeWire, x11rb), AGENTS.md (platform sentence).
- **Baseline policy:** global baseline `784272c` remains unchanged.
- **Validation:** `cargo fmt --all -- --check`, workspace `cargo clippy --all-targets -- -D warnings`, `cargo test --workspace` (all green), `npm run check:docs`, and `npm run check:claude` pass on the Linux dev machine with `libpipewire` 1.6.9, on the tree rebased over the macOS work.

## Scoped Elevate call detection — 2026-09-29 (0.58.0)

- **Source reviewed:** `cc37f47` (0.57.5) plus `crates/hark-meeting/src/detect.rs` (the "Elevate UC" browser title marker and `elevate uc.exe` in `DEFAULT_APPS`) and `crates/hark-pipeline/src/meeting/mod.rs` (the "Elevate" display name).
- **Evidence:** on a live Elevate web-phone call in Chrome the window title was `Elevate UC - Google Chrome` and Chrome held the mic (ConsentStore `LastUsedTimeStop == 0`); after hang-up, with the tab still open and the title unchanged, Chrome released it. The desktop exe name is inferred from the product name "Elevate UC" and unverified.
- **Scope:** Meetings detection rules. No config or schema changes; `detect_apps` left at its default picks up the new entry.
- **Baseline policy:** global baseline `784272c` remains unchanged.
- **Validation:** `cargo fmt`, workspace `cargo clippy --all-targets -- -D warnings`, WSL Debian `cargo test --offline -p hark-meeting -p hark-pipeline` (98 + 114 passed; the native Windows test binary was blocked by App Control, os error 4551), `npm run check:docs`, and `npm run check:claude` pass.

## Scoped Gemini mock-server note — 2026-09-29 (0.57.5)

- **Source reviewed:** `9d8d183` (0.57.4), `crates/hark-stt/src/meeting_gemini.rs` tests only: the mock Files API server now sets each accepted socket to blocking.
- **Scope:** Transcription (Gemini Files adapter) and Meetings (cleanup paragraph) describe how the adapter is tested. No behavior, config, or schema changes.
- **Baseline policy:** global baseline `784272c` remains unchanged.
- **Validation:** `cargo fmt --check`, workspace `cargo clippy --all-targets -- -D warnings`, `cargo test -p hark-stt` (68 passed; the Gemini tests 20 times in a row with no failure), `npm run check:docs`, and `npm run check:claude` pass.

## Scoped meeting id note — 2026-09-29 (0.57.4)

- **Source reviewed:** `0f73a69` (0.57.3), `crates/hark-pipeline/src/meeting/coordinator.rs` only: `new_meeting_id` split into a clock read and `unique_in`, so its test no longer depends on the wall clock.
- **Scope:** Meetings storage paragraph now states how a meeting folder id is formed. No behavior, config, or schema changes.
- **Baseline policy:** global baseline `784272c` remains unchanged.
- **Validation:** `cargo fmt --check`, workspace `cargo clippy --all-targets -- -D warnings`, `cargo test -p hark-pipeline` (113 passed), `npm run check:docs`, and `npm run check:claude` pass.

## Scoped meeting prompt placement — 2026-09-29 (0.57.3)

- **Source reviewed:** `97d8686` (0.57.2), `crates/hark-app/src/meeting_prompt.rs` only: primary-monitor placement and the once-a-second always-on-top re-assert.
- **Scope:** Meetings (detection prompt paragraph) and Desktop UI (already updated in 0.57.2). No config, schema, or detection-rule changes.
- **Baseline policy:** global baseline `784272c` remains unchanged.
- **Validation:** `npm run check:docs` and `npm run check:claude` pass. Rust gates were run for 0.57.2 and are not rerun for this documentation-only change.

## Scoped optional meeting AEC release — 2026-09-28 (0.57.0)

- **Source reviewed:** `7b8bb126` plus the production Rust AEC3 wrapper, bounded recorder pairing, schema 6 settings/UI, tail and failure regressions, and notice packaging.
- **Scope:** setup, configuration, capture/meetings, architecture, desktop UI, notices, release packaging, and historical comparison context. New production modules are mapped in `_toc.yaml`; source-citation updates preserve generated-section boundaries.
- **Baseline policy:** global baseline `784272c` remains unchanged.
- **Validation:** both npm guards and WSL fmt/clippy/workspace tests passed; details below in `SUMMARY.md`. Real-speaker validation was explicitly deferred by the owner and is not claimed from synthetic or headphone evidence.

## Scoped MP3 finalization correction — 2026-09-28

- **Source reviewed:** `b0d45b5` plus the MP3 finalizer, mono bitrate, and exact-duration/ending-audio regressions.
- **Scope:** Audio Capture and Meetings describe complete end-of-file flushing, required gapless metadata, 40 kbps mono exports, and limitations of existing archives.
- **Baseline policy:** the existing global baseline and source mappings remain unchanged.
- **Validation:** the completed gate results are recorded in `SUMMARY.md`.

## Final Meetings Polish / AEC update — 2026-09-28 (0.56.0)

- **Source reviewed:** `7cbdc234` plus the standalone AEC experiment and final documentation reconciliation. Application source/tests are unchanged from the verified 0.55.0 snapshot.
- **Scope:** reproducible comparison, numeric evidence, real-speaker procedure, privacy/provider and capture-alignment clarification, authorization, delivery status, and lesson publication. Earlier per-feature release sections and verification records are preserved.
- **Baseline policy:** global baseline `784272c` remains; existing source mappings stay intact and the standalone tool is mapped to Meetings.
- **Validation:** standalone WSL/native-GNU checks and evidence audits are recorded in `SUMMARY.md`; synthetic quality, skipped AI review, native limitations, and pending production decisions remain explicit.

## Scoped Gemini meeting final-pass update — 2026-09-28 (0.55.0)

- **Source reviewed:** `3dfac02` plus only Polish item 5: explicit Gemini Files processing, schema 5, independent key/model settings, and window-scoped speaker labels.
- **Scope:** README/setup, Meetings, transcription, configuration, storage migration, desktop UI, overview, glossary, and shifted shared-source citations. Both new provider/worker modules are mapped. Deepgram saved-recording reruns remain explicit; no AEC benchmark/tool is added.
- **Baseline policy:** the global baseline and `_toc.yaml` ref remain unchanged.
- **Validation:** independent snapshot gates and HTTP cleanup fixtures are recorded in `SUMMARY.md`. Native policy restrictions and skipped AI review are distinguished from passing tests.

## Scoped meeting exports update — 2026-09-28 (0.54.0)

- **Source reviewed:** `6ee98fbe` (including the Windows hook ownership fix) plus only Polish item 4: subtitle timing, excerpts, Word export, and Windows text sharing.
- **Scope:** README/setup, overview, glossary, Meetings, audio capture, and desktop UI. The new export/encoding/share modules are mapped; changed-source citations use file links. Config schema remains 4; later Gemini speaker labels are excluded.
- **Baseline policy:** global baseline and `_toc.yaml` ref remain unchanged.
- **Validation:** independent snapshot and native harness results are recorded in `SUMMARY.md`.

## Scoped Windows shortcut build fix — 2026-09-28 (0.53.1)

- **Source reviewed:** `287df97` plus the tracker ownership fix discovered by Windows CI run `36464405873`.
- **Scope:** Audio Capture documents allocation during listener setup, outside key callbacks. The global baseline remains unchanged.
- **Validation:** native hotkey tests and local gates are recorded in `SUMMARY.md`; Windows CI must verify the lint fix.

## Scoped meeting shortcut update — 2026-09-28 (0.53.0)

- **Source reviewed:** `713434de` plus only Polish item 3, shared shortcut routing and config schema 4.
- **Scope:** Windows meeting shortcut setup, shared listener ownership, pure router invariants, migration/validation, and related source citations. Storage, Spellbook, and Voice Cleanup received citation-only updates where their shared source files shifted. No Gemini enum/model/schema-5 setting is included.
- **Baseline policy:** global baseline and `_toc.yaml` ref remain unchanged. The new pure router is mapped; source citations affected by shifted lines link to current files.
- **Validation:** independent snapshot gate results are recorded in `SUMMARY.md`.

## Scoped registry-driven detection update — 2026-09-28 (0.52.0)

- **Source reviewed:** `c0ae95b` plus only Polish item 2, registry notifications and deadline scheduling.
- **Scope:** Meetings detection and operational behavior, overview, watcher mapping, and the existing meeting crate policy. No hotkey, config-schema, export, Gemini, or AEC change.
- **Baseline policy:** the global baseline and `_toc.yaml` ref remain unchanged. Shifted detector/probe citations on the Meetings page use durable file links.
- **Validation:** independently checked snapshot results are recorded in `SUMMARY.md`.

## Scoped saved-meeting rerun update — 2026-09-28 (0.51.0)

- **Source reviewed:** `691d3fd` plus the feature-1-only saved-meeting rerun snapshot.
- **Scope:** README, overview, Getting Started, Meetings final-pass behavior, storage transactions, and glossary; the new worker and upload fixture are mapped in `_toc.yaml`. Shifted source references on these pages now link to files rather than stale line ranges.
- **Baseline policy:** the global baseline and `_toc.yaml` ref remain unchanged. Other Polish items are outside this snapshot.
- **Validation:** the independently checked snapshot results are recorded in `SUMMARY.md`.

## Scoped Meetings shipping update — 2026-09-28

- **Source reviewed:** `9a61d11d06056d6b54b9ad6f4cd8c4fb1f2fe654` plus the
  accompanying uncommitted README/privacy and plan edits for Part A.
- **Scope:** README, overview introduction, Getting Started, Glossary,
  documentation index, and Meetings privacy; no Rust behavior or config schema changes.
- **Baseline policy:** the global generation baseline below and
  `_toc.yaml`'s `ref_commit_hash` remain unchanged. This scoped review does not
  certify or skip the other pages changed since that baseline.
- **Validation:** recorded in `SUMMARY.md` after the shipping documentation checks.

## Global generation baseline

- **Commit:** `784272cbb488d15fa278f737963380e3121538c7`
- **Branch:** `main`
- **Generated:** 2026-09-24T20:15:37-04:00
- **Mode:** update (agent orientation plus legacy-page refresh)
- **Base commit:** `edda9d9df731374ab20faac8a9f2dc86c18ea8f3`
- **Pages generated:** 0 new
- **Sections regenerated:** 50 across 11 existing pages
- **Citation style:** repo-relative for regenerated material
- **Working tree:** dirty by design; this metadata records the source baseline at
  `HEAD`, while the documentation, agent guide, validation script, CI wiring,
  and one code-doc correction are the uncommitted output of this run.

## Run — 2026-09-24 (0.47.1, agent orientation and drift prevention)

This scoped run made the repository understandable without relying on a
Claude-only entrypoint and refreshed the pages most likely to mislead an agent
about the current product.

### Added

- Root `AGENTS.md`: tool-neutral product contract, trust hierarchy, runtime
  pipeline, workspace map, invariants, workflow, and verification commands.
- `scripts/check-docs-sync.mjs` plus the `npm run check:docs` command.
- A dependency-free CI documentation-drift job. It requires a mapped page to
  co-change when a mapped source changes after this baseline.

### Regenerated or corrected

- `OVERVIEW.md`: current provider stack, 15-crate layout, live/fallback path,
  and agent navigation.
- `core/ARCHITECTURE.md`: single-instance startup, current threading, Gemini
  live pump, bounded shutdown, current events, failure stages, and retry budget.
- `features/TRANSCRIPTION.md`: batch/live traits, gpt-transcribe, Deepgram,
  Gemini Live, fused cleanup, audio contracts, and error hygiene.
- `features/AUDIO_CAPTURE.md`: shortcut recording, Windows/Linux hooks,
  interception and lost-release handling, channel-0 capture, quiet-mic gate,
  and streaming reader.
- `features/INVOCATIONS.md`: removed the obsolete claim that provider-cleaned
  transcripts were not consumed; documented current Smart-mode behavior.
- `GETTING_STARTED.md`, `GLOSSARY.md`, and
  `operations/RELEASE_AND_PACKAGING.md`: repaired current prerequisites,
  provider vocabulary, v0.47 version snapshot, and documentation CI.
- `Docs/README.md`: current update summary and an explicit agent-guide route.
- `core/DATA_STORAGE.md`: current three-migration schema, invocation-aware word
  accounting, retention behavior, two-connection worker model, and 500 ms
  shutdown bound.
- `features/SPELLBOOK.md`: schema-v2 entries and exact aliases, Unicode-safe
  phonetic folding, tokenizer-aligned History selections, and the Whisper,
  gpt-transcribe, Deepgram, and Gemini Live vocabulary contracts.
- `features/VOICE_CLEANUP.md`: all 11 voices, Gemini cleanup inheritance,
  provider resolution, expansion/fail-open guards, fused Smart-mode behavior,
  and sanitized authentication errors.

### Scope boundary and follow-up

This was not a full-wiki rewrite. The first pass identified three mapped pages
whose sources had changed after the previous generated snapshot:

- `core/DATA_STORAGE.md` (`crates/hark-app/src/storage.rs`)
- `features/SPELLBOOK.md` (`hark-spellbook` public API and matcher)
- `features/VOICE_CLEANUP.md` (`hark-voice/src/error.rs`)

The follow-up pass regenerated all three and expanded their TOC ownership to
the current migrations, adapters, pipeline seams, configuration, and UI files.
That closes the explicitly recorded legacy refresh queue. Older pinned GitHub
citations remain on untouched wiki pages; reconciling citation style across the
entire wiki is still outside this scoped run.

### Validation contract

`check:docs` proves TOC/page ownership, baseline ancestry, and mapped
source/page co-change. It does not prove that prose is correct. Marker balance,
links, citations, Mermaid, and content review remain required parts of a
documentation sync.

### Validation

- `npm run check:docs`: pass; 16 pages mapped against `784272c`, 24 working-tree
  paths, and no silent mapped-source drift.
- `npm run check:claude`: pass; approximately 4,164 always-on tokens across two
  files, five scoped rules, and functional rule/hook wiring.
- `cargo test -p hark-store -p hark-spellbook -p hark-voice`: pass; 166 tests
  passed and no tests failed. Existing feature-gated `hark-stt` unused/dead-code
  warnings were emitted while compiling dependencies.
- `git diff --check`: pass.
- Documentation structure: pass; 83 balanced AUTOGEN pairs matched all 83
  generated TOC sections, 683 relative links resolved, 512 current-tree line
  citations were in bounds, and 14 Mermaid blocks had recognized static openers.
- `mmdc` is not installed, so Mermaid render validation was not performed.

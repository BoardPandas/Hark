# Generation Metadata

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

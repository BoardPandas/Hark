# Generation Summary

- **Mode:** init
- **Commit:** `6a33396` (branch `main`)
- **Generated:** 2026-07-17T16:32:04-04:00
- **Docs root:** `Docs/`
- **Pages:** 14 generated / 14 planned in `_toc.yaml`
- **Sections:** 76 AUTOGEN sections
- **Source citations:** 1036 (all converted to absolute GitHub blob URLs pinned to the generation commit)

## Pages

| Page | Sections | Citations | Diagrams |
|------|----------|-----------|----------|
| OVERVIEW.md | 6 | 90 | 2 |
| core/ARCHITECTURE.md | 6 | 87 | 3 |
| GETTING_STARTED.md | 6 | 32 | 0 |
| core/CONFIGURATION.md | 6 | 85 | 0 |
| core/DATA_STORAGE.md | 6 | 80 | 1 |
| features/AUDIO_CAPTURE.md | 6 | 94 | 1 |
| features/TRANSCRIPTION.md | 6 | 92 | 1 |
| features/SPELLBOOK.md | 6 | 71 | 1 |
| features/VOICE_CLEANUP.md | 5 | 66 | 1 |
| features/TEXT_INJECTION.md | 5 | 55 | 1 |
| features/UPDATES_AND_AUTOSTART.md | 5 | 72 | 1 |
| features/DESKTOP_UI.md | 6 | 100 | 0 |
| operations/RELEASE_AND_PACKAGING.md | 5 | 44 | 1 |
| GLOSSARY.md | 2 | 68 | 0 |

## Validation

- **Structure:** PASS. Every page has exactly one `PAGE_ID` as its first line and matches `_toc.yaml`. All 76 `BEGIN:AUTOGEN`/`END:AUTOGEN` pairs are balanced and their IDs match the TOC. No stray or duplicated markers.
- **Internal links:** PASS. All page-to-page links (Related Pages, README index) resolve to existing files.
- **Source citations:** PASS. All 1036 evidence links resolved to absolute GitHub blob URLs at commit `6a33396`; no dangling repo-relative citation links remain.
- **Mermaid:** PASS (static). `mmdc` was not on PATH, so diagrams were validated statically: 14 blocks across 10 pages, every opening line is a valid diagram type, all flowcharts use `graph TD`, and no unquoted node labels or pipe-less edge labels were found. No syntactic (`mmdc`) validation was performed.

## Coverage

All 13 workspace crates are documented, plus the config, installer, release workflow, and root manifests:

| Source area | Documented in |
|-------------|---------------|
| `crates/hark-app` (main loop, tray, overlay, egui UI, storage glue, update glue) | ARCHITECTURE, DESKTOP_UI, DATA_STORAGE, UPDATES_AND_AUTOSTART |
| `crates/hark-pipeline` | ARCHITECTURE |
| `crates/hark-audio` + `crates/hark-hotkey` | AUDIO_CAPTURE |
| `crates/hark-stt` | TRANSCRIPTION |
| `crates/hark-spellbook` | SPELLBOOK |
| `crates/hark-voice` | VOICE_CLEANUP |
| `crates/hark-inject` | TEXT_INJECTION |
| `crates/hark-config` + `crates/hark-keychain` | CONFIGURATION |
| `crates/hark-store` | DATA_STORAGE |
| `crates/hark-update` + `crates/hark-autostart` | UPDATES_AND_AUTOSTART |
| `installer/`, `.github/workflows/release.yml`, `.github/RELEASING.md`, `package.json`, `Cargo.toml` | RELEASE_AND_PACKAGING |

### Notes and known gaps

- The SPELLBOOK builder found that the biasing/`Spellbook` settings logic it was pointed at does not live in `crates/hark-spellbook` (which only implements the phonetic `Corrector`); it traced the real implementation to `crates/hark-config`, `crates/hark-pipeline`, and `crates/hark-stt` and cited those instead. The page documents the cross-crate split explicitly rather than citing a nonexistent struct.
- Test files (`crates/*/tests/**`, `examples/**`) are cited where they illustrate observed behavior but are not documented as standalone pages.
- macOS-specific code paths (CGEventTap hotkey, login item, macOS update link-out) are documented from the current source; several are Windows-first with macOS paths noted where the code marks them incomplete.
- No API-reference, database-service, auth, or hosting/infra pages exist: Hark is a native single-process desktop app with no web backend.

## Regeneration

Run `/doc-sync update` after code changes to regenerate only the AUTOGEN sections whose source files changed. Manual edits between AUTOGEN markers are preserved.

---

## Incremental Update — 2026-07-22 14:26

- **Mode:** update (scoped to the Invocations feature)
- **Commit range:** `1c17387..bcfcc3f`

### Phase A — TOC drift
- New pages: 1
  - `hark_08b_invocations` → `features/INVOCATIONS.md` (6 sections, 1 flowchart)
- Removed pages: 0
- Added sections: 6 (all on the new page)
- Removed sections: 0

### Phase B — Source diff
- Sections regenerated: 6 across 4 existing pages
  - `hark_12_desktop_ui_overview` — was "four egui pages (History, Spellbook, Stats, Settings)"; now five, with Invocations
  - `hark_12_desktop_ui_pages` — page table gains the Invocations row plus an Invocations subsection
  - `hark_08_spellbook_matcher` — `window_matches` now takes the Jaro-Winkler threshold as a parameter; documents the 0.85 vs 0.90 split and `window_similarity`
  - `hark_08_spellbook_api` — `Expander`, `phrase_word_count`, and `normalized_phrase` added to the public API table
  - `hark_04_configuration_schema` — `[[invocations.entries]]` keys, the last-field ordering rule, and the deliberate absence of a `validate` rule
  - `hark_05_data_storage_schema` — migration 003, the `invocation` column, the ER diagram, and the spoken-word stats rule
- Pages touched: 5 (1 new, 4 edited)
- `Docs/README.md`: Invocations added to the Features table; "Latest Updates" refreshed through v0.20.0

### Validation
- Structure errors: 0 (every PAGE_ID and BEGIN/END pair matches `_toc.yaml`)
- Broken internal links: 0 across all 15 pages (16 as of 0.35.5, with `ON_DEVICE_STT.md` registered)
- Mermaid: 14 blocks, 0 invalid (static check; `mmdc` not installed, so no render test was performed)
- Citations emitted this run: verified against `git show bcfcc3f:<path>` — 0 nonexistent paths, 0 out-of-range line numbers

### Known coverage gaps

This run was **deliberately not** a full `1c17387..bcfcc3f` refresh. That range
spans six releases; regenerating everything would rewrite most of the wiki in a
single unreviewable diff. The following remain stale or undocumented and need
their own `/doc-sync update` run:

| Release | Undocumented work | Affected page |
|---|---|---|
| 0.15.0 | `over_expanded` guard and `LENGTH_DISCIPLINE_CLAUSE` | VOICE_CLEANUP |
| 0.16.0 | `hark-single-instance` crate | none — no page exists |
| 0.17.0 | `hark-audio/src/gain.rs`, peak-window silence gating, live input meter | AUDIO_CAPTURE |
| 0.18.0 | `hark-local-stt` crate (5 modules), `hark-pipeline/src/local.rs`, `hark-config/src/local.rs` | ON_DEVICE_STT — **closed 0.35.5**: the hand-written page is now registered in `_toc.yaml` as `hark_07b_on_device_stt` and carries citations |
| 0.18.1 | multi-monitor overlay placement | DESKTOP_UI |

26 of 83 non-test Rust source files are not matched by any `_toc.yaml` source
pattern, concentrated in `hark-local-stt`, `hark-single-instance`, and the
`hark-app/src/ui/settings/*` submodules.

### Coverage gaps since 0.19.0 (appended 2026-08-21, no regeneration performed)

The table above stopped at 0.18.1 and was not extended for the next **37
commits**, so a reader trusting it as complete would have understated the gap by
17 releases. Extended here from `git log bcfcc3f..HEAD`; still no regeneration —
that remains a separate `/doc-sync update` run.

| Releases | Undocumented work | Affected page |
|---|---|---|
| 0.21.0–0.21.3, 0.23.1 | The Nocturne restyle, and the recording overlay becoming a real frameless, shape-clipped, transparently-composited window | DESKTOP_UI |
| 0.22.0, 0.24.0 | Four added cleanup voices, the no-dashes rule, and the Grammar voice | VOICE_CLEANUP |
| 0.23.0 | Trailing period suppressed on single-word dictations | VOICE_CLEANUP |
| 0.25.0, 0.29.1, 0.30.0, 0.30.4, 0.30.5 | Window/pill focus and surfacing behaviour, including the OS-level main-window kick | DESKTOP_UI |
| 0.26.0 | Dictionary renamed to Spellbook (terminology sweep) | SPELLBOOK, GLOSSARY |
| 0.27.0–0.27.1 | Transcript text selection and snapping in History | DESKTOP_UI |
| 0.28.0–0.29.0 | Adding Spellbook terms from a history selection; Spellbook aliases and the **schema v2 migration** | SPELLBOOK, **DATA_STORAGE** |
| 0.30.1–0.30.3 | `ci.yml` added (fmt/clippy/test gate), the changelog hook unsilenced | RELEASE_AND_PACKAGING |
| 0.30.2 | Per-page scrollbars in the Spellbook | DESKTOP_UI |
| **0.31.0–0.35.2** | **The whole push-to-talk shortcut overhaul** — shortcut recording, `hark-hotkey/src/edges.rs`, `capture.rs`, `keycode.rs`, `known.rs`, all 114 keys bindable, trap-key refusal, conflict reporting, lock-key suppression (`swallow_lock_keys`) | AUDIO_CAPTURE (the only page citing `hark-hotkey`) |
| 0.35.1 | Icon font stack ordering | DESKTOP_UI |
| 0.35.3, 0.35.5 | Release gated on the repo's own checks; SHA-pinned actions; `cargo audit` in CI | RELEASE_AND_PACKAGING |

**Highest-value next run:** `hark-hotkey`. It is 3,870 LOC — the second-largest
crate — it changed in five of the last eight releases, and `AUDIO_CAPTURE.md`
still describes the pre-0.31 model in which the shortcut was not user-recordable.

### Pre-existing issue (not introduced here, not corrected)

Whole-file citations written by the original `init` run are off by one
(`#L1-L{lines+1}`), e.g. `tray/mod.rs#L1-L236` where the file has 235 lines at
`1c17387`. GitHub clamps such ranges, so they render correctly. They sit in
untouched AUTOGEN blocks and in "Relevant source files" lists whose file sets
did not change, so the incremental-update policy forbids rewriting them here.
Citations emitted by this run use exact line counts.

---

## Incremental Update — 2026-09-10 17:17

- **Mode:** update (scoped)
- **Commit range:** `bcfcc3f..edda9d9`
- **Trigger:** 0.39.0 removed the portable Windows download and changed the
  in-app updater to run the signed Inno installer instead of self-replacing the
  running exe, falsifying four sections.

### Phase A — TOC drift

- New pages: 0
- Removed pages: 0
- Added sections: 0
- Removed sections: 0
- `project.ref_commit_hash` and `updated_at` advanced to `edda9d9` / 2026-09-10.

### Phase B — Source diff

- Sections touched: 6, across 3 pages — 4 fully regenerated, 2 surgically
  corrected.

| Page | Section | Was wrong because |
|---|---|---|
| `GETTING_STARTED.md` | `hark_03_getting_started_install` | Told readers a portable exe is attached to each release |
| `features/UPDATES_AND_AUTOSTART.md` | `hark_11_updates_autostart_overview` | "ships as a single signed portable `.exe` … updates itself in place", and a four-stage lifecycle ending "swap the exe and relaunch" |
| `operations/RELEASE_AND_PACKAGING.md` | `hark_13_release_packaging_overview` | Described a single-job workflow publishing installer + portable exe |
| `operations/RELEASE_AND_PACKAGING.md` | `hark_13_release_packaging_workflow` | Every citation derived from a 223-line `release.yml`; the file is now 704 lines across four jobs, so the line numbers pointed at unrelated code |
| `features/UPDATES_AND_AUTOSTART.md` | `hark_11_updates_autostart_checker` | Named the old `-windows-x64.exe` asset suffix |
| `features/UPDATES_AND_AUTOSTART.md` | `hark_11_updates_autostart_appglue` | Described `restart()` calling `hark_update::apply` then `hark_update::relaunch` — both functions no longer exist |

**Two of these were not in the requested set.** The brief named four sections;
`_checker` and `_appglue` were found during validation, and both are covered by
the same Phase B rule — their `source_files` include `crates/hark-update/src/lib.rs`
and `crates/hark-app/src/update.rs`, which changed. `_appglue` was the worse of
the two: it documented the call sequence of two deleted functions, which is the
kind of claim a reader would act on.

The workflow section was the expensive one. It was not merely worded wrong — its
whole citation set had rotted, because `release.yml` was restructured into
`version` / `release` / `linux` / `linux-arch` and more than tripled in length.
Every step's range was re-derived from the current file rather than adjusted.

### Validation

- Structure errors: 0. All AUTOGEN markers across `Docs/` balanced and in order.
- Internal links: 102 checked across `Docs/`, 0 broken.
- Citations: 57 repo-relative citations verified — each resolves to a file that
  exists, with a line span inside it and no inverted ranges. 36 pinned absolute
  URLs in the two surgically corrected blocks were left untouched by design.
  Two defects were caught by this check rather than by review:
  - **An off-by-one.** A two-line header edit to `release.yml` shifted a
    PowerShell snippet, and the cited range had drifted a line off the code it
    quoted.
  - **A wrong base for every new citation.** They were first written
    repo-root-relative (`crates/...`), which is how the paths are written in
    prose but not how markdown resolves them — from `Docs/features/` that
    points at `Docs/features/crates/...`. All 52 were rewritten to page-relative
    (`../../crates/...`), matching the convention already used in
    `ON_DEVICE_STT.md`. A link check that only looked at `.md` targets missed
    this; checking every citation target is what surfaced it.
- Mermaid: 1 diagram regenerated (the update lifecycle, now ending in an install
  rather than a swap). `mmdc` is not on PATH, so it was checked statically per
  `references/mermaid-policy.md`: `graph TD`, every node and edge label quoted,
  no special characters in node IDs. Syntactic validation was skipped.
- Coverage gap: 0 new source files unmapped by the TOC.

### Known gap left open, deliberately

Citation style is now mixed across the wiki: this run emitted repo-relative
links (the policy's preferred form), while untouched sections still carry
absolute blob URLs pinned to `1c17387` or `bcfcc3f`. Those older links are not
wrong — each points at the code its section was written from — but the wiki will
read inconsistently until a full run reconciles it.

`_checker` and `_appglue` are mixed *within* a single block, which the citation
policy ("pick one style per page") would not choose deliberately. It is the
honest option here: the surrounding sentences are still accurate, and their
pinned URLs point at the code those sentences were written from. Rewriting them
to repo-relative without re-deriving every line number would silently repoint
them at unrelated current code — a worse outcome than visible inconsistency.
They should be fully regenerated on the next unscoped run.

The "Highest-value next run" note above (`hark-hotkey` / `AUDIO_CAPTURE.md`)
still stands and is unaffected by this run.

---

## Incremental Update — 2026-09-24 19:41 (0.47.1)

- **Mode:** update (scoped agent-orientation refresh)
- **Source baseline:** `784272cbb488d15fa278f737963380e3121538c7`
- **Previous generated baseline:** `edda9d9df731374ab20faac8a9f2dc86c18ea8f3`
- **Pages added:** 0
- **Generated sections refreshed:** 32 across 8 pages

### Outcomes

- Added a root, tool-neutral `AGENTS.md` that explains the product, trust
  hierarchy, current runtime pipeline, 15-crate workspace, hard invariants,
  normal workflow, and verification commands.
- Refreshed Overview, Architecture, Transcription, Audio Capture, Invocations,
  Getting Started, Glossary, and Release/Packaging where v0.40-v0.47 had made
  important claims false or incomplete.
- Added a dependency-free documentation source-map guard, exposed as
  `npm run check:docs` and run by CI with full Git history.
- Updated `_toc.yaml` ownership for Gemini Live, gpt-transcribe, Linux hotkeys,
  shortcut capture, the live-stream pump, the CI workflow, the tool-neutral
  guide, and the single-instance crate.

### Validation

- `npm run check:docs`: PASS. 16 mapped pages checked against `784272c`; 21
  working-tree paths and no silent mapped-source drift.
- `npm run check:claude`: PASS. Approximately 4,164 always-on tokens across two
  files; five scoped rules; rule and hook wiring functional.
- `cargo fmt --all -- --check`: PASS.
- `git diff --check`: PASS.
- Structure and links: PASS. 82 AUTOGEN pairs balanced; all 82 generated TOC
  section IDs matched their markers; four hand-written on-device sections were
  intentionally marker-free; 452 relative links resolved.
- Citations: PASS for 281 current-tree relative line citations; every target
  existed and every line range was in bounds. Older pinned GitHub citations in
  untouched sections were not repointed.
- Mermaid: 14 blocks passed static opening checks. `mmdc` was not installed, so
  no syntactic render validation was performed.

### Known historical gaps retained

The old-baseline audit found three pages whose mapped source changed without a
full semantic refresh: `core/DATA_STORAGE.md`, `features/SPELLBOOK.md`, and
`features/VOICE_CLEANUP.md`. They are intentionally carried as the next refresh
queue. The new baseline and CI guard prevent additional silent drift; they do
not retroactively certify those pages.

The original init summary near the top of this file records a 13-crate/14-page
snapshot. It is historical. The current TOC records 15 crates and 16 pages.

---

## Incremental Update — 2026-09-24 20:15 (0.47.1)

- **Mode:** update (legacy-page refresh follow-up)
- **Source baseline:** `784272cbb488d15fa278f737963380e3121538c7`
- **Pages added:** 0
- **Generated sections refreshed:** 18 across 3 pages

### Outcomes

- Regenerated `core/DATA_STORAGE.md` against the current store and app worker,
  including migration 003, spoken-word accounting for invocations, and the
  bounded storage shutdown path.
- Regenerated `features/SPELLBOOK.md` against schema-v2 entries/aliases, the
  Unicode-safe matcher, selection snapping, pipeline construction, and all
  current provider vocabulary contracts.
- Regenerated `features/VOICE_CLEANUP.md` against the 11 current voices,
  provider resolution (including Gemini), pipeline skip/guard behavior,
  fused Smart results, and auth-error sanitization.
- Expanded `_toc.yaml` ownership so future changes to the migrations,
  provider adapters, configuration, pipeline seams, or relevant UI code must
  co-change the page they affect.
- Closed the three-page legacy refresh queue recorded by the preceding run.

### Validation

- `npm run check:docs`: PASS. 16 mapped pages checked against `784272c`; 24
  working-tree paths and no silent mapped-source drift.
- `npm run check:claude`: PASS. Approximately 4,164 always-on tokens across two
  files; five scoped rules; rule and hook wiring functional.
- `cargo test -p hark-store -p hark-spellbook -p hark-voice`: PASS. 166 tests
  passed; 0 failed. Compilation emitted two existing feature-gated `hark-stt`
  warnings.
- `git diff --check`: PASS.
- Structure and links: PASS. 83 AUTOGEN pairs matched all 83 generated TOC
  sections; 683 relative links resolved; 512 current-tree line citations
  existed and were in bounds.
- Mermaid: 14 blocks passed static opener checks. `mmdc` was unavailable, so
  syntactic render validation was not performed.

---

## Scoped Update — 2026-09-28 (Meetings shipping preparation)

- **Source reviewed:** `9a61d11d06056d6b54b9ad6f4cd8c4fb1f2fe654` and this
  working tree's README/privacy and plan edits. Release 0.50.5 is not yet cut.
- **Outcome:** README features/privacy and the mapped Overview, Getting Started,
  and Glossary pages distinguish dictation from Meetings. The Meetings privacy
  section and documentation index now describe actual start modes, audio scope,
  local paths/cap/deletion, and separate live/final/summary provider requests.
- **Verified distinction:** on-device Primary keeps live chunks local; it does
  not disable Deepgram refinement or LLM summaries. Manual start retains broad
  playback capture when it adopts a call for auto-stop.
- **Baseline:** retained `784272cbb488d15fa278f737963380e3121538c7` in both
  `GENERATION.md` and `_toc.yaml`; a scoped privacy review does not certify the
  rest of the wiki or advance its global source baseline.

### Verification

- `npm run check:claude`: PASS; rule scoping and hook wiring functional,
  approximately 4,216 always-on tokens, five rules.
- `npm run check:docs`: PASS after updating all three pages mapped to README;
  17 mapped pages checked against `784272c`, no silent mapped-source drift.
  The first run correctly identified missing Getting Started/Glossary updates.
- WSL Debian `cargo test -p hark-store --test meetings`: PASS; 13 passed,
  zero failed. Covers existing speaker rename, FTS search/refinement, and deletion.
- `git diff --check`: PASS.
- Full Rust formatting, workspace clippy, and workspace tests were not run for
  these documentation-only changes. Existing CI for `9a61d11` passed on
  Windows, Linux, and macOS; this is evidence for that commit, not a new build.
- No native call test, installer execution, or Mermaid render was performed.
  User-reported Teams/Meet coverage is preserved in the plan; the handoff's
  0.50.4 Meet prompt/auto-stop retest remains pending.
- Approved LL-G publication completed in commit `1a743d3`: five new lessons, one
  App Control update, five shelf indexes, and the master index. All 12 files were
  verified through the GitHub API. LL-G CI reports the same existing blank-line
  failures in its PowerShell and TypeScript indexes as parent `dd8a331`; neither
  file changed in this publication. This does not invalidate Hark's passing guards.
- Release preparation synchronized `package.json`, the Cargo workspace, and all
  16 Hark lockfile package versions to 0.50.5 with `cargo update --workspace
  --offline`; no third-party dependency versions changed. Both Hark npm guards
  passed again, and the 13 meeting-store tests passed on the 0.50.5 working tree.
- The existing `v0.50.4` Release workflow completed successfully, including the
  Windows signed installer, Linux packages, and Arch package. No 0.50.5 commit,
  push, tag, or release has occurred during this preparation.


## Scoped saved-meeting rerun validation — 2026-09-28 (0.51.0 review snapshot)

- **Source:** `691d3fd` plus only Polish item 1, assembled in `C:/Users/chaz/.codex/worktrees/meeting-rerun/Hark`. The root checkout's other Polish changes are outside this snapshot.
- **Documentation:** README, Getting Started, overview, glossary, Meetings, and data storage now describe the confirmed Deepgram rerun and transactional replacement. The new worker and HTTP fixture are mapped; shifted citations in the edited pages use file links. The global documentation baseline remains unchanged.
- **Gates passed:** `npm run check:claude`; `npm run check:docs` (17 mapped pages, no silent mapped-source drift); WSL Debian `cargo fmt --all -- --check`; `cargo clippy --offline --all-targets -- -D warnings`; `cargo test --offline --workspace` (**957 passed, 0 failed, 1 ignored** across 41 test suites, including doc-test suites).
- **Evidence:** the tests cover unchanged MP3 upload bytes/content type, path traversal and Unix symlinks, worker protection release, transactional transcript/FTS replacement with speaker reset, rollback preserving previous state, and failed-save acknowledgement. The ignored test requires the downloaded on-device model.
- **Limits:** these Linux checks do not establish native Windows UI, audio capture, keychain, or real-provider behavior. This snapshot has not been committed, pushed, packaged, or released. `cargo update --workspace --offline` changed only the 16 local workspace package versions; no external dependency was added or updated. Neither case-colliding agent guide was included.


## Scoped registry-driven detection validation — 2026-09-28 (0.52.0 snapshot)

- **Source:** `c0ae95b` plus only Polish item 2 in `C:/Users/chaz/.codex/worktrees/meeting-rerun/Hark`. The prior staged tree was verified identical to `c0ae95b` before a soft reset aligned the reused worktree. Item 1 remains intact; the coordinator's later meeting-toggle change is excluded.
- **Documentation:** Meetings describes recursive notifications, rearming, fresh debounce/auto-stop snapshots, ten-second backstop, two-second polling fallback, 30-second watcher retry, and explicit shutdown. Overview, the source map, and crate policy match that scope; the global baseline remains unchanged.
- **Gates passed:** `npm run check:claude`; `npm run check:docs` (17 mapped pages, no silent mapped-source drift); WSL Debian `cargo fmt --all -- --check`; `cargo clippy --offline --all-targets -- -D warnings`; `cargo test --offline --workspace` (**967 passed, 0 failed, 1 ignored**, 41 suites including doc-tests).
- **Native Windows fixture check passed:** `cargo test --offline -p hark-meeting probe_win::watch::win::tests -- --nocapture` (**2 passed, 0 failed**, 93 filtered out). These tests create isolated temporary HKCU keys, verify recursive change/rearm/delete behavior, callback-sender retirement, and missing-root handling, then clean up those test keys.
- **Limits:** registry fixtures do not establish real-call detection, capture, or UI behavior. The ignored Linux test requires downloaded local-STT model weights. No external package was added or updated; the offline lock sync changed 16 workspace version entries to 0.52.0. No agent-guide path is included. The parent task owns the authorized commit, push, and tag operations.

Feature-1 delivery update reported by the parent task on 2026-09-28: `c0ae95b` was committed and pushed; Windows/Linux/macOS CI passed in run `36462226486`; `v0.51.0` was pushed and the release workflow started. This records verified CI and a started release, not release completion.


## Scoped shared meeting shortcut validation — 2026-09-28 (0.53.0 snapshot)

- **Source:** `713434de` plus only Polish item 3 in the reused managed worktree. The prior staged tree was verified identical to the feature-2 commit before a soft reset. Features 1–2 remain intact; the copied config and settings UI were split to schema 4 with no Gemini final-pass enum, model field, model validation, or schema-5 migration.
- **Documentation:** README/setup, Meetings, configuration, architecture, input capture, desktop UI, overview, glossary, and the hotkey crate policy describe the optional binding, shared listener, conflicts, and best-effort backup/save migration. Data Storage, Spellbook, and Voice Cleanup received citation-only updates for changed shared source files. The global documentation baseline remains unchanged.
- **Gates passed:** `npm run check:claude`; `npm run check:docs` (17 mapped pages, no silent mapped-source drift); WSL Debian `cargo fmt --all -- --check`; `cargo clippy --offline --all-targets -- -D warnings`; `cargo test --offline --workspace` (**978 passed, 0 failed, 1 ignored**, 41 suites including doc-tests).
- **Native Windows tests passed:** `cargo test --offline -p hark-hotkey -p hark-config` (**173 passed, 0 failed, 0 ignored**). Fixtures cover conflict ordering/subsets, repeat and injected-event suppression, meeting-only engage edges, release healing/watchdog behavior, and schema-4 provider preservation, backup, persistence, and round trip. App routing fixtures in the workspace suite cover a missing or busy dictation worker and disabled/unsupported Meetings.
- **Limits:** these fixtures do not establish a physical global shortcut or real meeting capture. The ignored local-STT test requires downloaded model weights. The offline lock sync changed 16 workspace versions to 0.53.0 and added the internal config → hotkey dependency; no external package version changed. Agent-guide paths are excluded. The parent owns commit, push, and tag operations.


## Windows shortcut build correction — 2026-09-28 (0.53.1)

Windows CI for `287df97` found `clippy::large_enum_variant` in `HookState`:
the new dual-chord tracker enlarged one variant to 320 bytes. It now lives in
a box allocated once before hook installation; callbacks borrow the same state.
Native Windows `cargo test --offline -p hark-hotkey` passed **87 tests, zero
failures**. This is a layout/ownership correction; shortcut routing is unchanged.
The parent will verify Windows CI before creating the shortcut release tag.
The AI review workflows for 0.50.5 through 0.53.0 skipped their review because
`ANTHROPIC_API_KEY` was absent; their green workflow status is not a code review.


## Scoped meeting export validation — 2026-09-28 (0.54.0 snapshot)

- **Source:** `6ee98fbe` plus Polish item 4 in the managed worktree. Previous features and config schema 4 remain; Gemini window-scoped labels and provider work are excluded. The snapshot includes subtitle timing, line/time excerpts, DOCX, and native Windows text sharing. Export tests were run before the Windows-only 0.53.1 ownership fix was incorporated; the incorporated fix passed native Windows `cargo test --offline -p hark-hotkey` (**87 passed, 0 failed**) and a new format check.
- **Gates:** WSL Debian `cargo fmt --all -- --check`; `cargo clippy --offline --all-targets -- -D warnings`; `cargo test --offline --workspace` (**986 passed, 0 failed, 1 ignored**, 41 suites including doc-tests). The local-STT fixture requiring downloaded weights is ignored. Both npm guards pass after the mapped documentation updates.
- **Native verification:** Windows `cargo test --offline -p hark-audio -p hark-meeting` passed **200 tests, 0 failed**. A standalone native share harness compiled the snapshot's actual WinRT source and exercised its actual Word and excerpt modules plus copied pure export module: **28 passed, 0 failed**. DOCX tests inspect escaping/styles/Unicode/line breaks and reopen packed output; audio tests verify exact samples across chunks, MP3 decode-before-cut, EOF clipping, and preservation of existing output on failure.
- **Harness limits:** save dialogs, file operations, and audio calls are stubbed in the share harness; their real audio implementations are covered by the separate crate tests. It does not open a share chooser or save dialog, test real capture, or establish interactive native behavior.
- **Dependency resolution:** approved `docx-rs` 0.4.22 has default features disabled. The initial offline resolver hit a cached wasm-bindgen/js-sys conflict; seeding the already-resolved root lock and rerunning `cargo update --workspace --offline` succeeded. External additions are only docx-rs 0.4.22, zip 8.6.0, and typed-path 0.12.3; existing package versions are unchanged, with dependency-name disambiguation for the two zip versions. Agent guides are excluded; parent owns commit/push/tag operations.


## Scoped Gemini final-pass validation — 2026-09-28 (0.55.0 snapshot)

- **Source:** `3dfac02` plus Polish item 5 in the managed worktree. Features 1–4 and the 0.53.1 hook fix remain intact. Schema 5 adds Gemini/model defaults without switching existing providers; both `none` and `deepgram` migrations from versions 3 and 4 are covered. The provider-neutral detail label is included. Saved-recording re-runs remain explicitly Deepgram.
- **Gates passed:** WSL Debian `cargo fmt --all -- --check`; `cargo clippy --offline --all-targets -- -D warnings`; `cargo test --offline --workspace` (**1,002 passed, 0 failed, 1 ignored**, 41 suites including doc-tests). Both npm guards pass. The ignored local-STT fixture needs downloaded model weights.
- **HTTP and pure fixtures:** tests cover per-track windows and the partial tail; aborting a failed window; energetic leading/trailing/internal omissions; clamped timestamps and window-scoped speaker IDs; disabled interaction storage; same-origin upload validation; unique requested file names; DELETE after success, inference failure, malformed/missing/mismatched finalization metadata, and a lost finalization reply. New fixtures establish visible cleanup failure with one retry and harmless DELETE 404 after successful inference.
- **Native results and limitation:** `cargo test --offline -p hark-stt -p hark-config -p hark-meeting` completed **254 unit tests, 0 failed** across the three libraries, including all Gemini HTTP fixtures. The overall command then failed to launch the STT `adapter_pure` integration executable because Windows Application Control blocked it (`os error 4551`); that executable never ran natively. The same integration suite passed in WSL. This is not a passing full native gate or a live Gemini request test.
- **Scope and delivery:** offline lock sync changes only the 16 local package versions to 0.55.0, with no external dependency changes. New adapter/worker sources are mapped; affected citation offsets were replaced with file links. No AEC tooling or agent-guide changes are included. The parent owns commit/push/tag and CI/release verification. Prior AI-review workflows skipped review because `ANTHROPIC_API_KEY` was absent; their successful workflow status must not be described as a completed code review.


## Final Meetings Polish / AEC validation — 2026-09-28 (0.56.0 snapshot)

- **Source and scope:** `7cbdc234` plus the standalone `tools/meeting-aec-bakeoff` package, numeric evidence, and documentation. Application crates/tests are byte-for-byte unchanged from feature 5; its WSL clippy/workspace result (**1,002 passed, 0 failed, 1 ignored**) remains the source verification. This snapshot does not rerun unchanged application tests or add experimental dependencies to its lockfile.
- **Repeated standalone checks:** `build-wsl.py test --release --locked --offline --all-features --jobs 2` passed **4 tests, 0 failed**; strict all-targets release clippy passed. Native Windows GNU `cargo test --release --locked --offline --no-default-features --features rust-aec3 --target x86_64-pc-windows-gnu --jobs 1` passed **4 tests, 0 failed**. Both reused the original tool's existing build targets; its sources match the snapshot. Python syntax and standalone/application formatting checks passed.
- **Evidence audit:** JSON and CSV each contain **63 measurements**, seven fixtures × three engines × three repetitions. Each fixture has the same input fingerprint across engines/repetitions, all sample rates are 16 kHz, and reported metrics are finite. The numeric evidence is copied unchanged. The previous full timed comparison and native seven-fixture replay are described in [RESULTS.md](../../tools/meeting-aec-bakeoff/RESULTS.md); no new timing run was performed during release preparation.
- **Documentation:** reconciled current provider choices, scoped speaker IDs, notes independence, spellbook behavior, bounded decoding, and AEC alignment constraints. Preserved the shared-hook/boxed-tracker details, export guards, schema-5 migration behavior, and per-feature evidence. Removed stale separate-approval language and kept AI review marked skipped when its key was absent.
- **Delivery and limits:** Part A 0.50.5 is fully released; feature delivery and verified lesson publication are tracked in [the plan](../../tasks/2026-09-26-plan-meeting-transcription.md). No production AEC engine is selected or enabled. Native C++/MSVC, real speaker listening, capture alignment/drift integration, and manual Meetings behavior remain separate validation. This preparation does not claim an app commit/push/tag, a live provider call, or a completed AI code review.

- **Verified lesson publication:** LL-G [commit `9ae35d63`](https://github.com/BoardPandas/LL-G/commit/9ae35d63e61c1140a0794b1c3d915777f314d8c8) atomically published three new HIGH entries, one HIGH amendment, and their indexes in eight files (+258/−5). All eight remote blobs and six Hark source paths were verified. Snapshot index counts pass; the same three pre-existing blank-line errors remain in structural checks. LL-G CI run `36468052579` failed on those same three baseline issues, with no new errors; its later count step was skipped. The independent full-snapshot count check passed (719 entries). Publication remains verified; no LL-G tag was created.
- **Documentation audit:** 61 balanced AUTOGEN pairs, 838 relative links resolved, and 332 current-file line citations within bounds across changed/new pages. Nineteen reversed/out-of-bounds inherited citations were replaced with file links. This verifies structure and targets, not Mermaid rendering or exhaustive semantic coverage.

- **Delivery verified before the final snapshot:** features 1–5 are committed and pushed separately with passing Windows/Linux/macOS CI and release tags. Feature 3 ships as `v0.53.1` after its Windows layout fix; `v0.53.0` remains untagged. Feature 5's exact SHA `7cbdc234` passed all eight CI jobs in run `36467977562`, and annotated `v0.55.0` was pushed. Its release workflow was triggered, but completion is not yet verified. Part A `v0.50.5` and features 1–2 have completed full releases. Feature 6 remains the prepared snapshot described here until the parent performs its commit/push/tag.

## MP3 finalization correction — 2026-09-28 (0.56.1)

- Corrected end-of-file flushing preserves buffered PCM and writes gapless timing metadata. Mono exports use 40 kbps because the tag cannot fit a 32 kbps frame at 16 kHz. Stereo archives remain 64 kbps; legacy audio is not rewritten.
- Reproduced the original failure before the fix. Independent synthetic comparison verified exact lengths for 12 input sizes with the corrected finalizer. Production regressions cover stereo/mono frame and chunk boundaries, the last 50 ms of audio, short excerpts, and missing metadata rejection.
- WSL Debian: `cargo fmt --all -- --check` and `cargo clippy --all-targets -- -D warnings` passed; `cargo test --workspace` passed 1,004 tests, failed 0, ignored 1 optional model-fixture test. The audio crate contributes 104 passing tests.
- `npm run check:claude` and `npm run check:docs` passed. Independent source review found no consequential issue. Native device/installer behavior was not exercised; AEC remains experimental.
- Existing ±1-second recovery verification remains unchanged for compatibility; exact fresh-output timing is covered by regression tests. Previously discarded PCM cannot be recovered from old archives.

## Optional production meeting AEC — 2026-09-28 (0.57.0)

- The owner delegated engine selection, waived further speakerphone testing, and explicitly authorized a main commit/push and build tag. Rust `aec3 = 0.4.0` is selected; schema 6 exposes **Reduce speaker echo**, default off, applied on the next meeting. AEC only changes the microphone before spool and live chunking; the system track and dictation path remain unchanged.
- WSL Debian Rust 1.98.1: `cargo fmt --all -- --check`, `cargo clippy --all-targets --locked --offline -- -D warnings`, and `cargo test --workspace --locked --offline` passed: **1,029 passed, 0 failed, 1 ignored**. The ignored test requires optional local-model weights. Both npm guards and `git diff --check` passed. The first strict lint exposed one new test's constant-size chunk iterator; it was corrected and the full Rust gates rerun successfully.
- Nine audio-wrapper tests cover finite/immutable frames, injected missing/malformed graph output, reset, near-only preservation, synthetic echo reduction, and fixed 128-sample latency against an identical high-pass reference. An adapted ending-impulse fixture separates this 8 ms processing delay from acoustic delay. Nine pairing and five recorder tests verify latency compensation/original-tail fallback, exact final counts/content, bounded FIFO capacity, missing reference, engine failure, device-loss races, resampler tails, AEC-off preservation, and live/saved audio agreement.
- Linux notice delivery was checked with four Bash syntax checks, four positive/eight negative gate controls, byte-identical Arch/tarball staging, a real deb fixture, and rpm metadata. The upstream MIT/BSD notice and patent text match pinned source after trailing-whitespace normalization. Historical benchmark JSON/CSV hashes and edited documentation AUTOGEN marker sequences are unchanged.
- This commit's Windows CI, signed installer, and actual Linux package artifacts remain pending at the pre-commit snapshot. Prior 0.56.1 CI and release completed successfully, but do not qualify the new source. Real-speaker intelligibility remains unverified by the owner's choice. Existing first-delivery placement and automatic acoustic-delay estimation do not establish common hardware-timestamp alignment or active device-clock drift correction.

## Linux meetings parity — 2026-09-29 (unreleased)

- Meeting mode runs on Linux end to end: PipeWire loopback (default-sink monitor for manual starts, per-app output stream nodes for detected ones, 16 kHz mono f32 converted by PipeWire), graph-node detection with `/proc` process trees and X11 window titles, the shared evdev chord router, and the un-gated Meetings UI, tray entry, Settings section, and Share menu (zenity/kdialog save dialogs, Word export, desktop folder reveal). `libpipewire-0.3` is declared by every Linux package; CI installs `libpipewire-0.3-dev`.
- Verified on real Linux (Fedora, PipeWire 1.6.9) against a private session-manager stack with null sinks: both loopback modes captured ~16,000 frames/s continuously (silence included), `IncludeTree` provably tapped the targeted stream (sine on a non-default sink), the detector debounced a mic holder to `Prompt("zoom")` with watcher wakes on graph churn, and probe snapshots cost ~3 ms. `cargo fmt`, workspace clippy `-D warnings`, `cargo test --workspace`, both npm guards: green.
- Not claimed: a real microphone call on production hardware, Wayland browser-title detection (documented absent), and per-app capture for apps that bypass PipeWire (documented default-sink fallback). Windows and macOS builds are unchanged apart from the shared-module rename; Windows CI will confirm compilation.

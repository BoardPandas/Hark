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

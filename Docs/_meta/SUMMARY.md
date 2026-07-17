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
| features/DICTIONARY.md | 6 | 71 | 1 |
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
| `crates/hark-dictionary` | DICTIONARY |
| `crates/hark-voice` | VOICE_CLEANUP |
| `crates/hark-inject` | TEXT_INJECTION |
| `crates/hark-config` + `crates/hark-keychain` | CONFIGURATION |
| `crates/hark-store` | DATA_STORAGE |
| `crates/hark-update` + `crates/hark-autostart` | UPDATES_AND_AUTOSTART |
| `installer/`, `.github/workflows/release.yml`, `.github/RELEASING.md`, `package.json`, `Cargo.toml` | RELEASE_AND_PACKAGING |

### Notes and known gaps

- The DICTIONARY builder found that the biasing/`Dictionary` settings logic it was pointed at does not live in `crates/hark-dictionary` (which only implements the phonetic `Corrector`); it traced the real implementation to `crates/hark-config`, `crates/hark-pipeline`, and `crates/hark-stt` and cited those instead. The page documents the cross-crate split explicitly rather than citing a nonexistent struct.
- Test files (`crates/*/tests/**`, `examples/**`) are cited where they illustrate observed behavior but are not documented as standalone pages.
- macOS-specific code paths (CGEventTap hotkey, login item, macOS update link-out) are documented from the current source; several are Windows-first with macOS paths noted where the code marks them incomplete.
- No API-reference, database-service, auth, or hosting/infra pages exist: Hark is a native single-process desktop app with no web backend.

## Regeneration

Run `/doc-sync update` after code changes to regenerate only the AUTOGEN sections whose source files changed. Manual edits between AUTOGEN markers are preserved.

# Generation Metadata

- **Commit:** `edda9d9df731374ab20faac8a9f2dc86c18ea8f3`
- **Branch:** `main`
- **Generated:** 2026-09-10T17:17:44-04:00
- **Mode:** update (scoped)
- **Base commit:** `bcfcc3fef6f02252870fc3f06440d99992818ade`
- **Pages generated:** 0 new
- **Sections regenerated:** 4 across 3 existing pages
- **Citation style:** **repo-relative** for sections regenerated in this run, per
  `references/citation-policy.md`, which prefers them because they stay valid in
  the working tree, on GitHub, and in any renderer that resolves relative paths.
  Untouched sections still carry absolute blob URLs pinned to `1c17387` or
  `bcfcc3f`; those remain correct, because each points at the code it was
  written from. The wiki is therefore mixed-style until a full run reconciles
  it — the same transitional state the 0.35.5 amendment below describes.

## Run — 2026-09-10 (0.39.0, installer-only Windows distribution)

Scoped to the four sections falsified by 0.39.0, which removed the portable
Windows download and changed the in-app updater to run the signed Inno installer
instead of self-replacing the running exe:

- `GETTING_STARTED.md` / `hark_03_getting_started_install`
- `features/UPDATES_AND_AUTOSTART.md` / `hark_11_updates_autostart_overview`
- `operations/RELEASE_AND_PACKAGING.md` / `hark_13_release_packaging_overview`
- `operations/RELEASE_AND_PACKAGING.md` / `hark_13_release_packaging_workflow`

The release-workflow section needed more than a wording fix: every line number
in it was derived from a 223-line `release.yml` that has since been restructured
into four jobs and grown to 704 lines, so all of its citations pointed at
unrelated code. They were re-derived from the current file, and all 52 citations
across the four sections were checked to resolve to a real file and an in-range
line span (one off-by-one was caught and corrected this way, introduced when a
two-line header edit shifted a snippet).

**Deliberately NOT regenerated:** the rest of the wiki. The `bcfcc3f..edda9d9`
range spans a large amount of work, and a full refresh would produce one
unreviewable diff. Sections stale for other reasons remain listed under "Known
coverage gaps" in `SUMMARY.md`.

## Amendment — 2026-08-21 (0.35.5, hand edit, no regeneration)

The stamp above describes the **last `/doc-sync` run**, not the last change to
`Docs/`. It was already inaccurate before this amendment: `Docs/` was edited at
`01a3d37` (0.26.0, the Spellbook rename) without the stamp moving, and 37 commits
have landed since `bcfcc3f`. Treat "Generated" as *last generated*, and this
section as the log of hand edits since.

Hand edits made in 0.35.5, from a repo audit:

- `OVERVIEW.md` — the AUTOGEN stack table asserted "no local model", which stopped
  being true at 0.18.0. The STT row was corrected and an on-device row added, with
  repo-relative citations (the surrounding AUTOGEN block still carries absolute
  blob URLs pinned to `1c17387`; it will be reconciled on the next real run).
- `features/ON_DEVICE_STT.md` — registered in `_toc.yaml` as `hark_07b_on_device_stt`
  (it had never been listed, so `/doc-sync update` could not see it) and given 15
  citations, all verified against current line numbers. Its sections are marked
  `autogen: false`: the prose is hand-written and good, and should not be
  overwritten by a regeneration.
- `_meta/SUMMARY.md` — the known-coverage-gaps table stopped at 0.18.1; extended
  through 0.35.5.

## Scope note

This run was **deliberately scoped to the Invocations feature** rather than a
full refresh of the `1c17387..bcfcc3f` range. That range spans six releases
(0.14.3 through 0.20.0), so a full regeneration would rewrite most of the wiki
in one unreviewable diff. Sections falsified by feature work in 0.15.0-0.18.1
remain stale and are listed under "Known coverage gaps" in `SUMMARY.md`.

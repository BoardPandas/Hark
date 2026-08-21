# Generation Metadata

- **Commit:** `bcfcc3fef6f02252870fc3f06440d99992818ade`
- **Branch:** `main`
- **Generated:** 2026-07-22T14:26:25-04:00
- **Mode:** update (scoped)
- **Base commit:** `1c1738716fa4cd758b0c26ec94d0873d1bc35ac1`
- **Pages generated:** 1 new (`hark_08b_invocations`)
- **Sections regenerated:** 6 across 4 existing pages
- **Citation style:** absolute GitHub blob URLs pinned to the commit at which each section was generated. Sections regenerated in this run cite `bcfcc3f`; untouched sections still cite `1c17387`, which is correct — each citation points at the code it was written from.

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

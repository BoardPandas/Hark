# Incremental Update Policy

Update mode regenerates only the AUTOGEN sections affected by source-code changes. Manual notes between markers are preserved.

## Two-phase update

```
Phase A: TOC drift            Phase B: Source diff
- Detect added pages          - Compute git diff against base_commit
- Detect removed pages        - Map changed files to TOC sections
- Detect added sections       - Regenerate only those sections
- Detect removed sections     - Update PAGE_ID files in place
```

Run Phase A first. If the TOC was edited manually since last generation, propagate the structural changes before regenerating section content.

## Phase A: TOC sync

### Inputs
- `Docs/_toc.yaml` (current)
- `Docs/**/*.md` (existing pages with PAGE_ID markers)

### Detection
1. Build the set of expected `(page_id, section_id)` pairs from `_toc.yaml`
2. Build the set of actual pairs by scanning AUTOGEN markers in existing files
3. Diff the two sets

### Actions

**Added pages:** generate the full page using the appropriate template (init Step 3).

**Added sections in existing pages:** insert a new AUTOGEN block in TOC order, immediately after the previous section's END marker. Do not touch sibling sections.

**Removed pages:** if the page file exists, do not delete it automatically. Move it to `Docs/archive/` and add a note in `_meta/SUMMARY.md`. The user can delete from archive later.

**Removed sections:** delete the AUTOGEN block AND its trailing `---` separator. Preserve any manual content between markers as comments tagged `<!-- ARCHIVED: {section_id} -->`.

### Coexistence rule

Markdown files in `Docs/` without a `PAGE_ID` marker are treated as manually maintained and never modified by doc-sync. This lets the user drop hand-written guides into `Docs/` alongside generated content.

## Phase B: Source diff

### Inputs
- Base commit: from `Docs/_meta/GENERATION.md`
- Target commit: `git rev-parse HEAD`

### Detection
```bash
git diff --name-status {base}..{target}
```

For each changed file, find the set of TOC sections whose merged `source_files` glob matches the file. The union is the set of sections to regenerate.

### Three change classes

| Status | Action |
|---|---|
| Added (`A`) | If the file matches an existing pattern, regenerate that section. If not, decide whether to extend a section's `source_files`, add a new section, or add a new page. |
| Modified (`M`) | Regenerate every matching section. |
| Deleted (`D`) | Re-resolve the section's `source_files`. If empty after deletion, remove the section per Phase A rules. Otherwise, regenerate. |
| Renamed (`R`) | Treat as Deleted + Added. If the rename keeps the file under the same glob, only regenerate. |

### New source files: where to put them

Decision tree for an added file that does not match any existing pattern:

1. **CI/CD, build script, lockfile, dotfile** — ignore.
2. **Test file** — usually ignore (covered by the module's existing tests section). If a new test framework is introduced, add a section to the relevant module page.
3. **New file in an existing module directory** — extend that module page's `source_files` glob if it does not already cover the directory.
4. **New top-level directory or service** — create a new page (likely under `features/` or `core/`) with a default 4-5 section template.
5. **New config file** — add to the Configuration page if one exists, otherwise create a Configuration section in the most relevant page.

## Safe AUTOGEN replacement

When regenerating a section, you MUST:
1. Use Edit (not Write) on the page file
2. The `old_string` must include the exact `BEGIN:AUTOGEN` line, the section content, AND the exact `END:AUTOGEN` line
3. The `new_string` must include the same BEGIN/END markers verbatim, with new content between
4. Never replace the `---` separator between AUTOGEN blocks
5. Never touch content above the first AUTOGEN block (PAGE_ID, source-files block, H1, related pages line) unless the page-level source files actually changed
6. Never touch content below the last AUTOGEN block (manual notes, etc.)

### Example Edit

`old_string`:
```
<!-- BEGIN:AUTOGEN myapp_02_arch_components -->
## Service Components

The application is split into three services. ([app.ts:1-20](src/app.ts#L1-L20))

Sources: [app.ts:1-20](src/app.ts#L1-L20)
<!-- END:AUTOGEN myapp_02_arch_components -->
```

`new_string`:
```
<!-- BEGIN:AUTOGEN myapp_02_arch_components -->
## Service Components

The application is split into four services after the billing extraction. ([app.ts:1-25](src/app.ts#L1-L25))

| Service | Entry | Responsibility |
|---|---|---|
| api | `src/api/server.ts` | HTTP request handling |
| worker | `src/worker/index.ts` | Background jobs |
| billing | `src/billing/server.ts` | Stripe integration |
| scheduler | `src/scheduler/index.ts` | Cron triggers |

Sources: [app.ts:1-25](src/app.ts#L1-L25), [billing/server.ts:1-40](src/billing/server.ts#L1-L40)
<!-- END:AUTOGEN myapp_02_arch_components -->
```

## "Relevant source files" block

Update the top-of-page collapsible block when:
- The file list changed (added or removed files)
- Cited line ranges shifted significantly (> 10 lines for a referenced range)

Otherwise leave it alone. Do not rewrite the block on every update — diff churn matters.

## TOC metadata update

After all section regenerations succeed, update `_toc.yaml`:
```yaml
project:
  ref_commit_hash: "{new_target_commit}"
  branch: "{branch}"
  updated_at: "{YYYY-MM-DD}"
```

## Failure handling

| Failure | Action |
|---|---|
| Source file no longer exists when regenerating | Skip the section, log to SUMMARY, do not modify the page |
| Edit fails because AUTOGEN markers are missing | Add a SUMMARY entry; do not attempt to "repair" with Write — that risks clobbering manual content |
| Mermaid block fails after 3 fix attempts | Comment the block, add TODO marker, log to SUMMARY |
| Page file does not exist for an expected PAGE_ID | Treat as new page; generate from scratch |

## Update report

Append to `Docs/_meta/SUMMARY.md`:

```markdown
## Incremental Update — {YYYY-MM-DD HH:MM}

- **Mode:** update
- **Commit range:** `{base[:7]}..{target[:7]}`

### Phase A — TOC drift
- New pages: {N}
  - {page_id} → {filename}
- Removed pages: {N} (moved to archive/)
- Added sections: {N}
- Removed sections: {N}

### Phase B — Source diff
- Files changed: {N} ({A} added / {M} modified / {D} deleted / {R} renamed)
- Sections regenerated: {N}
- Pages touched: {N}

### Validation
- Structure errors: {N}
- Mermaid invalid: {N} ({fixed} fixed, {commented} commented)
- Coverage gap: {N} new source files not yet mapped to TOC
```

---
name: doc-sync
description: Build, audit, or incrementally refresh comprehensive project documentation in the `Docs/` folder. Generates a TOC-driven wiki with categorized pages (core, features, operations, api, design, runbooks), AUTOGEN markers for safe updates, evidence-based citations with line numbers, Mermaid diagrams, and a README index. Use after significant code changes, when onboarding a new repo, or to audit existing docs for staleness.
user-invocable: true
argument-hint: (optional) "audit" | "init" | "update" | a path to focus on
allowed-tools:
  - Read
  - Write
  - Edit
  - Glob
  - Grep
  - Bash
  - WebFetch
  - Agent(builder)
model: opus
effort: medium
---

# Documentation Sync

You are responsible for keeping `Docs/` aligned with the codebase. This skill produces thorough, evidence-based documentation, not surface-level notes. Treat `Docs/` as the canonical project wiki.

## Resolve the docs root (do this first)

Before any mode runs, determine the canonical documentation folder and reuse that exact name for the entire run. Do NOT assume `Docs/` exists just because a case-insensitive check passes.

1. List the repo-root entries and look for an existing docs folder **case-insensitively**: match `Docs`, `docs`, `DOCS`, or any other casing. On Windows and macOS the filesystem is case-insensitive, so `Docs/` and `docs/` resolve to the same directory; on Linux and in git they do not. Use `git ls-files` to see the case git actually tracks:
   ```bash
   git ls-files | grep -iE '^docs/' | head -n 1
   ```
   If that returns a path, the segment before the first `/` (e.g. `docs`) is the **tracked, canonical** casing. Prefer it over whatever the local filesystem reports.
2. Set a single variable `DOCS_ROOT` to the resolved name:
   - If a docs folder already exists (any casing), `DOCS_ROOT` = its existing/tracked name. Never rename it and never create a second folder in a different case.
   - If none exists, `DOCS_ROOT` = `Docs` (the default for new repos).
3. For the rest of this skill, every reference to `Docs/` means `{DOCS_ROOT}/`. When the existing folder is lowercase `docs/`, write to `docs/_toc.yaml`, `docs/README.md`, `docs/_meta/...`, etc. Match the existing case exactly so git, CI, and case-sensitive hosts see one consistent folder.
4. State the resolved `DOCS_ROOT` in your first status message so the user knows which folder you are writing to.

## Output Conventions

- All generated documentation lives under the resolved docs root (`DOCS_ROOT`, default `Docs/`) at the repo root. The paths below are written as `Docs/` for brevity but always mean `{DOCS_ROOT}/`.
- A single `Docs/_toc.yaml` is the source of truth for page IDs, source-file mappings, sections, and diagram requirements.
- Every generated page carries stable `<!-- PAGE_ID: ... -->` and `<!-- BEGIN:AUTOGEN ... -->` / `<!-- END:AUTOGEN ... -->` markers so that incremental updates only rewrite generated regions, never manual notes.
- Generation metadata lives in `Docs/_meta/GENERATION.md` (commit hash, branch, timestamp) and `Docs/_meta/SUMMARY.md` (coverage report).
- The user-facing index is `Docs/README.md`, modeled on the supportforge layout (Quick Start table, then categorized tables).

## Folder Layout (target)

```
Docs/
├── README.md                      # Index: Quick Start + categorized tables
├── _toc.yaml                      # TOC: pages, sections, source mappings
├── _meta/
│   ├── GENERATION.md              # commit, branch, timestamp
│   └── SUMMARY.md                 # coverage and validation report
├── core/                          # Architecture, auth, data model, principles
├── api/                           # API_REFERENCE.md and per-surface API docs
├── features/                      # One page per major feature or integration
├── operations/                    # Runbooks, monitoring, deploy, secrets
├── design-system/                 # UI/UX standards (only for UI projects)
├── frontend/                      # Frontend-specific guides (only when applicable)
├── plans/                         # Active design / planning docs (manual)
└── archive/                       # Historical docs (manual)
```

Omit folders that have no relevant pages. Add other folders only when the repo demands it (e.g., `desktop-agent/`, `branding/`).

## Three Modes

The skill operates in one of three modes. Detect the mode from the user's argument, the state of the resolved docs root (`DOCS_ROOT`, see "Resolve the docs root" above), and the recency of generation metadata. Resolve `DOCS_ROOT` first — all existence checks below run against that folder, regardless of its case.

| Mode | When to use | Output |
|------|------|--------|
| **init** | No docs folder exists in any case, or `{DOCS_ROOT}/_toc.yaml` is missing | Full repo scan → TOC design → page generation → validation → index |
| **update** | `{DOCS_ROOT}/_toc.yaml` exists; user changed code since last generation | Git diff → affected pages → regenerate only those AUTOGEN sections |
| **audit** | User asks to "audit" / "check" docs, or wants a stale-references report only | Inventory + cross-reference + report; no rewrites |

If the argument is empty: pick **init** when no `{DOCS_ROOT}/_toc.yaml`, otherwise **update**.

---

## References (read before writing)

Load these reference files before generating or updating any page. They contain the rules you must follow:

- `${CLAUDE_SKILL_DIR}/references/page-template.md` — required page structure, markers, headings
- `${CLAUDE_SKILL_DIR}/references/citation-policy.md` — evidence rules, source URL format, line numbers
- `${CLAUDE_SKILL_DIR}/references/mermaid-policy.md` — diagram syntax rules and validation
- `${CLAUDE_SKILL_DIR}/references/toc-schema.md` — `_toc.yaml` schema and ID conventions
- `${CLAUDE_SKILL_DIR}/references/doc-categories.md` — when to create which page type
- `${CLAUDE_SKILL_DIR}/references/incremental-update.md` — safe AUTOGEN replacement rules
- `${CLAUDE_SKILL_DIR}/references/readme-template.md` — `Docs/README.md` index structure

Page templates live in `${CLAUDE_SKILL_DIR}/templates/`:

- `overview.md`, `architecture.md`, `api-reference.md`, `feature.md`,
  `database-schema.md`, `module.md`, `data-flow.md`, `runbook.md`,
  `getting-started.md`, `configuration.md`, `glossary.md`, `_toc.yaml.template`

Each template defines required sections and minimum content expectations. Do not skip required sections; mark them `_TBD_` with a reason if no source exists.

---

## Mode: init (full generation)

### Step 1: Scan the repository

1. Capture git metadata. Run in parallel:
   - `git rev-parse HEAD` (commit hash)
   - `git rev-parse --abbrev-ref HEAD` (branch)
   - `git config --get remote.origin.url` (repo URL → derive `repo_base_url`)
2. Use Glob to enumerate the repo:
   - Top-level directories
   - Entry points: `**/main.{ts,js,py,go,rs}`, `**/index.{ts,js,tsx}`, `**/cmd/**/main.go`
   - Config: `package.json`, `pyproject.toml`, `Cargo.toml`, `go.mod`, `Dockerfile*`, `docker-compose*`, `*.tf`, `.env.example`
   - Build/CI: `Makefile`, `.github/workflows/*`, `turbo.json`, `nx.json`
   - Existing docs: `README*`, `**/*.md` outside the docs root (`{DOCS_ROOT}/`, matched case-insensitively) and `node_modules/`
3. Read `README.md`, `package.json` (or equivalent), and 2-3 entry-point files to understand purpose, stack, and structure.
4. Identify language(s), frameworks, key services, and module boundaries.

### Step 2: Design the TOC

1. Read `${CLAUDE_SKILL_DIR}/references/toc-schema.md` and `${CLAUDE_SKILL_DIR}/references/doc-categories.md`.
2. Pick page categories that match the project. Use the page-count guideline:

   | Project size | Files | Pages |
   |---|---|---|
   | Small | < 10 | 3-5 |
   | Medium | 10-50 | 5-8 |
   | Large | 50-200 | 8-14 |
   | Very Large | > 200 | 12-20 |

3. Group pages into folders (`core/`, `features/`, `operations/`, etc.) per the folder layout above.
4. For each page, decide:
   - `id` (`{repo}_{NN}_{slug}`), `title`, `filename`, `description`
   - `source_files` (page-level glob patterns)
   - `sections[]` with `id`, `title`, `autogen: true`, optional section-level `source_files`, `diagrams_needed`, `diagram_types`
   - `related_pages[]` for cross-linking
5. Write `Docs/_toc.yaml` using `${CLAUDE_SKILL_DIR}/templates/_toc.yaml.template` as a starting point. Replace placeholders with real values.

### Step 3: Generate pages

For each page in `_toc.yaml`:

1. Resolve `source_files` (page-level + section-level) using Glob.
2. Read each resolved file with Read (no line-number guessing — use the actual numbers Read returns).
3. Pick the matching template from `${CLAUDE_SKILL_DIR}/templates/` based on the page category.
4. Write the page to `Docs/{folder}/{filename}` following `${CLAUDE_SKILL_DIR}/references/page-template.md`:
   - `<!-- PAGE_ID: {id} -->` at the very top
   - Collapsible "Relevant source files" block listing inputs with line ranges
   - `# {title}` H1
   - `> **Related Pages**:` line linking to `related_pages`
   - One AUTOGEN block per section, with H2 heading, content, inline citations, and end-of-section `Sources:` line
   - `---` separator between sections
5. Apply citation rules from `${CLAUDE_SKILL_DIR}/references/citation-policy.md`:
   - Inline citations wrapped in parentheses, before the period
   - End-of-section `Sources:` summary
   - Use real line numbers from Read output; never invent
6. Generate Mermaid diagrams per `${CLAUDE_SKILL_DIR}/references/mermaid-policy.md` when `diagrams_needed: true`. Use `graph TD`, quote all node text, no special chars in subgraph names.

For large repos, generate pages in parallel by spawning `builder` agents per page (the custom agent in `.claude/agents/`, never the built-in `Explore` type -- the built-in loads every MCP tool schema and blows the context window). The `builder` agent has Write and Edit; do NOT use `explorer` for page generation, as it is read-only and cannot write pages. Each subagent receives: the page entry from TOC, the resolved source files, and the template path. Wait for all to complete before validation.

### Step 4: Generate the README index

Write `Docs/README.md` using `${CLAUDE_SKILL_DIR}/references/readme-template.md`. Include:
- A "Latest Updates" callout pulled from the most recent `CHANGELOG.md` entries (omit the callout entirely if `CHANGELOG.md` does not exist)
- A Quick Start table mapping common goals to entry pages
- One categorized table per folder, listing every generated page with its description from `_toc.yaml`
- A "Related Resources" section linking to root `README.md`, `agents.md`, and `CLAUDE.md` if they exist

### Step 5: Write metadata

Obtain the timestamp by running `date -Iseconds` (Bash) or `Get-Date -Format o` (PowerShell). Never infer or guess the current date.

Write `Docs/_meta/GENERATION.md`:

```markdown
# Generation Metadata

- **Commit:** `{hash}`
- **Branch:** `{branch}`
- **Generated:** {ISO timestamp}
- **Mode:** init
- **Pages generated:** {count}
- **Sections generated:** {count}
```

### Step 6: Validate

Run validation in this order:

1. **Structure validation** (model-driven, no script needed):
   - Every page has exactly one `PAGE_ID` marker matching the TOC
   - Every `autogen: true` section has matched `BEGIN:AUTOGEN` / `END:AUTOGEN` markers with the correct ID
   - No orphaned, duplicated, or extra markers
   - Every internal link `[text](path)` points to a file that exists
2. **Mermaid validation:**
   - If `mmdc` is on PATH (`command -v mmdc` in Bash), extract each ` ```mermaid` block to a uniquely named temp file in the session scratchpad directory (never `/tmp`) and run `mmdc -i {block}.mmd -o {block}.svg --quiet` per block. Unique filenames keep parallel validations from clobbering each other. If `mmdc` is unavailable, fall back to the static checks in `${CLAUDE_SKILL_DIR}/references/mermaid-policy.md` and note that syntactic validation was skipped.
   - For each invalid block, attempt at most 3 fixes per block using the rules in `${CLAUDE_SKILL_DIR}/references/mermaid-policy.md`. If still invalid, comment the block out and add a `<!-- TODO: invalid mermaid -->` marker.
3. **Coverage check:**
   - List source files referenced by `_toc.yaml` patterns vs. files actually cited in pages
   - List uncited public APIs (exports, route handlers, CLI commands)

### Step 7: Write the SUMMARY report

Write `Docs/_meta/SUMMARY.md` per `${CLAUDE_SKILL_DIR}/references/incremental-update.md` (Summary section). Include: pages generated vs. expected, citations per page, diagrams per page, validation errors, uncovered files.

---

## Mode: update (incremental)

### Step 1: Detect changes

1. Read the prior commit from `Docs/_meta/GENERATION.md` (`base_commit`).
2. Verify the base commit still exists: `git cat-file -e {base_commit}`. If it is unreachable (rebase, squash-merge, shallow clone), fall back to `git merge-base HEAD origin/{default_branch}` as the base; if that also fails, warn the user and degrade to a full regeneration of all TOC pages instead of a diff-scoped update.
3. Get current commit: `git rev-parse HEAD` (`target_commit`).
4. Check for uncommitted changes: `git status --porcelain`. If the tree is dirty, include the dirty files in the changed set (`git diff --name-status {base_commit}` without a target covers committed + working-tree changes) and record `dirty tree` in `Docs/_meta/GENERATION.md` so the citation line numbers are known to reflect the working tree, not a commit.
5. List changed files: `git diff --name-status {base_commit}..{target_commit}` (added / modified / deleted / renamed).
6. Read `Docs/_toc.yaml` and resolve every page's `source_files` glob.
7. Compute the affected set: pages whose resolved files intersect changed files.

### Step 2: Detect TOC drift

For every changed file that does NOT match any TOC source pattern:
- If it's a CI/CD/config/docs file, ignore.
- If it's substantive source code, decide whether to add a section to an existing page or create a new page. Update `_toc.yaml` accordingly.

### Step 3: Regenerate affected sections only

For each affected page:
1. Read the existing page file.
2. For each affected section (only those whose `source_files` intersect the changed files):
   - Re-read source files with Read to get current line numbers
   - Use Edit (NOT Write) to replace ONLY the content between `<!-- BEGIN:AUTOGEN {section_id} -->` and `<!-- END:AUTOGEN {section_id} -->`
   - Preserve everything outside the markers verbatim, including manual notes between AUTOGEN blocks
3. Update the "Relevant source files" block at the top to reflect new line ranges only for files that changed.

For each new TOC page: generate as in init Step 3.

For each deleted source file with no remaining coverage:
- If a section's `source_files` is now empty, remove the AUTOGEN block from the page and delete the section from `_toc.yaml`.
- If a page has no remaining sections, do NOT delete the page file. Move it to `Docs/archive/`, remove it from `_toc.yaml`, and note the move in `_meta/SUMMARY.md` (per `${CLAUDE_SKILL_DIR}/references/incremental-update.md`). The user deletes from archive when ready.

### Step 4: Update README and metadata

1. Update `Docs/README.md` only if pages were added, removed, or renamed.
2. Rewrite `Docs/_meta/GENERATION.md` with the new commit, timestamp, and `mode: update`.
3. Update the `project` block in `Docs/_toc.yaml`: `ref_commit_hash` = target commit, `branch`, and `updated_at` = today (per `${CLAUDE_SKILL_DIR}/references/incremental-update.md`).
4. Update the "Latest Updates" callout in `Docs/README.md` from new `CHANGELOG.md` entries since `base_commit` (skip if `CHANGELOG.md` does not exist).

### Step 5: Validate

Run the same validation sequence as init Step 6, scoped to changed files where possible.

### Step 6: Write the SUMMARY report

Write `Docs/_meta/SUMMARY.md` listing: commit range, pages updated, sections regenerated, sections added, sections removed, validation results.

---

## Mode: audit (no rewrites)

Use the legacy lightweight workflow. Do not rewrite anything.

### Step 1: Inventory
- Glob `{DOCS_ROOT}/**/*.md` (the resolved docs root) and any `**/*.md` outside it, excluding `node_modules/`, `dist/`, `build/`, `vendor/`, `.git/`, and other dependency or build-output folders.
- Categorize: API, setup, architecture, user, changelog.

### Step 2: Cross-reference
For each doc:
- Stale references (functions, files, endpoints that no longer exist) — Grep the codebase for each named symbol
- Incorrect examples (signatures changed)
- Broken internal links (file paths that no longer resolve)
- Stale version references vs `package.json` / lockfiles
- Missing docs for new public APIs / endpoints / config

### Step 3: Report

Write the audit report to stdout (not to a file unless asked). Format:

```
# Documentation Audit

## Summary
- Files scanned: N
- Issues: N (high: X, medium: Y, low: Z)

## Findings
[HIGH] Docs/api/API_REFERENCE.md:142 — references removed endpoint POST /v1/foo
[MED]  Docs/core/AUTHENTICATION.md — example uses old `getSession()` signature
[LOW]  Docs/README.md — internal link to features/old-feature.md (file does not exist)

## Missing Documentation
[NEEDS DOCS] src/api/routes/billing.ts — 8 routes with no doc page
[NEEDS DOCS] env var STRIPE_WEBHOOK_SECRET — referenced in code, not in docs

## Recommendations
1. Run `/doc-sync update` to regenerate stale sections.
2. Add a feature page for billing.
```

---

## Subagent usage

For init mode on medium/large repos, fan out page generation to `builder` agents in parallel (the custom agent, never the built-in `Explore` type; never `explorer`, which lacks Write). Each subagent prompt must include:
- The page's TOC entry (id, title, sections, source_files, diagrams_needed)
- Absolute path of the matching template
- Absolute paths of the reference files (page-template, citation-policy, mermaid-policy)
- The git ref_commit_hash and repo_base_url for citations
- An explicit instruction: write to `Docs/{folder}/{filename}` and return only "DONE: {filename}" or an error

Wait for all to complete, then run validation in the main session. Never run validation inside a subagent — it must see the whole output set.

For update mode, single-page jobs are usually fine without subagents. Spawn subagents only if more than 5 pages are affected.

---

## Enforcement rules

- NEVER invent line numbers, file paths, function names, or behavior. If a claim has no source, write `_TBD_` with a one-line reason.
- NEVER modify content outside `BEGIN:AUTOGEN` / `END:AUTOGEN` markers in update mode.
- NEVER skip the AUTOGEN markers — they are the contract that makes incremental updates safe.
- NEVER write to a path outside the resolved docs root `{DOCS_ROOT}/` (the legacy README at the repo root is left alone unless the user asks). Never create a second docs folder in a different case than the one that already exists.
- NEVER delete a generated page automatically. Pages that lose all their sections move to `Docs/archive/` with a note in `_meta/SUMMARY.md`; only the user deletes from archive.
- ALWAYS use `graph TD` for flowcharts, quote all Mermaid node text, and validate diagrams when `mmdc` is available.
- ALWAYS update `Docs/_meta/GENERATION.md` after a successful run so the next update knows the base commit.
- ALWAYS keep `Docs/_toc.yaml` and `Docs/README.md` in sync — every TOC page must appear in the README index, and every README link must point to a TOC-tracked file.

## Final checklist before reporting completion

- [ ] `Docs/_toc.yaml` valid (unique IDs, kebab-case slugs, every page has ≥ 1 source file)
- [ ] Every page has PAGE_ID and matched AUTOGEN markers per `${CLAUDE_SKILL_DIR}/references/page-template.md`
- [ ] Every page has at least: 6 H2 sections, 8 substantive bullets across sections, 2 tables when applicable, 2 code/file snippets when sources exist, 8+ source paths in "Relevant source files"
- [ ] Every diagram block parses (or is commented out with TODO)
- [ ] `Docs/README.md` lists every generated page
- [ ] Every link in `Docs/README.md` (Quick Start, folder tables, Related Resources) resolves to a file that exists
- [ ] `Docs/_meta/GENERATION.md` reflects the current commit
- [ ] `Docs/_meta/SUMMARY.md` written with coverage and validation results

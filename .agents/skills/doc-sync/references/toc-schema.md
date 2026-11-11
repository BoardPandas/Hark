# `Docs/_toc.yaml` Schema

`_toc.yaml` is the source of truth for which pages exist, what they cover, and which source files they map to. Generation, incremental update, and validation all read from this file.

## Schema

```yaml
project:
  name: string             # Required. Project name (display).
  description: string      # Required. 1-2 sentence project summary.
  repo_base_url: string    # Optional. Base URL for absolute citations, e.g., "https://github.com/owner/repo/blob".
  ref_commit_hash: string  # Required. Set from `git rev-parse HEAD` at generation time.
  branch: string           # Optional. Branch name at generation time.
  updated_at: string       # Required. YYYY-MM-DD.

pages:
  - id: string             # Required. Globally unique. Format: {repo}_{NN}_{slug}.
    title: string          # Required.
    folder: string         # Optional. Subdirectory under Docs/ (e.g., "core", "features").
    filename: string       # Required. Must end in `.md`.
    description: string    # Optional. One-line summary used in Docs/README.md.
    category: string       # Optional. One of: overview | architecture | api | feature | runbook | data-model | module | guide | configuration | glossary.
    source_files:          # Required. Page-level source patterns (shared by all sections).
      - string             # File paths or glob patterns.
    sections:              # Required. At least one entry.
      - id: string         # Required. Globally unique. Format: {page_id}_{slug}.
        title: string      # Required.
        description: string  # Optional. Hint for the generator.
        autogen: boolean   # Required. true means doc-sync owns this section.
        source_files:      # Optional. Section-level files merged with page-level.
          - string
        diagrams_needed: boolean       # Required.
        diagram_types:                 # Required when diagrams_needed=true.
          - string         # One of: flowchart | sequence | class | state | er
        sections:          # Optional. Nested subsections (max depth 3 from page).
          - ...
    related_pages:         # Optional. List of page IDs for cross-linking.
      - string

notes:                     # Optional. Free-text notes about TOC decisions.
  - string
```

## ID conventions

| Field | Format | Example |
|---|---|---|
| Page ID | `{repo}_{NN}_{slug}` | `myapp_03_authentication` |
| Section ID | `{page_id}_{slug}` | `myapp_03_authentication_sessions` |
| Nested section ID | `{parent_section_id}_{slug}` | `myapp_03_authentication_sessions_redis` |

Rules:
- `{repo}` is the kebab-cased repo name
- `{NN}` is a zero-padded order number
- `{slug}` is kebab-case, lowercase, no special characters
- All IDs are globally unique across the file
- Page numbering is for ordering only; gaps are allowed when pages are removed

## Folder mapping

| `category` | Default `folder` |
|---|---|
| overview | (root of Docs/) |
| architecture | `core/` |
| api | `api/` |
| feature | `features/` |
| runbook | `operations/` |
| data-model | `core/` |
| module | (root of Docs/) or `modules/` if many |
| guide | `guides/` or appropriate subfolder |
| configuration | `core/` or `operations/` |
| glossary | (root of Docs/) |

The generator is free to override `folder` when a more specific category fits the repo (for example, `desktop-agent/` for a desktop-agent project).

## `source_files` patterns

| Pattern | Matches |
|---|---|
| `src/main.py` | Exact file |
| `src/*.py` | Top-level Python files in `src/` |
| `src/**/*.py` | Python files anywhere under `src/` recursively |
| `src/api/` | Every file under `src/api/` |
| `docs/api/*.md` | Markdown files in `docs/api/` |

Patterns are resolved during generation. Use globs liberally for pages whose scope grows over time (e.g., `src/features/**/*.ts`).

## Diagram type values

| Type | Use for |
|---|---|
| `flowchart` | Process flows, data flow, decision trees |
| `sequence` | API call exchanges, message timing |
| `class` | Type relationships, inheritance, composition |
| `state` | State machines, lifecycle transitions |
| `er` | Database schema, entity relationships |

## Validation rules

1. All page and section IDs are globally unique
2. Every page has at least one `source_files` entry
3. `filename` ends with `.md`
4. `diagram_types` values are from the allowed set when `diagrams_needed: true`
5. `category` (if set) is from the allowed set
6. Nested section depth ≤ 3 from the page root
7. `ref_commit_hash` is a 40-char hex string

## Page count guidelines

| Project size | Files | Pages |
|---|---|---|
| Small | < 10 | 3-5 |
| Medium | 10-50 | 5-8 |
| Large | 50-200 | 8-14 |
| Very Large | > 200 | 12-20 |

## Example

```yaml
project:
  name: "MyApp"
  description: "Multi-tenant SaaS for managing widgets, with REST API and Next.js dashboard."
  repo_base_url: "https://github.com/acme/myapp/blob"
  ref_commit_hash: "abc1234567890abcdef1234567890abcdef12345"
  branch: "main"
  updated_at: "2026-04-16"

pages:
  - id: myapp_01_overview
    title: "Overview"
    folder: ""
    filename: "OVERVIEW.md"
    description: "Project introduction, stack, and where to start."
    category: overview
    source_files:
      - "README.md"
      - "package.json"
    sections:
      - id: myapp_01_overview_introduction
        title: "Introduction"
        autogen: true
        diagrams_needed: false
        diagram_types: []
      - id: myapp_01_overview_stack
        title: "Technology Stack"
        autogen: true
        diagrams_needed: false
        diagram_types: []
      - id: myapp_01_overview_layout
        title: "Repo Layout"
        autogen: true
        diagrams_needed: true
        diagram_types: ["flowchart"]
    related_pages:
      - myapp_02_architecture
      - myapp_03_getting-started

  - id: myapp_02_architecture
    title: "Architecture"
    folder: "core"
    filename: "ARCHITECTURE.md"
    description: "System architecture, services, runtime topology."
    category: architecture
    source_files:
      - "src/**/*.ts"
      - "docker-compose.yml"
      - "infra/**/*.tf"
    sections:
      - id: myapp_02_architecture_overview
        title: "System Overview"
        autogen: true
        diagrams_needed: true
        diagram_types: ["flowchart"]
      - id: myapp_02_architecture_services
        title: "Service Components"
        autogen: true
        source_files:
          - "src/services/**/*.ts"
        diagrams_needed: true
        diagram_types: ["class"]
      - id: myapp_02_architecture_data-flow
        title: "Data Flow"
        autogen: true
        diagrams_needed: true
        diagram_types: ["sequence"]
    related_pages:
      - myapp_01_overview

notes:
  - "Frontend lives at apps/dashboard but does not have its own page yet."
  - "Plan to add a separate page when frontend grows past 30 components."
```

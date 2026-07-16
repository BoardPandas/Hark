# `Docs/README.md` Index Structure

The README is the front door to `Docs/`. It must be skimmable in 30 seconds and get a reader to the right page in two clicks.

## Required sections (in order)

1. Title (H1)
2. Latest Updates callout (blockquote with 3-5 bullets)
3. Quick Start table (Goal → Page)
4. One H2 section per folder, each containing a markdown table of pages
5. Related Resources section linking to root files
6. Last Updated footer

## Template

```markdown
# {Project Name} Documentation

> **Latest Updates ({Month YYYY}):**
> - **v{X.Y.Z}:** {one-line summary of most recent release notes}
> - **v{X.Y.Z}:** {next}
> - **v{X.Y.Z}:** {next}

## Quick Start

| Goal | Start Here |
|------|------------|
| **Understand the system** | [ARCHITECTURE.md](core/ARCHITECTURE.md) |
| **API endpoints** | [API_REFERENCE.md](api/API_REFERENCE.md) |
| **Run the project locally** | [GETTING_STARTED.md](GETTING_STARTED.md) |
| **Deploy / operate** | [RUNBOOK.md](operations/RUNBOOK.md) |
| **Database schema** | [DATABASE_SCHEMA.md](core/DATABASE_SCHEMA.md) |

---

## Core

Foundational platform documentation.

| Document | Description |
|----------|-------------|
| [ARCHITECTURE.md](core/ARCHITECTURE.md) | System architecture and infrastructure |
| [AUTHENTICATION.md](core/AUTHENTICATION.md) | Auth flows, sessions, tokens |
| [DATABASE_SCHEMA.md](core/DATABASE_SCHEMA.md) | Tables, relationships, indexes |
| [CONFIGURATION.md](core/CONFIGURATION.md) | Configuration layers and environment variables |

---

## API

| Document | Description |
|----------|-------------|
| [API_REFERENCE.md](api/API_REFERENCE.md) | Comprehensive endpoint reference |

---

## Features

Feature-specific documentation.

| Document | Description |
|----------|-------------|
| [{FEATURE_NAME}.md](features/{FEATURE_NAME}.md) | {one-line description from _toc.yaml} |
| ... | ... |

---

## Operations

Deployment, monitoring, and on-call procedures.

| Document | Description |
|----------|-------------|
| [RUNBOOK.md](operations/RUNBOOK.md) | Deployment, rollback, scaling |
| [MONITORING.md](operations/MONITORING.md) | Metrics, dashboards, alerts |
| [SECRETS_AND_ENV.md](operations/SECRETS_AND_ENV.md) | Environment variables and secret storage |

---

## Design System

(Include only if the project has a UI surface and a design-system page exists.)

| Document | Description |
|----------|-------------|
| [DESIGN_SYSTEM.md](design-system/DESIGN_SYSTEM.md) | Tokens, typography, color, spacing |

---

## Frontend

(Include only if there is a frontend page.)

| Document | Description |
|----------|-------------|
| [FRONTEND.md](frontend/FRONTEND.md) | Framework setup, routing, data fetching |

---

## Glossary

| Document | Description |
|----------|-------------|
| [GLOSSARY.md](GLOSSARY.md) | Domain terms used across the project |

---

## Plans

Active design and planning documents (manually maintained).

| Document | Description |
|----------|-------------|
| ... | ... |

---

## Archive

Historical reference documents.

- [archive/](archive/) — superseded specs, completed migration reports, prior reviews

---

## Related Resources

| Resource | Location |
|----------|----------|
| Repo README | [../README.md](../README.md) |
| Agent Registry | [../agents.md](../agents.md) |
| Claude Code Config | [../CLAUDE.md](../CLAUDE.md) |

---

**Last Updated:** {Month YYYY}
```

## Generation rules

- Drop any section that has no entries. Do not leave headings with empty tables.
- Pull descriptions from `_toc.yaml`'s `description` field; if missing, derive from the page H1 + first sentence of the first section.
- Pull "Latest Updates" bullets from the most recent 3-5 entries in `CHANGELOG.md`. If `CHANGELOG.md` does not exist, omit the callout.
- "Related Resources" only links to files that actually exist.
- Sort Quick Start by likelihood of use (Architecture and Getting Started first), not alphabetically.

## Update rules

When a page is added/removed/renamed:
- Update the matching folder section
- Update Quick Start only if the change affects the entry-page choices
- Update the Latest Updates callout from new CHANGELOG entries since the last `_meta/GENERATION.md` commit
- Refresh the Last Updated footer

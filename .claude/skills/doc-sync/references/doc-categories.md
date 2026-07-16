# Documentation Categories

This file is a catalog. When you design `_toc.yaml`, pick categories from here based on what the repo actually contains. Do not add categories the source material does not support.

## Always include (when material exists)

### Overview
- File: `Docs/OVERVIEW.md` (or rolled into the README index for very small repos)
- Template: `templates/overview.md`
- Source: `README.md`, `package.json`, top-level entry points
- Sections: Introduction, Purpose, Tech Stack, Repo Layout, Entry Points, Where to Start, How to Navigate
- Diagrams: a single high-level repo-layout flowchart

### Architecture
- File: `Docs/core/ARCHITECTURE.md`
- Template: `templates/architecture.md`
- Source: service entry files, infra config, deployment manifests
- Sections: System Context, Components, Service Communication, Runtime Topology, Interfaces & Contracts, Scaling Notes, Failure Modes
- Diagrams: system overview flowchart, optional sequence per major flow

### Getting Started
- File: `Docs/GETTING_STARTED.md`
- Template: `templates/getting-started.md`
- Source: `README.md`, `Makefile`, `docker-compose.yml`, `.env.example`
- Sections: Prerequisites, Installation, Configuration, Run Locally, Run Tests, Quick Start
- Diagrams: optional installation flowchart

## Include for backend or full-stack repos

### API Reference
- File: `Docs/api/API_REFERENCE.md`
- Template: `templates/api-reference.md`
- Source: route files, OpenAPI specs, controllers
- Sections: Authentication Methods, Common Response Patterns, Rate Limiting, then one section per resource group with an endpoint table
- Diagrams: sequence diagrams for non-trivial flows (auth, OAuth, webhook delivery)
- Page split rule: if the API has > 60 endpoints, split into one page per resource group under `Docs/api/`

### Database Schema / Data Model
- File: `Docs/core/DATABASE_SCHEMA.md`
- Template: `templates/database-schema.md`
- Source: migrations, ORM models, schema files
- Sections: Overview, Table Categories, ER Diagram, per-table sections, Foreign Keys, Indexes
- Diagrams: ER for major entity clusters

### Authentication
- File: `Docs/core/AUTHENTICATION.md`
- Template: pattern from `templates/feature.md`
- Source: auth middleware, session code, OAuth handlers
- Sections: Auth Methods, Session Lifecycle, Token Format, OAuth Flows, Security Properties
- Diagrams: sequence for each auth flow

## Include for repos with significant features or integrations

### Per-feature page
- File: `Docs/features/{FEATURE_NAME}.md`
- Template: `templates/feature.md`
- One page per major feature or third-party integration
- Sections: Overview, Architecture, Data Model, API Surface, Configuration, Failure Modes, Operational Notes
- Diagrams: at minimum one architecture or sequence diagram

### Per-module page
- File: `Docs/modules/{MODULE_NAME}.md` (or under `core/` if there are < 4 modules)
- Template: `templates/module.md`
- Source: a single source-tree directory
- Sections: Responsibilities, Key Files, Public Interfaces, Inputs/Outputs, Configuration, Tests, Edge Cases

## Operations

### Runbook
- File: `Docs/operations/RUNBOOK.md`
- Template: `templates/runbook.md`
- Source: deployment scripts, CI workflows, infra manifests
- Sections: Deployment Procedure, Rollback, Scaling, On-call Procedures, Common Incidents, Recovery Steps

### Monitoring
- File: `Docs/operations/MONITORING.md`
- Source: dashboard configs, alert rules, metrics emitters
- Sections: Service Health Checks, Key Metrics, Alert Rules, Dashboards, Log Locations

### Secrets / Environment
- File: `Docs/operations/SECRETS_AND_ENV.md`
- Source: `.env.example`, secret-loading code, deployment scripts
- Sections: Required Environment Variables, Optional Variables, Secret Storage, Rotation Procedure

### Configuration
- File: `Docs/core/CONFIGURATION.md`
- Template: `templates/configuration.md`
- Source: config files, config-loading code
- Sections: Configuration Layers, Environment Variables, Config File Formats, Defaults, Override Order

## UI projects (skip otherwise)

### Design System
- File: `Docs/design-system/DESIGN_SYSTEM.md`
- Source: token files, component library
- Sections: Tokens, Type Scale, Color Palette, Spacing, Component Inventory, Accessibility Requirements

### Frontend Guide
- File: `Docs/frontend/FRONTEND.md`
- Source: framework config, top-level pages, component conventions
- Sections: Framework Setup, Routing, State Management, Data Fetching, Styling, Testing

## Always-last category

### Glossary
- File: `Docs/GLOSSARY.md`
- Template: `templates/glossary.md`
- Source: domain terms encountered across pages
- Sections: Terms (one entry per term) and Acronyms, matching `templates/_toc.yaml.template`

## When NOT to add a page

- The category exists in the catalog but the repo has no source for it. Example: no `Dockerfile`, no compose file, no IaC ⇒ skip Runbook.
- The page would have fewer than 3 substantive sections after honest writing.
- The content already lives elsewhere and would be a duplicate.

## When to split a page

- The page is over 600 lines after generation
- The page covers more than one logical resource group (e.g., API for users + API for billing — split into two pages)
- A single section's `source_files` matches > 30 files

## When to merge pages

- Two pages share more than half their `source_files`
- A page has only 2 sections and no diagrams — fold it into the most related sibling

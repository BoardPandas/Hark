<!-- PAGE_ID: {page_id} -->
<details>
<summary>Relevant source files</summary>

- [migrations/](migrations/)
- [src/models/](src/models/)

</details>

# {Project Name} -- Database Schema

> **Database:** {engine} {version}
> **Last Updated:** {YYYY-MM-DD}

> **Related Pages**: [Architecture](ARCHITECTURE.md)

---

<!-- BEGIN:AUTOGEN {page_id}_overview -->
## Overview

{1-2 sentence description of the schema's purpose and design philosophy: multi-tenant? single-tenant? event-sourced?}

{Paragraph on key conventions: ID format (UUID vs serial), timestamp columns, soft-delete strategy.}

Sources: {migration / model citations}
<!-- END:AUTOGEN {page_id}_overview -->

---

<!-- BEGIN:AUTOGEN {page_id}_categories -->
## Table Categories

| Category | Tables | Purpose |
|---|---|---|
| Core | `users`, `sessions`, ... | {purpose} |
| {Category} | `t1`, `t2`, ... | {purpose} |

Sources: [migrations/](migrations/)
<!-- END:AUTOGEN {page_id}_categories -->

---

<!-- BEGIN:AUTOGEN {page_id}_er-diagram -->
## Entity Relationship Diagram

```mermaid
erDiagram
    USER ||--o{ SESSION : has
    USER ||--o{ {RESOURCE} : owns
    USER {
        uuid id PK
        string email
        string role
    }
    SESSION {
        uuid id PK
        uuid user_id FK
        timestamptz expires_at
    }
    {RESOURCE} {
        uuid id PK
        uuid user_id FK
        string name
    }
```

Sources: {citations}
<!-- END:AUTOGEN {page_id}_er-diagram -->

---

<!-- BEGIN:AUTOGEN {page_id}_table-{table} -->
### `{table_name}`

{One-sentence description.}

| Column | Type | Constraints | Description |
|---|---|---|---|
| `id` | `uuid` | PK | Primary key |
| `created_at` | `timestamptz` | NOT NULL DEFAULT now() | Creation timestamp |
| `{column}` | `{type}` | `{constraints}` | {description} |

**Indexes:**
- `idx_{table}_{column}` on `({column})` -- {purpose}

**Foreign keys:**
- `{column}` → `{ref_table}.{ref_column}` ON DELETE {action}

Sources: [migration:N-M](migrations/X.sql#LN-LM)
<!-- END:AUTOGEN {page_id}_table-{table} -->

---

<!-- BEGIN:AUTOGEN {page_id}_indexes -->
## Indexes Summary

| Table | Index | Columns | Purpose |
|---|---|---|---|
| `{table}` | `{idx_name}` | `({cols})` | {why this index exists} |

Sources: [migrations/](migrations/)
<!-- END:AUTOGEN {page_id}_indexes -->

---

<!-- BEGIN:AUTOGEN {page_id}_migrations -->
## Migration Conventions

- {Naming convention for migration files}
- {How down-migrations are handled, if at all}
- {Rules for breaking schema changes}

Sources: [migrations/](migrations/), {migration tool docs}
<!-- END:AUTOGEN {page_id}_migrations -->

---

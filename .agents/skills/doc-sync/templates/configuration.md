<!-- PAGE_ID: {page_id} -->
<details>
<summary>Relevant source files</summary>

- [.env.example](.env.example)
- [src/config/](src/config/)

</details>

# Configuration

> **Related Pages**: [Getting Started](../GETTING_STARTED.md), [Runbook](../operations/RUNBOOK.md)

---

<!-- BEGIN:AUTOGEN {page_id}_overview -->
## Overview

{Sentence on configuration philosophy: env vars only? config files? layered?}

| Layer | Source | Override Order |
|---|---|---|
| Defaults | code | lowest |
| Config file | `{path}` | medium |
| Environment | shell / secrets manager | highest |

Sources: [src/config/index.ts:N-M](src/config/index.ts#LN-LM)
<!-- END:AUTOGEN {page_id}_overview -->

---

<!-- BEGIN:AUTOGEN {page_id}_required-env -->
## Required Environment Variables

| Variable | Description | Example | Source |
|---|---|---|---|
| `DATABASE_URL` | Postgres connection string | `postgres://...` | [config.ts:N](src/config/index.ts#LN) |
| `REDIS_URL` | Redis connection string | `redis://...` | [config.ts:N](src/config/index.ts#LN) |
| `{VAR}` | {description} | `{example}` | [config.ts:N](src/config/index.ts#LN) |

Sources: [.env.example](.env.example), [src/config/index.ts](src/config/index.ts)
<!-- END:AUTOGEN {page_id}_required-env -->

---

<!-- BEGIN:AUTOGEN {page_id}_optional-env -->
## Optional Environment Variables

| Variable | Default | Description | Source |
|---|---|---|---|
| `{VAR}` | `{default}` | {description} | [config.ts:N](src/config/index.ts#LN) |

Sources: [src/config/index.ts](src/config/index.ts)
<!-- END:AUTOGEN {page_id}_optional-env -->

---

<!-- BEGIN:AUTOGEN {page_id}_validation -->
## Validation

{How config is validated at startup. Schema validation? Fail-fast on missing required vars?}

```{lang}
// excerpt of validation logic
```

Sources: [config.ts:N-M](src/config/index.ts#LN-LM)
<!-- END:AUTOGEN {page_id}_validation -->

---

<!-- BEGIN:AUTOGEN {page_id}_secrets -->
## Secret Management

- {Where production secrets live}
- {How they are injected at runtime}
- {Rotation procedure}

Sources: {citations}
<!-- END:AUTOGEN {page_id}_secrets -->

---

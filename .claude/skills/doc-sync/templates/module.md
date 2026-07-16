<!-- PAGE_ID: {page_id} -->
<details>
<summary>Relevant source files</summary>

- [path/to/file:N-M](path/to/file#LN-LM)

</details>

# Module: {Module Name}

> **Related Pages**: [Architecture](../core/ARCHITECTURE.md)

---

<!-- BEGIN:AUTOGEN {page_id}_responsibilities -->
## Responsibilities

- {What this module owns}
- {What it explicitly does NOT own}

Sources: {entry-file citations}
<!-- END:AUTOGEN {page_id}_responsibilities -->

---

<!-- BEGIN:AUTOGEN {page_id}_key-files -->
## Key Files

| File | Purpose |
|---|---|
| [{name}.ts](path/to/{name}.ts) | {one-line purpose} |

Sources: {citations}
<!-- END:AUTOGEN {page_id}_key-files -->

---

<!-- BEGIN:AUTOGEN {page_id}_public-interface -->
## Public Interface

| Export | Type | Signature | Source |
|---|---|---|---|
| `{name}` | function | `(args) => result` | [file:N](path#LN) |
| `{name}` | class | -- | [file:N](path#LN) |

Sources: [index.ts:N-M](path/index.ts#LN-LM)
<!-- END:AUTOGEN {page_id}_public-interface -->

---

<!-- BEGIN:AUTOGEN {page_id}_io -->
## Inputs and Outputs

| Type | Name | Location | Description |
|---|---|---|---|
| Input | {name} | {file/CLI/env} | {purpose} |
| Output | {name} | {file/dir/API} | {purpose} |

Sources: {citations}
<!-- END:AUTOGEN {page_id}_io -->

---

<!-- BEGIN:AUTOGEN {page_id}_configuration -->
## Configuration

| Variable | Default | Source |
|---|---|---|
| `{ENV_VAR}` | `{value}` | [config.ts:N](path/config.ts#LN) |

Sources: [config.ts](path/config.ts)
<!-- END:AUTOGEN {page_id}_configuration -->

---

<!-- BEGIN:AUTOGEN {page_id}_examples -->
## Example Usage

```{lang}
// import and call the module
```

Sources: {citation to a real call site if one exists}
<!-- END:AUTOGEN {page_id}_examples -->

---

<!-- BEGIN:AUTOGEN {page_id}_edge-cases -->
## Edge Cases and Limits

- {Limit or edge case with citation}

Sources: {citations or "_TBD_"}
<!-- END:AUTOGEN {page_id}_edge-cases -->

---

<!-- BEGIN:AUTOGEN {page_id}_tests -->
## Tests

| Test File | Coverage |
|---|---|
| [{name}.test.ts](path/to/test) | {what it covers} |

Sources: [tests/](path/to/tests/)
<!-- END:AUTOGEN {page_id}_tests -->

---

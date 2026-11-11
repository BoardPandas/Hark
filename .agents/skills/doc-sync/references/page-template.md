# Page Template and Markers

This file is the contract every generated page must satisfy.

## Required structure

```markdown
<!-- PAGE_ID: {page_id} -->
<details>
<summary>Relevant source files</summary>

The following files were used as evidence for this page:

- [path/to/file1.ext:1-N](path/to/file1.ext#L1-LN)
- [path/to/file2.ext:M-K](path/to/file2.ext#LM-LK)

</details>

# {Page Title}

> **Related Pages**: [Page A](../folder/page-a.md), [Page B](../folder/page-b.md)

---

<!-- BEGIN:AUTOGEN {section_id} -->
## {Section Title}

{1-2 sentence intro.}

{Body: prose, tables, code blocks, mermaid diagrams. Inline citations wrapped in parentheses, before the period.}

Sources: [file.ext:M-K](file.ext#LM-LK), [other.ext:P](other.ext#LP)
<!-- END:AUTOGEN {section_id} -->

---

<!-- BEGIN:AUTOGEN {next_section_id} -->
## {Next Section Title}

...
<!-- END:AUTOGEN {next_section_id} -->

---
```

## Marker rules

### PAGE_ID

- Exactly one `<!-- PAGE_ID: {id} -->` per file
- Must be the very first non-blank line
- `id` must match the page entry in `Docs/_toc.yaml`

### AUTOGEN

- Every section with `autogen: true` in TOC needs paired `BEGIN:AUTOGEN` / `END:AUTOGEN` markers
- The `section_id` after BEGIN must equal the `section_id` after END
- Content outside markers is preserved across regenerations
- A `---` horizontal rule separates each AUTOGEN block
- Nested sections may use nested AUTOGEN blocks (parent BEGIN, then child BEGIN/END pairs, then parent END)

### Manual sections (autogen: false)

```markdown
## Manual Section Title

<!-- This section is manually maintained. doc-sync will not modify it. -->

{free-form content}

---
```

Do NOT add AUTOGEN markers to manual sections.

## Heading levels

| Section depth | Heading |
|---|---|
| Page title | H1 |
| Top-level section | H2 |
| Nested level 1 | H3 |
| Nested level 2 | H4 |

## Relevant source files block

- Use a `<details>` block at the top so the listing collapses by default
- Include 8+ paths when available
- Path format: `[path:start-end](path#Lstart-Lend)` using the relative path from the repo root and the actual line ranges Read returned
- For files where no specific range applies (e.g., a config file referenced as a whole), omit the line range

## Related Pages line

- One line, immediately after the H1
- Use markdown link syntax with relative paths
- 2-5 links is usually right

## Section content requirements

Each section must:
- Begin with a 1-2 sentence intro that frames the topic
- Use tables for parameters, endpoints, configuration, schemas
- Use fenced code blocks with language identifiers for examples
- Cite real lines from real files using the format in `citation-policy.md`
- End with a `Sources:` line listing every cited file (or omit if every claim is inline-cited and Sources would be redundant)

## Depth defaults (per page)

- ≥ 6 H2 sections
- ≥ 8 substantive bullets across sections
- ≥ 2 tables when sources permit
- ≥ 2 code or file snippets when sources permit
- "Relevant source files" lists ≥ 8 paths when the repo has that much material

If a page does not meet a target because the source material truly does not exist, note it explicitly in `_meta/SUMMARY.md` rather than padding with filler.

## Snippet quality

Snippets must be information-dense. Prefer code that exposes:
- Real parameters and defaults
- Conditional branches or feature flags
- Data structures or schema fields
- Error handling or validation logic
- I/O paths and artifact names

If only trivial snippets exist (e.g., a one-line export), replace them with a structured table summarizing the parameters/exports instead.

## Anti-patterns

- Bullet list AND table covering the same data — pick the table
- Lorem-ipsum / placeholder text not labeled `_TBD_`
- Citations without line numbers
- Headings with no content under them
- Trailing whitespace inside AUTOGEN blocks (breaks marker matching for some renderers)

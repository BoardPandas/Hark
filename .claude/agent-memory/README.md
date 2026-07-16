# Agent Memory

This directory stores persistent, version-controlled knowledge that agents build up through execution. Unlike skills (static, loaded at startup), agent memory is dynamic and evolves with the project.

Curate it deliberately. Agents reliably maintain memory only when prompts say so directly, so prefer explicit, hand-tended entries over passive accumulation.

## Conventions

- **patterns.md** -- Recurring code patterns, architectural decisions, and conventions discovered during implementation.
- **decisions.md** -- Key technical decisions with rationale. Helps future sessions understand "why" without re-deriving.
- **debugging.md** -- Failed approaches and dead ends. Prevents future sessions from repeating the same mistakes.

## Entry Format

Use `YYYY-MM-DD: Title` headers. Keep each entry to a few sentences with links to relevant files or commits for deeper investigation. Never store secrets, API keys, or tokens in memory files.

## Scope Levels

Memory operates across three distinct scopes (in addition to this directory):

- **Project scope** (`.claude/agent-memory/`): team-shared, version-controlled. Default.
- **User scope** (`~/.claude/agent-memory/<agent-name>/`): personal, cross-project.
- **Local scope** (`.claude/agent-memory-local/<agent-name>/`): personal, project-specific, git-ignored.

## Activation

Agents pick up files from this directory either by listing them explicitly in `memory:` frontmatter (preferred, e.g., `architect.md` references `agent-memory/decisions.md`) or by setting `memory: project` to auto-load the standard files.

## Rules

1. Never overwrite existing entries -- append new findings.
2. Each entry should include a date and brief context.
3. Only the first 200 lines of any memory file are injected into agent context at startup. Keep the most important entries at the top.
4. When a file exceeds 200 lines, split by topic (e.g., `react-patterns.md`, `auth-decisions.md`) -- topic partitioning beats date partitioning.
5. Remove entries that are contradicted by current code or no longer relevant.
6. This directory is version-controlled -- commit changes so the whole team benefits.

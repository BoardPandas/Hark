# Scaffold File Templates

Referenced by `init-repo` Steps 7, 8, 14, and 15. Read this file when creating path-scoped rules, initializing `.claude/agent-memory/`, writing `.claude/bp-audit.md`, or resetting `CHANGELOG.md` and version fields to `0.0.1`.

## Step 7: Path-Scoped Rule Example & Inventory

### Standard rules to create based on detected stack

For every project:
- `.claude/rules/tests.md` — Testing conventions, paths: `["**/test/**", "**/*.test.*", "**/*.spec.*", "**/__tests__/**"]`
- `.claude/rules/bp-check.md` — RULE 3 enforcement: check BP before modifying infrastructure/tooling configs. Paths: `["CLAUDE.md", ".claude/**", "Dockerfile*", "biome.*", "turbo.json", "vitest.config.*", "jest.config.*", ".github/**"]` (prune any glob matching zero files per `npm run check:claude`).
- `.claude/rules/llg-check.md` — RULE 1 enforcement: check LL-G before writing code. Paths: real source directories in this repo (e.g., `["src/**", "scripts/**"]`). Copy from the bootstrap repo's `.claude/rules/llg-check.md` and prune dead globs.

For frontend projects:
- `.claude/rules/frontend.md` — Component patterns, styling rules, accessibility (paths matching frontend source dirs).
- `.claude/rules/styles.md` — CSS/styling conventions (paths matching style files).

For backend/API projects:
- `.claude/rules/api.md` — API conventions, error handling, auth patterns (paths matching API source dirs).
- `.claude/rules/database.md` — Migration rules, query patterns, ORM conventions (paths matching schema/migration files).

For monorepos: scope rules to each package/app.

### Example rule file format

```markdown
---
description: Frontend component conventions
paths:
  - "src/components/**"
  - "src/ui/**"
---

# Component Rules

- Max 200 lines per component file. Split large components into composition.
- Always export a single default component per file.
- Use semantic HTML elements over generic divs.
```

## Step 8: Agent Memory `.claude/agent-memory/debugging.md` Template

Initialize `.claude/agent-memory/debugging.md` with:

```markdown
# Gotchas & Learning Lessons

Reference this file before starting work. Add entries when you discover non-obvious behavior, surprising failures, or patterns that wasted time. Don't make the same mistakes twice.

## Format

### [Number]. [Short descriptive title]

**Context:** When/where this occurs.
**Problem:** What goes wrong.
**Solution:** What to do instead.
**Why:** Brief explanation of root cause.

---

*Keep entries concise and actionable. Remove entries that no longer apply.*
```

`.claude/agent-memory/README.md` should explain that files are version-controlled and team-shared, agents read relevant memory files before starting work and update them on new patterns/decisions, entries stay concise with no session-specific noise, and memory complements `CLAUDE.md` (rules in `CLAUDE.md`, discovered knowledge in `agent-memory`).

## Step 14: BP Audit & LL-G Seed Templates

### 14a. FOUNDATIONAL BP checklist

- [ ] Hierarchical `CLAUDE.md` under 16 KB (and total always-on context under 20 KB) with tech stack, standards, workflow
- [ ] RULE 0 (read-only-first) present in `CLAUDE.md`
- [ ] RULE 1 (LL-G integration) present in `CLAUDE.md` with 4-step protocol
- [ ] RULE 3 (BP integration) present in `CLAUDE.md`
- [ ] Credential deny-list in `settings.json`
- [ ] Context management guidance (compact at 50%, handoff docs)
- [ ] Plans must end with lessons learned stated in `CLAUDE.md`
- [ ] Plan-then-execute workflow documented
- [ ] Anti-pattern awareness documented
- [ ] `.claude/rules/bp-check.md` exists with live globs
- [ ] `.claude/rules/llg-check.md` exists with live globs

### 14c. `.claude/bp-audit.md` template

```markdown
# BP Audit Results
Date: <YYYY-MM-DD>
Score: Y/X (percentage%)

## Failing Practices (fix with `/apply-practice <slug>`)

### FOUNDATIONAL
- [ ] `<slug>` -- <what's missing>

### RECOMMENDED
- [ ] `<slug>` -- <what's missing>

## Passing Practices
- [x] `<slug>` -- <title>

## Note
Fix failing practices one at a time with `/apply-practice <slug>`. Re-running `/init-repo` refreshes this audit.
```

### 14d. LL-G seed section for `.claude/agent-memory/debugging.md`

```markdown
## Known Gotchas (from LL-G)

These are HIGH-severity gotchas for this project's tech stack.
See LL-G for full details: https://github.com/BoardPandas/LL-G

- [Gotcha title]: brief summary (tech, severity)
```

## Step 15: Fresh `0.0.1` Changelog & Manifest Baseline

### 15b. Fresh `CHANGELOG.md` template

```markdown
# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/2.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.1] - <YYYY-MM-DD>

### Added
- Initial project setup and Claude Code configuration.

[Unreleased]: https://github.com/<owner>/<repo>/compare/v0.0.1...HEAD
[0.0.1]: https://github.com/<owner>/<repo>/releases/tag/v0.0.1
```

Use today's actual date (never a placeholder or a date copied from template entries). Fill links from `git remote get-url origin`; omit the two link lines if there is no GitHub remote. Delete any template-inherited `CHANGELOG-archive.md`.

### 15c. Manifest version fields to reset to `0.0.1`

Reset only the manifest(s) the project actually has (do not create missing manifests):
- `package.json` — `"version": "0.0.1"`
- `pyproject.toml` — `version = "0.0.1"`
- `Cargo.toml` — `version = "0.0.1"`
- `*.csproj` — `<Version>0.0.1</Version>`

## Steps 9 & 10: Skill and Agent Frontmatter Field Reference

### Skill frontmatter fields (Step 9)
- `disable-model-invocation: true` — manual slash-command invocation only.
- `model: haiku | sonnet | opus` — `haiku` for mechanical step-by-step tasks, `sonnet` for analysis/research, `opus` for orchestration/planning.
- `effort: low | medium | high | xhigh | max` — match reasoning depth to task complexity.
- `context: fork` — run in isolated subagent context to prevent main-session context pollution.
- `agent: <agent-name>` — bind execution to a `.claude/agents/<agent-name>.md` persona/toolset. **Requires `context: fork`** (ignored without it), and every `allowed-tools` entry must be covered by that agent's `tools:`.

### Agent frontmatter fields (Step 10)
- `memory: user | project | local` — persistent memory scope (`project` = `.claude/agent-memory/`, recommended on `reviewer`, `architect`, `builder`, `explorer`, `tester`).
- `skills: [skill1, skill2]` — skills available to the agent (e.g., `[plan-repo, spec-developer]` on `architect`).
- `isolation: worktree` — temporary git worktree for file-writing agents (`builder`). Never add to read-only analysis agents (`explorer`, `reviewer`, `security`, `performance`, `ux-reviewer`).
- `background: true` — avoid setting as a frontmatter default; pass `run_in_background` per spawn instead so inline parallel workflows are not broken.
- `context: <instructions>` — extra system-prompt instructions scoped to this agent.
- `maxTurns: N` — iteration cap (e.g., `20`–`40`).

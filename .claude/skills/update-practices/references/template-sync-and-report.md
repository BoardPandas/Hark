# Template Sync Reference & Report Format

Referenced by `update-practices` Step 2b and Step 8. Read this file in full before running Step 2b or formatting the Step 8 report.

## Step 2b: Categorizing Template Files by Git Blob SHA

Fetch the tree API URL from `.claude/references/source-urls.md` using Bash so you get raw JSON:

```bash
curl -s "<tree-api-url>"
```

Extract all paths under `.claude/` plus each file's git blob SHA. Ignore files outside `.claude/` unless listed in **Files outside `.claude/` that MUST still sync** below.

Do NOT use WebFetch to compare file contents: WebFetch runs pages through a model and can truncate or reformat long files. Compare git blob SHAs instead:

1. **If missing locally:** check `.claude/references/template-sync-ignore.md`:
   - Listed → **TEMPLATE-IGNORED** (deliberately removed by this project; do not re-create).
   - Not listed → **TEMPLATE-NEW**.
2. **If present locally:** compute `git hash-object <local-path>` and compare with the tree entry's SHA:
   - Match → **TEMPLATE-CURRENT** (byte-for-byte identical, no action needed).
   - Differ → download raw bytes into scratchpad and diff locally:
     ```bash
     curl -s "<raw-base-url>/<path>" -o <scratchpad>/<path>
     git diff --no-index <local-path> <scratchpad>/<path>
     ```
     - Incremental edits to a few lines/sections → **TEMPLATE-UPDATED**.
     - Body substantially rewritten (most steps/sections changed, reordered, or replaced) → **TEMPLATE-REWRITTEN** (take the template body whole, then graft back only genuinely project-specific pieces).

## Files to Sync (Whitelist)

Only sync these categories of `.claude/` files from the template:

| Category | Path pattern | Sync strategy |
|----------|-------------|---------------|
| Skills | `.claude/skills/**` (`SKILL.md` plus supporting files under `references/`, `templates/`, `evals/`, etc.) | Sync new skills entirely, including supporting files. For **TEMPLATE-UPDATED**, merge new steps/sections while preserving project-specific customizations. For **TEMPLATE-REWRITTEN**, replace the body wholesale with the template version, then re-apply only genuinely project-specific additions. |
| Agents | `.claude/agents/*.md` | Sync new agents entirely. For **TEMPLATE-UPDATED**, update frontmatter and instructions while preserving project-specific `context` or `skills` overrides. For **TEMPLATE-REWRITTEN**, replace the body with the template version, preserving only project-specific `context`/`skills`/`tools` overrides. |
| Rules | `.claude/rules/*.md` | Sync new rules entirely. For **TEMPLATE-UPDATED**, update content while preserving custom `paths:` frontmatter. For **TEMPLATE-REWRITTEN**, replace the body with the template version, preserving only the project's custom `paths:` frontmatter. |
| Scripts | `.claude/scripts/*.sh` | Sync new scripts entirely. For existing scripts, replace with template version unless local version has project-specific logic (paths, env vars, tool references). |
| References | `.claude/references/source-urls.md` | Merge: add new URLs from template not already present. Remove a URL only when verified unreachable on two consecutive runs (`deadUrls` in sync state). |
| References | `.claude/references/hooks-and-settings.md` | Generic catalog. Replace with template version unless local copy has project-specific notes appended (merge new rows in). |
| References | `.claude/references/ux-laws.md` | Generic catalog. Replace with template version unless local copy has project-specific notes appended (merge new entries in). |
| References | `.claude/references/infrastructure.md` | Do NOT sync; infrastructure is project-specific. |
| References | `.claude/references/tools.md` | Do NOT sync; tools depend on project stack. |
| References | `.claude/references/design-guardrails.md` | Do NOT sync; guardrails depend on project stack. |
| Settings | `.claude/settings.json` | Deep-merge: add new hooks, permissions, and env vars from template. Never remove existing entries. Preserve project-specific matchers and custom hooks. |
| Settings | `.claude/settings.local.json.example` | Replace with template version. |
| References | `.claude/references/managed-settings.example.json` | Replace with template version. |

Historical removals owned by adopting projects (do NOT sync, audit, or delete if present):
- Eval suite removed in 0.21.0 (`.claude/evals/`, `scripts/run-evals.mjs`, `.github/workflows/agent-evals.yml`).
- AI review and scheduled security workflows removed in 0.21.8 (`claude-review.yml`, `security-scan.yml`).

## Files Outside `.claude/` That MUST Still Sync

These live outside `.claude/` and are load-bearing for CI guards and generated mirrors:

| File | Sync strategy |
|------|---------------|
| `REVIEW.md` | Sync entirely if absent — `scripts/check-claude-wiring.mjs` errors without it. If present, merge new passes and exclusions while preserving the project's own severity bar and nit cap. |
| `scripts/check-claude-wiring.mjs` | Replace with template version, then re-run it. |
| `scripts/check-claude-wiring.test.mjs` | Replace with template version. |
| `scripts/sync-codex.mjs` | Replace with template version (if project uses generated Codex/Antigravity mirrors). |
| `scripts/sync-antigravity.mjs` | Replace with template version alongside `scripts/sync-codex.mjs`. |
| `scripts/sync-codex.test.mjs` | Replace with template version alongside `scripts/sync-codex.mjs`. |
| `scripts/antigravity-hooks.test.mjs` | Replace with template version alongside `.claude/scripts/_antigravity-hook.sh`. |

After syncing any of these, run `npm run sync:codex` (if present), `npm run check:claude`, and `npm test` before reporting success.

## Files to NEVER Sync

- `.claude/agent-memory/*`: project-specific memory, never overwrite.
- `.claude/references/template-sync-state.json`: generated locally by `update-practices`.
- `.claude/references/template-sync-ignore.md`: project-specific removal list; never overwrite local entries.
- `intent/**` and `tasks/**`: the project's own artifact chain.
- `CLAUDE.md`, `.claude/references/agent-registry.md`, `instructions.md`: project-tailored.
- `AGENTS.md`, `.agents/**`, `.codex/**`: generated Codex and Antigravity mirrors. Never sync or hand-edit them; regenerate with `npm run sync:codex` after template changes are applied.
- Any other file outside `.claude/` not named in the exception table above.

## Persist Sync State Schema

Write `.claude/references/template-sync-state.json`:

```json
{
  "lastSyncedCommit": "<SHA from git ls-remote>",
  "lastSyncedDate": "<today's date>",
  "deadUrls": ["<registry URLs that were unreachable this run>"]
}
```

## Step 8: Verbatim Report Format

```
BOOTSTRAP TEMPLATE SYNC:
  Template repo: <repo URL from source-urls.md>
  Template commit: <SHA from git ls-remote>
  [TEMPLATE-NEW] Added <path>: <description>
  [TEMPLATE-UPDATED] Updated <path>: <what changed (incremental merge)>
  [TEMPLATE-REWRITTEN] Replaced body of <path>: <upstream rewrite; project-specific bits re-applied: ...>
  [TEMPLATE-CURRENT] No changes needed: <list>
  [TEMPLATE-IGNORED] Skipped per template-sync-ignore.md: <list>
  [TEMPLATE-SKIPPED] Skipped <path or step>: <reason, e.g., project-specific, this repo is the template, commit already synced>

CHANGES APPLIED:
  [NEW] Added skill <name>: <reason>
  [NEW] Added rule <path>: <scope description>
  [NEW] Added agent-memory file <path>: <purpose>
  [NEW] Added hook <event>/<matcher>: <reason>
  [NEW] Added setting <key> = <value>: <reason>
  [UPDATED] Modified .claude/settings.json: <what changed>
  [UPDATED] Modified agent <name> frontmatter: <fields added/changed>
  [UPDATED] Modified skill <name> frontmatter: <fields added/changed>
  [UPDATED] Modified rule <path>: <what changed>
  [DEPRECATED] Replaced <old pattern> with <new pattern> in <file>
  [CURRENT] No changes needed for: <list>
  [PRUNED] Removed from <file>: <what was removed and why>
  [DEAD-URL] Flagged (strike 1) or removed (strike 2): <url list>

FEATURES IN USE:
  - Path-scoped rules: <count> rules in .claude/rules/
  - Agent memory: <count> files in .claude/agent-memory/
  - Hook events: <list of configured events>
  - Hook types: <command|http|prompt|agent|mcp_tool>
  - Advanced agent frontmatter: <agents using background/isolation/context/skills/memory>
  - Advanced skill frontmatter: <skills using context/agent>
  - Settings: <list of configured optional settings>

FEATURES AVAILABLE BUT NOT CONFIGURED:
  <list any features that could be enabled but aren't, with instructions>

HEALTH AUDIT:
  Wiring guard: <PASS|FAIL> (npm run check:claude)
  Hooks: <n> verified in both directions, <n> fire-only, <n> unverifiable
  Skills: <n> checked, <n> BROKEN, <n> DEGRADED
  Agents: <n> checked, <n> BROKEN, <n> DEGRADED
  Memory: <n> files, <n> stale entries, <n> gotchas not yet routed to LL-G
  CLAUDE.md: <bytes> / <ceiling>; <n> duplication findings
  [BROKEN] <what> in <file>: <why it cannot work> -> <fix applied, and how it was re-verified>
  [DEGRADED] <what> in <file>: <cost> -> <fix applied, or why reported instead>
  [SUGGESTION] <proposal>: <evidence it is needed> -- NOT applied, your call
  [HEALTHY] <what was verified, not just the name>
  [UNVERIFIABLE] <check>: <why it could not be run>

WEB SEARCH:
  Window: <cutoff date> to <today's date> (7 days)
  Version gate: <current Claude Code version>
  <N> of <total> results passed both gates
  [CORROBORATED] <finding>: applied to <file> (confirmed by <official source>)
  [UNCORROBORATED] <finding> (<url>, published <date>): reported only, not applied

CLAUDE CODE VERSION: <version from claude --version>
CURRENT DATE: <today's date>
SOURCES CHECKED: <count> of <total> fetched successfully
```

## Step 3: Detailed Comparison Checklist

Categorize every finding as **NEW**, **UPDATED**, **DEPRECATED**, or **CURRENT** across:

- **Core files:** skills, agents, `.claude/settings.json` (permissions, hooks, env), `tools.md`, `design-guardrails.md`.
- **Path-scoped rules (`.claude/rules/*.md`):** rules still valid for stack, new source directories needing rules, `paths:` globs matching real files, new official/community rule patterns.
- **Agent memory (`.claude/agent-memory/`):** `README.md`, `patterns.md`, `decisions.md`, `debugging.md` present; `README.md` conventions accurate; no stale/contradicted entries; `debugging.md` uses standard gotcha format.
- **Agent frontmatter:** `background`, `isolation`, `context`, `skills`, `memory`, `effort`, `maxTurns`.
- **Skill frontmatter:** `context: fork`, `agent` (takes effect only alongside `context: fork`), `model`, `effort`, `disable-model-invocation`.
- **Hook events & settings:** check against `.claude/references/hooks-and-settings.md` (single source of truth; add newly introduced upstream hook events/types/settings to that reference file rather than inline in a skill).
- **Cost / token efficiency (do NOT disable 1M context window):**
  - `effort:` set appropriately (`low`/`medium` mechanical, `medium` analysis, `high`/`max` orchestration/planning).
  - Cheapest fitting `model:` (`haiku` step-by-step, `sonnet` analysis, `opus` orchestration/planning).
  - Cache preservation (avoid unnecessary mid-session model switches).
  - Cheaper extractors (`pdftotext`, DOM reads over screenshots) noted in `tools.md` where relevant.
  - Long mechanical scans/fetches delegated to cheaper subagents.

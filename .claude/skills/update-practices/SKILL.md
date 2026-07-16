---
name: update-practices
model: opus
effort: high
description: Fetch latest Claude Code best practices and update the .claude/ folder configuration. Safe to run repeatedly.
user-invocable: true
argument-hint: (no arguments needed)
allowed-tools:
  - Read
  - Write
  - Edit
  - Glob
  - Grep
  - Bash
  - WebFetch
  - WebSearch
  - Agent
---

# Update Best Practices

You have been asked to update this repository's Claude Code configuration to the latest best practices. Follow these steps exactly.

## Important: Date Awareness

Check the current date FIRST. All best practices must be verified as current as of today's date. Do not rely on cached knowledge; use WebSearch to confirm that recommended versions, tools, and patterns are still current.

## Step 1: Read Current Configuration

1. Read `.claude/references/source-urls.md` to get the list of URLs to fetch. All external URLs used by this skill come from that registry; never hardcode URLs here.
2. Read `CLAUDE.md` in the repo root. Note its contents and version references.
3. Read `agents.md` in the repo root. Note registered agents.
4. Scan `.claude/skills/` using Glob. List all existing skills.
5. Scan `.claude/agents/` using Glob. List all existing agents.
6. Read `.claude/settings.json`. Note current settings.
7. Read `.claude/references/tools.md` (if it exists). Note current tools.
8. Read `.claude/references/design-guardrails.md` (if it exists). Note current guardrails.
9. Scan `.claude/rules/` using Glob. List all existing path-scoped rules.
10. Scan `.claude/agent-memory/` using Glob. List all existing memory files.

## Step 2: Fetch Latest Practices

Spin up parallel `explorer` agents to fetch and analyze sources. Use the custom `explorer` agent (defined in `.claude/agents/`), never the built-in `Explore` type: the built-in loads every MCP tool schema and blows the context window.

1. **Official sources subagent:** "Fetch all official Anthropic sources from the source URL registry. Extract: current Claude Code version, new features, deprecated features, new recommended settings/skills/agents/hooks, folder structure changes, new frontmatter fields for agents and skills, new hook events, new settings options. WHY: We need to know what changed officially to update the config accurately."

2. **Community sources subagent:** "Fetch all community sources from the source URL registry. Extract: new practical patterns, updated skill examples, agent configurations, workflow improvements, path-scoped rule patterns, agent memory patterns, HTTP hook patterns. WHY: Community sources capture battle-tested patterns ahead of official docs."

3. **Stack freshness subagent:** "Check the project's detected stack (from CLAUDE.md or dependency manifests) against current versions and best practices as of today's date. WHY: We need to ensure tools.md and design guardrails reflect the latest stable versions."

Each subagent must report any registry URL that is unreachable (404, DNS failure, repo gone) so Step 4 can track it. Wait for all subagents, then proceed.

## Step 2b: Sync from Bootstrap Template

The `.claude/` folder in this repo was originally scaffolded from the **claude-code-bootstrap** template. This step checks the template for any new or updated files that should be pulled in.

The template repo URL, tree API URL, and raw-content base URL all live in the "Bootstrap Template" section of `.claude/references/source-urls.md`. Read them from there.

### Guard 1: Skip if this repo IS the template

Run `git remote get-url origin` and compare it against the template repo URL from the registry. If they point at the same repository, **skip this entire step** and note `TEMPLATE-SKIPPED (this repo is the template)` in the report. Syncing the template repo against its own remote main would treat unpushed local work as drift and resurrect deliberately deleted files.

### Guard 2: Skip if the template has not changed

1. Get the template's current main commit SHA cheaply, without downloading content:
   ```bash
   git ls-remote <template-repo-url> refs/heads/main
   ```
2. Read `.claude/references/template-sync-state.json` (if it exists). It stores `lastSyncedCommit`, `lastSyncedDate`, and `deadUrls` from the previous run.
3. If the remote SHA equals `lastSyncedCommit`, **skip the rest of this step** and note `TEMPLATE-CURRENT (commit <SHA> already synced)` in the report.

### Fetch the template file tree

Fetch the tree API URL from the registry using Bash so you get raw JSON:

```bash
curl -s "<tree-api-url>"
```

The response contains every path in the template plus each file's git blob SHA. Extract all paths under `.claude/`; these are the canonical template files. Ignore files outside `.claude/` (CLAUDE.md, README.md, package.json, etc.) since those are project-specific.

### Categorize each template file by blob SHA

Do NOT use WebFetch to compare file contents. WebFetch runs pages through a model and can truncate or reformat long files, so it cannot support an exact comparison. Compare git blob SHAs instead; they are byte-exact and require no downloads for unchanged files.

For each `.claude/` path in the template tree:

1. **Check if it exists locally.** If not, check the ignore list (see below):
   - Listed in `.claude/references/template-sync-ignore.md` → mark as **TEMPLATE-IGNORED** (deliberately removed by this project; do not re-create).
   - Not listed → mark as **TEMPLATE-NEW**.
2. If it exists locally, compute the local blob SHA and compare with the tree entry's SHA:
   ```bash
   git hash-object <local-path>
   ```
   - SHAs match → mark as **TEMPLATE-CURRENT** (byte-for-byte identical, no action needed).
   - SHAs differ → download the template version's raw bytes into the scratchpad and diff locally:
     ```bash
     curl -s "<raw-base-url>/<path>" -o <scratchpad>/<path>
     git diff --no-index <local-path> <scratchpad>/<path>
     ```
     - The diff touches a few lines/sections (incremental edits) → mark as **TEMPLATE-UPDATED**.
     - The body has been substantially rewritten (most steps/sections changed, reordered, or replaced) → mark as **TEMPLATE-REWRITTEN**. A rewrite is when a merge would produce a Frankenstein file; the template's version is the new canonical source.

   Do not rely on quick heuristics like "the file exists, so it's current." A skill that exists locally can still be stale if its instructions were rewritten in the template. Always classify from the real diff.

### Files to sync (whitelist)

Only sync these categories of `.claude/` files from the template:

| Category | Path pattern | Sync strategy |
|----------|-------------|---------------|
| Skills | `.claude/skills/**` (SKILL.md plus supporting files under `references/`, `templates/`, `evals/`, etc.) | Sync new skills entirely, including their supporting files. For **TEMPLATE-UPDATED** files, merge new steps/sections but preserve project-specific customizations (e.g., custom matchers, stack-specific checks). For **TEMPLATE-REWRITTEN** files, **replace the body wholesale with the template version**, then re-apply only the genuinely project-specific additions (custom stack checks, custom paths). Do not try to merge a rewrite line-by-line. |
| Agents | `.claude/agents/*.md` | Sync new agents entirely. For **TEMPLATE-UPDATED** agents, update frontmatter fields and instructions but preserve project-specific `context` or `skills` overrides. For **TEMPLATE-REWRITTEN** agents, replace the body with the template version, preserving only project-specific `context`/`skills`/`tools` overrides. |
| Rules | `.claude/rules/*.md` | Sync new rules entirely. For **TEMPLATE-UPDATED** rules, update content but preserve custom `paths:` frontmatter if the project has different file structure. For **TEMPLATE-REWRITTEN** rules, replace the body with the template version, preserving only the project's custom `paths:` frontmatter. |
| Scripts | `.claude/scripts/*.sh` | Sync new scripts entirely. For existing scripts, replace with template version unless local version has project-specific logic (check for project-specific paths, env vars, or tool references). |
| References | `.claude/references/source-urls.md` | Merge: add any new URLs from template that are not already present. A URL may be removed only when it was verified unreachable on two consecutive runs (tracked via `deadUrls` in the sync state file). |
| References | `.claude/references/hooks-and-settings.md` | Generic catalog, not project-specific. Replace with the template version unless the local copy has project-specific notes appended, in which case merge new rows in. |
| References | `.claude/references/ux-laws.md` | Generic catalog, not project-specific. Replace with the template version unless the local copy has project-specific notes appended, in which case merge new entries in. |
| References | `.claude/references/infrastructure.md` | Do NOT sync; infrastructure is project-specific. |
| References | `.claude/references/tools.md` | Do NOT sync; tools depend on project stack. |
| References | `.claude/references/design-guardrails.md` | Do NOT sync; guardrails depend on project stack. |
| Settings | `.claude/settings.json` | Deep-merge: add new hooks, permissions, and env vars from template. Never remove existing entries. Preserve project-specific matchers and custom hooks. |
| Settings | `.claude/settings.local.json.example` | Replace with template version (it is just an example file). |

### Files to NEVER sync

- `.claude/agent-memory/*`: project-specific memory, never overwrite.
- `.claude/references/template-sync-state.json`: generated locally by this skill.
- `.claude/references/template-sync-ignore.md`: project-specific removal list; never overwrite local entries.
- Any file not in the `.claude/` directory.
- `CLAUDE.md`, `agents.md`, `instructions.md`: these are project-tailored.

### Apply template changes

For each **TEMPLATE-NEW** file:
- Create it locally with the template content.
- If the file previously existed and was deleted (visible in `git log`, or the user tells you), do not re-create it; add its path to `.claude/references/template-sync-ignore.md` instead and report it as TEMPLATE-IGNORED.

For each **TEMPLATE-UPDATED** file:
- Apply the sync strategy from the table above.
- When merging, use the non-destructive rules: never remove project-specific content, append/merge rather than replace.

For each **TEMPLATE-REWRITTEN** file:
- The upstream body is the new canonical source. Replace the local body with the template version per the sync strategy above.
- Before replacing, scan the local file for genuinely project-specific content (custom stack checks, custom paths, custom `paths:`/`context`/`skills` frontmatter). Re-apply ONLY those after pulling in the rewrite.
- Do not attempt a line-by-line merge of a rewrite; that produces an incoherent hybrid. Take the template body whole, then graft back the small project-specific pieces.

For each **TEMPLATE-IGNORED** file:
- Take no action. List it in the report so the user knows the ignore entry is still active.

### Persist sync state

After syncing, write `.claude/references/template-sync-state.json`:

```json
{
  "lastSyncedCommit": "<SHA from git ls-remote>",
  "lastSyncedDate": "<today's date>",
  "deadUrls": ["<registry URLs that were unreachable this run>"]
}
```

`deadUrls` drives the two-strikes rule for the source URL registry: a URL that appears in `deadUrls` from the previous run AND is unreachable again this run may be removed from `source-urls.md` (report the removal). A URL that recovers is dropped from `deadUrls`.

## Step 3: Compare and Identify Changes

Categorize findings as:

- **NEW:** Features or patterns not yet reflected in the current config.
- **UPDATED:** Patterns that exist but need modification to match current best practices.
- **DEPRECATED:** Patterns in use that are no longer recommended.
- **CURRENT:** Patterns that already match best practices (no action needed).

Check each of these areas explicitly:

### Core files
- Skills (all template skills present and current)
- Agents (all template agents present and current)
- Settings (permissions, hooks, env)
- Tools reference
- Design guardrails

### Path-scoped rules (.claude/rules/*.md)
- Are existing rules still valid for the current stack?
- Are there new path patterns that should have rules (e.g., new source directories added)?
- Do rule frontmatter `paths:` patterns still match the actual file structure?
- Are there new best-practice rule templates from official/community sources?

### Agent memory (.claude/agent-memory/)
- Does the directory exist? If not, it should be created.
- Are the standard files present (README.md, patterns.md, decisions.md, debugging.md)?
- Is the README still accurate about conventions?
- Have any memory entries become stale or contradicted by current code?
- Does `debugging.md` have the standard gotchas structure? If empty or unstructured, initialize with the template from init-repo.

### Agent frontmatter
Review each agent for new frontmatter fields:
- `background`: Should any agents run in the background?
- `isolation`: Should security agent use isolation?
- `context`: Should any agents have injected context?
- `skills`: Should any agents be bound to specific skills?
- `memory`: Should any agents read agent-memory files on startup?

### Skill frontmatter
Review each skill for new frontmatter fields:
- `context: fork`: Should any skills run in isolated context?
- `agent`: Should any skills be bound to a specific agent?
- Are `model` assignments still optimal?

### Hook events
- Are all recommended hooks configured?
- Are there new hook events available that should be adopted?
- Are any configured hooks using deprecated event names or syntax?
- Should HTTP hooks be added for team workflows?
- Are matchers using the correct syntax?

The full event catalog, hook types, and matcher syntax live in `.claude/references/hooks-and-settings.md`; check configured hooks against it. When the fetched sources reveal a hook event or type not yet in that reference, add it there (not into a skill body), so the catalog stays in one place.

### Settings
Check configured settings against the optional-settings catalog in `.claude/references/hooks-and-settings.md`. That reference is the single source of truth for which settings exist; do not maintain a settings list inside this skill. If the latest Claude Code version introduces a new setting, add it to that reference.

### Cost / token efficiency

Audit the config for token-efficiency patterns. Do NOT recommend disabling the 1M context window (the 200K default is intentionally not used in this template).

- **Per-prompt effort:** Are skills setting `effort:` in frontmatter where appropriate? Mechanical, step-by-step skills should use `low` or `medium`; analysis skills `medium`; orchestration/planning `high` or `max`. Flag skills missing `effort` where a non-default would save tokens.
- **Model routing:** Are skills and agents assigned the cheapest model that fits? Step-by-step: haiku. Analysis: sonnet. Orchestration/planning: opus. Flag any opus assignment that could be sonnet/haiku.
- **Cache preservation:** Skills and agents should not switch model mid-session unnecessarily (model switches invalidate the prompt cache). Flag skills that change model partway through a multi-step flow.
- **Input format swaps:** Where the project ingests PDFs, web pages, or screenshots, prefer cheaper extractors (`pdftotext` for PDFs, an agent-browser / DOM read over screenshot capture, code knowledge graphs over raw repo dumps). Note any tool reference in `tools.md` that should be added.
- **Subagent delegation:** Long mechanical work (log scans, repo-wide searches, doc fetches) should be delegated to subagents with cheaper models so the main session's cache stays intact.

## Step 4: Implement Changes

For each NEW or UPDATED item:

1. Determine which file(s) need to change.
2. Make the change. Follow the non-destructive merge rules:
   - Never remove custom project-specific content.
   - Append new sections rather than replacing existing ones.
   - For JSON files, deep-merge; preserve existing keys.
   - For agent-memory files, never overwrite; only add missing files.
   - For rules, preserve existing rules; only add new ones or update paths.
3. For DEPRECATED items: update the pattern to the recommended alternative.

**Canonical skill and agent inventory:** derive it from the template tree fetched in Step 2b (every `.claude/skills/*/SKILL.md` and `.claude/agents/*.md` path in the tree). Do not maintain a hardcoded list here; it drifts. Every template skill and agent should exist locally and be current unless it is listed in `template-sync-ignore.md`. If Step 2b was skipped because this repo is the template, the local tree IS the canonical inventory.

Update `.claude/references/tools.md` if any tools have new versions or new tools should be added.

Update `.claude/references/design-guardrails.md` if UI best practices have changed.

Review skill frontmatter and update `model`, `disable-model-invocation`, `context`, and `agent` fields if recommendations have changed.

Review agent frontmatter and update `background`, `isolation`, `context`, `skills`, and `memory` fields if recommendations have changed.

Review hook configuration:
- Verify all hook events are still valid.
- Add new recommended hooks.
- Update matchers if file paths have changed.
- Add HTTP hooks if `allowedHttpHookUrls` is configured and team webhooks are in use.

Review settings against the catalog in `.claude/references/hooks-and-settings.md`, and ensure `settings.local.json.example` exists if it should.

Apply the two-strikes dead-URL rule to `.claude/references/source-urls.md` using `deadUrls` from the sync state file (see Step 2b).

## Step 5: Prune CLAUDE.md Files

Review all CLAUDE.md files in the hierarchy. Remove:
- Advice the model now handles natively (check against current model capabilities)
- Outdated version references
- Redundant rules that duplicate parent CLAUDE.md content

Keep each CLAUDE.md focused and under 200 lines.

## Step 6: Prune and Validate Rules

Review `.claude/rules/*.md` files:
- Remove rules that duplicate CLAUDE.md content (rules should be path-specific, not general).
- Verify `paths:` patterns still match actual files in the project.
- Update rules if stack conventions have changed.
- Remove rules for deleted source directories.

## Step 7: Update Documentation

1. Update `CLAUDE.md` if skill or agent inventory changed.
2. Update `agents.md` if agent inventory changed.
3. Update `instructions.md` if usage patterns, available features, or configuration options changed. Ensure it documents:
   - Path-scoped rules (`.claude/rules/*.md`)
   - Agent memory (`.claude/agent-memory/`)
   - All agent and skill frontmatter fields
   - All hook events and types (command, http, prompt, agent)
   - All settings options including settings.local.json overrides

## Step 8: Report

Determine the installed Claude Code version by running `claude --version` locally; use web sources only as a fallback if the CLI is unavailable.

Print a diff-style summary:

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

CLAUDE CODE VERSION: <version from claude --version>
CURRENT DATE: <today's date>
SOURCES CHECKED: <count> of <total> fetched successfully
```

## Idempotency

Running this skill twice in a row must produce no changes the second time. Every change must be conditional: only apply if the current state differs from the target state. The sync state file makes this cheap; when the template commit has not moved and no source reports changes, the second run should complete with zero writes.

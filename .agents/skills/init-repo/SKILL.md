---
name: init-repo
description: "Build or rebuild the .claude/ folder with best practices. Use when setting up Claude Code in a new or existing repository. Run plan-repo first for new projects."
---

> Generated from `.claude/skills/init-repo/SKILL.md` by `scripts/sync-antigravity.mjs --with-skills`. Edit the source, then run `node scripts/sync-antigravity.mjs --with-skills`.

# Initialize Repository for Claude Code

You have been asked to initialize or upgrade this repository's Claude Code configuration. Follow these steps exactly.

## Important: Date Awareness

Before fetching any best practices, check the current date. All recommendations must reflect best practices as of today's date -- not cached knowledge. Use WebSearch to verify current versions.

## Step 1: Read Plan (if exists)

1. Check if `tasks/plan-repo.md` exists. If it does, read it and use it as the basis for all decisions below. The plan contains the chosen stack, file structure, design guardrails, and tools.
2. If no plan exists, that is fine -- proceed with auto-detection.

## Step 2: Read Current State

1. Read `CLAUDE.md` in the repo root (if it exists). Note its contents.
2. Read `.claude/references/agent-registry.md` (older repos: root `agents.md`) if it exists. Note its contents.
3. Read `README.md` in the repo root (if it exists). Identify the project's tech stack, purpose, and conventions.
4. Scan the `.claude/` folder (if it exists) using Glob. List all existing files.
5. Read `.claude/settings.json` (if it exists). Note current settings.
6. Check for existing `.claude/rules/*.md` files. Note any path-scoped rules already defined.
7. Check for existing `.claude/agent-memory/` directory. Note any shared knowledge files.

## Step 3: Fetch Best Practices (BP + LL-G + Web)

**3a. Load BP (Best Practices knowledge base) -- PRIMARY SOURCE**

1. Fetch `https://raw.githubusercontent.com/BoardPandas/BP/main/llms.txt` to get all concern categories.
2. Identify which concerns are relevant based on the detected tech stack from Step 2.
3. Fetch each relevant concern's `llms.txt` index (e.g., `practices/claude-config/llms.txt`, `practices/safety/llms.txt`).
4. Read ALL FOUNDATIONAL entries -- these apply to every repo regardless of stack.
5. Read RECOMMENDED entries whose `applies-to` tags match this project's tech stack.
6. Use these practices as the primary guide for all configuration decisions in Steps 4-12. BP entries include CHECK (verify if already applied) and IMPLEMENT (steps to adopt) sections -- follow the IMPLEMENT steps when building new config.

Key BP practices to apply during init:
- `claude-config/hierarchical-claude-md` -- structure CLAUDE.md correctly
- `claude-config/rule1-llg-integration` -- add RULE 1 to CLAUDE.md
- `claude-config/path-scoped-rules` -- create .claude/rules/ files
- `claude-config/hook-configuration` -- configure hooks
- `safety/credential-deny-list` -- add deny rules to settings.json
- `safety/read-only-first-rule` -- add RULE 0 to CLAUDE.md
- `context-management/compact-and-handoff` -- add context management guidance
- `context-management/agent-memory` -- initialize .claude/agent-memory/
- `documentation/plan-with-lessons-learned` -- add lessons learned requirement
- `ai-workflow/plan-then-execute` -- add planning guidance
- `ai-workflow/anti-patterns` -- add anti-pattern awareness

**3b. Load LL-G (Lessons Learned & Gotchas)**

1. Fetch `https://raw.githubusercontent.com/BoardPandas/LL-G/main/llms.txt` to get all technologies.
2. For each technology matching this project's stack, fetch its sub-index.
3. Note any HIGH-severity gotchas relevant to the config being generated (e.g., TypeScript strict mode, Drizzle version pinning, Better Auth import paths).
4. Only note them at this point. Do not write them into any file yet; Step 14d seeds them into `.claude/agent-memory/debugging.md` exactly once.

**3c. Fetch from web sources**

1. Read the source URL registry at `.claude/references/source-urls.md`.
2. Spin up parallel `explorer` agents to fetch and analyze sources. Use the custom `explorer` agent (defined in `.claude/agents/`), never the built-in `Explore` type -- the built-in loads every MCP tool schema and blows the context window.
   - **Subagent 1:** "Fetch official Anthropic sources and extract current Claude Code version, features, and recommended patterns. WHY: We need the latest official conventions to generate an up-to-date config."
   - **Subagent 2:** "Fetch community sources and extract practical tips, skill patterns, and agent patterns. WHY: Community sources have battle-tested patterns not in official docs."
   - **Subagent 3:** "Search the web for Claude Code best practices published in the last seven days, following the Live Web Search section of the source URL registry exactly. Resolve today's date and the current Claude Code version first, then apply both gates as hard filters: no visible publish date on or after (today - 7 days) means discard, and not tied to the current version's minor line means discard. Report kept and discarded counts. WHY: The registry only lists sources that update on their own cadence; this catches changed behavior in the current release that none of them have written up yet."
3. For URLs that fail to fetch, log the failure and continue. Do not halt.
4. Subagent 3's findings are claims, not sources. Use one only when an official source corroborates it or you can verify it against the installed CLI; report the rest to the user without writing them into the config. Zero surviving results is a normal outcome -- never widen the window or relax a gate to produce findings.

**Priority:** BP entries take precedence over web sources when they conflict. BP entries are vetted and tested across 28 repos; web sources may be outdated or context-specific.

## Step 4: Detect Stack and Generate Design Guardrails

1. Identify the tech stack from dependency manifests, file types, and the plan (if available).
2. Use WebSearch to find current best practices (as of today's date) for the detected stack:
   - Coding conventions and naming patterns
   - Project structure conventions
   - UI/design patterns (if the project has a frontend)
   - Testing patterns
3. If the project has a UI, generate `.claude/references/design-guardrails.md` with stack-specific UI/design SLA guidelines. Include:
   - Component size limits and composition patterns
   - Styling conventions for the chosen approach
   - Accessibility requirements (WCAG level, required ARIA patterns)
   - Performance budgets (bundle size, image optimization, lazy loading)

## Step 5: Analyze Gaps (against BP + web sources)

Compare the current `.claude/` folder against BP practices and web sources. For each applicable BP practice, run its CHECK items to see if the repo already follows it. Identify:

- **BP FOUNDATIONAL gaps** -- practices every repo should have but this one is missing
- **BP RECOMMENDED gaps** -- practices matching this tech stack that are missing
- Missing configuration files (settings.json, agents, skills)
- Outdated patterns or deprecated features in use
- Skills that should exist but do not
- Agent definitions that are missing or incomplete
- Settings that should be updated (including credential deny-list from BP `safety/credential-deny-list`)
- Missing tools.md entries for detected stack tools
- Missing `.claude/rules/*.md` files for path-scoped conventions (including bp-check.md and llg-check.md)
- Missing `.claude/agent-memory/` directory for team knowledge
- Missing RULE 0 (read-only-first), RULE 1 (LL-G), RULE 3 (BP) in CLAUDE.md
- Missing context management guidance (compact at 50%, handoff docs, anti-patterns)
- LL-G gotchas relevant to this stack that should be noted in agent-memory/debugging.md

## Step 6: Build or Update

For each gap identified, create or update the file. Follow these rules:

- **Template source:** The canonical template is the `BoardPandas/claude-code-bootstrap` repository (the repo this `.claude/` folder was copied from). Its CLAUDE.md skill table and `.claude/references/agent-registry.md` define the full template skill and agent lists. If a template skill, agent, rule, or script is missing here, copy it from that repo (via `git clone` to a temp directory or raw GitHub fetch) rather than reinventing it.
- **Non-destructive:** Never overwrite custom project-specific settings. Merge with existing config.
- **Skills:** Ensure all template skills exist in `.claude/skills/`. If additional skills are relevant to the detected tech stack, add them.
- **Agents:** Ensure all template agents exist in `.claude/agents/`. Add others if relevant.
- **Scripts:** Ensure `.claude/scripts/` contains every shell script referenced by hooks in `settings.json` (e.g., `session-start-kb-check.sh`, the commit-check scripts). A hook that points at a missing script fails on every session start. Copy missing scripts from the template source.
- **Settings:** Update `.claude/settings.json` with recommended permissions and hooks. Preserve existing custom entries.
- **Tools reference:** Update `.claude/references/tools.md` with stack-specific CLI tools, install commands, and usage patterns. **Important:** There is no local Docker, no local Postgres, no local Redis -- server-side infrastructure runs remotely on the project's profile, Cloudflare or Railway (see `.claude/references/infrastructure.md`). Do not add local infrastructure tools (docker, docker-compose, psql, redis-cli). Desktop targets are the exception: their build, signing, and packaging toolchains are local and do belong in tools.md. Preserve the existing **Available MCP Servers** section that documents all MCP integrations available to Claude Code.
- **CLAUDE.md:** Build a hierarchical CLAUDE.md structure:
  - Update root `CLAUDE.md` with project-specific stack info and conventions.
  - Plan (but do not create) subfolder CLAUDE.md files where distinct rules will apply.
  - Budget by **bytes, not lines**: 16 KB for CLAUDE.md, 20 KB for all always-on context. A line budget keeps passing while single lines grow to thousands of characters. `npm run check:claude` fails on either ceiling.
  - Do not restate what `instructions.md` or `.claude/references/` already cover. Detail goes down into those; CLAUDE.md keeps pointers. Do not duplicate the skill table either — skills are auto-discovered and their own `description:` is what triggers them; list only which ones are slash-only.
  - **Required section — `## Commands`:** the build, test, and lint commands, one line each. This is the first thing a new session needs and the last thing anyone remembers to write down.
  - **Required section — `## Verifying your work`:** the single command that proves a change is good, an example of healthy output, and an explicit statement that a failing check is never to be skipped, deleted, or narrowed. Make verification part of the definition of done.
  - **Required section — `## Things Claude Gets Wrong`:** seeded from the LL-G entries loaded in Step 3 for this project's stack, plus anything the gap analysis surfaced. The standing rule to write into the section: when a mistake happens twice, the correction goes here.
  - Include in the Planning section: "Every plan MUST end with a Lessons Learned / Gotchas section. After implementation, route discoveries to LL-G via `/add-lesson`, not to local files only."
  - If the project uses the artifact chain, document it: `intent.md` → `spec.md` → `plan.md` → diff, with the owner and approver of each.
- **Agent registry:** Update `.claude/references/agent-registry.md` to register all agents, preserving project-specific content. A lowercase root `agents.md` from an older template moves there: it collides with Codex's `AGENTS.md` on case-insensitive filesystems (Windows, default macOS), where one silently overwrites the other.
- **README.md:** If a README exists, add or update the "Claude Code" section. Do not alter other sections.

## Step 7: Configure Path-Scoped Rules

Read `.agents/skills/init-repo/references/scaffold-templates.md` ("Step 7: Path-Scoped Rule Example & Inventory") before creating rules.

Create `.claude/rules/*.md` with conditional `paths:` YAML frontmatter so rules load only when working with matching files (`paths:` is the only scoping key Claude Code reads; prune any glob that matches zero files in this repo so `npm run check:claude` passes). Always create `.claude/rules/tests.md`, `.claude/rules/bp-check.md`, and `.claude/rules/llg-check.md`, plus stack-specific rules (`frontend.md`, `styles.md`, `api.md`, `database.md`, or per-package monorepo rules) per the reference inventory.

## Step 8: Initialize Agent Memory

Create `.claude/agent-memory/` as a version-controlled team-shared knowledge base containing `README.md`, `patterns.md`, `decisions.md`, and `debugging.md`, using the exact `debugging.md` template and `README.md` guidelines in `.agents/skills/init-repo/references/scaffold-templates.md` ("Step 8: Agent Memory").

## Steps 9 & 10: Optimize Skill and Agent Frontmatter

Apply the skill and agent frontmatter guidelines in `.agents/skills/init-repo/references/scaffold-templates.md` ("Steps 9 & 10: Skill and Agent Frontmatter Field Reference"):

1. **Skills:** every skill must declare `model:` (`haiku` mechanical, `sonnet` analysis, `opus` orchestration/planning) OR bind `agent:` alongside `context: fork` (without `context: fork`, `agent:` is ignored, and every `allowed-tools` entry must be in the bound agent's `tools:`). Set `disable-model-invocation: true` on slash-only skills and appropriate `effort:` levels.
2. **Agents:** set `memory: project` on `reviewer`, `architect`, `builder`, `explorer`, and `tester`; set `skills: [plan-repo, spec-developer]` on `architect`; set `isolation: worktree` only on file-writing agents (`builder`), never on read-only analysis agents; avoid setting `background: true` in frontmatter (pass `run_in_background` per spawn instead). Add optional fields only where they provide clear value.

## Step 11: Configure Hooks

Read `.claude/references/hooks-and-settings.md` for the full catalog: every hook event, the five hook types (command, http, prompt, agent, mcp_tool), the `if:` field, matcher syntax, and the "Hooks to configure based on project needs" recommendations. That file is the single source of truth — do not re-paste the event table into any skill or CLAUDE.md.

For init, configure the always-on defaults from that reference (`SessionStart` knowledge-base reminder, `PreToolUse` on `Bash` with `if: "Bash(git commit*)"`, `Stop` and `Notification` sounds). The always-on hooks invoke scripts in `.claude/scripts/`; confirm those scripts exist (Step 6) before wiring the hooks, since a hook pointing at a missing script fails on every session start. Wire each one as `bash "$CLAUDE_PROJECT_DIR"/.claude/scripts/<name>.sh`, never by a cwd-relative path, which stops resolving once the shell leaves the repo root.

Then decide on the recommended development/team hooks: in an interactive session, ask the user with AskUserQuestion which ones they want. In a non-interactive or autonomous run, configure only the always-on defaults and list the skipped options in the final report so the user can add them later.

## Step 12: Configure Settings

Update `.claude/settings.json` with all relevant settings. Deep-merge with existing.

### Core settings (always configure)

The core-settings JSON block (permissions allow/deny lists, credential deny-list, `env`, `plansDirectory`) lives in `.claude/references/hooks-and-settings.md` under "settings.json core settings". That file is the single source of truth; read it and apply it rather than working from memory. Do not re-paste the block into any skill, CLAUDE.md, or instructions file.

### Optional settings and settings.local.json

The full optional-settings catalog (`attribution.*`, `autoUpdatesChannel`, `sandbox.*`, `worktree.*`, `language`, `allowedHttpHookUrls`, `alwaysThinkingEnabled`, `disableAllHooks`), the `settings.json` vs `settings.local.json` split, and the `.claude/settings.local.json.example` template all live in `.claude/references/hooks-and-settings.md`. Read it, then create `settings.local.json.example` from the template there.

For attribution, language, and autoUpdatesChannel preferences: in an interactive session, ask the user with AskUserQuestion before setting them. In a non-interactive or autonomous run, leave them unset and list them in the final report as pending decisions. Configure the rest based on project analysis.

## Step 12b: Create the Review Policy and Multi-Runtime Mirrors

Required: `scripts/check-claude-wiring.mjs` errors when `REVIEW.md` is missing or
missing a required section.

### REVIEW.md

Copy the template's `REVIEW.md` to the repo root, then tailor it to this project:

- Keep all four required section headings verbatim -- `## Passes`,
  `## What "Important" means here`, `## Cap the nits`, `## Do not report`.
- Adjust **Passes** to the detected stack, set the nit cap deliberately (3 by default), and point **Do not report** at what this project's formatter and CI cover.

### Codex and Antigravity mirrors

Copy the template's `scripts/sync-codex.mjs`, `scripts/sync-antigravity.mjs`, `scripts/sync-codex.test.mjs`, and `scripts/antigravity-hooks.test.mjs` (plus `.claude/scripts/_antigravity-hook.sh`), add `"sync:codex": "node scripts/sync-codex.mjs"` and `"check:codex": "node scripts/sync-codex.mjs --check"` to `package.json`, and prefix `check:claude` with `node scripts/sync-codex.mjs --check && `. Then run `npm run sync:codex`. It generates `AGENTS.md`, `.agents/` (skills, agents, rules, `hooks.json`), and `.codex/` from `CLAUDE.md` and `.claude/`; never hand-edit those. Diff any pre-existing files there first and move project-specific rules into the Claude source.

Verify before moving on (`npm run check:claude && npm test` — both must pass).

## Step 13: Create instructions.md

Create or update `instructions.md` in the repo root covering what `.claude/` contains, how to invoke each skill, hierarchical `CLAUDE.md` architecture, path-scoped rules (`.claude/rules/*.md`), agent memory (`.claude/agent-memory/`), agent/skill frontmatter fields, a pointer to `.claude/references/hooks-and-settings.md` (do not restate its catalog), subagent best practices, phase-based planning, context management, and how to add skills/agents/rules/memory.

## Step 14: BP Verification and Audit

Verify the result and create the audit trail using `.agents/skills/init-repo/references/scaffold-templates.md` ("Step 14: BP Audit & LL-G Seed Templates"):

1. **14a:** Re-run CHECK items for every FOUNDATIONAL BP practice on the reference checklist; apply any missing ones now.
2. **14b:** Check RECOMMENDED practices matching this stack's tags from BP (record only; do not auto-apply).
3. **14c:** Write `.claude/bp-audit.md` using the reference template.
4. **14d:** Append matching HIGH-severity LL-G gotchas (from Step 3b) to `.claude/agent-memory/debugging.md` using the reference snippet. If BP or LL-G are unreachable, note that in the final report.

## Step 15: Reset the Changelog and Version Baseline

A project cloned from this template inherits the template's own `CHANGELOG.md` and `package.json` version. Classify before touching anything:

1. **15a:** If `package.json` is still named `claude-code-bootstrap` and `scripts/new-project.mjs` exists, run `node scripts/new-project.mjs` and skip to 15d. Otherwise classify `CHANGELOG.md` as **Absent** (create `0.0.1`), **Template-inherited** (reset to `0.0.1`), or **Project-owned** (**do not reset**; preserve both `CHANGELOG.md` and manifest version). If ambiguous, ask with `AskUserQuestion` (or preserve untouched in autonomous runs and flag in the report).
2. **15b & 15c:** When resetting, write the fresh `0.0.1` `CHANGELOG.md` (deleting any template-inherited `CHANGELOG-archive.md`) and reset only existing manifest version fields (`package.json`, `pyproject.toml`, `Cargo.toml`, `*.csproj`) per `.agents/skills/init-repo/references/scaffold-templates.md` ("Step 15: Fresh `0.0.1` Changelog & Manifest Baseline").
3. **15d:** Keep `.claude/rules/commit-changelog.md` as shipped so subsequent commits advance cleanly from `0.0.1`.

## Step 16: Report

After verification completes, print a summary listing:

- Files created (with paths)
- Files updated (with what changed)
- Skills available (with model assignments and frontmatter)
- Agents registered (with frontmatter enhancements)
- Path-scoped rules created
- Agent memory initialized
- Hooks configured (events, matchers, types)
- Settings configured (highlighting opt-in features)
- Hierarchical CLAUDE.md plan
- Tools detected and added to tools.md
- Design guardrails generated (if applicable)
- BP audit score and failing practices (from Step 14)
- Changelog and version baseline (from Step 15): whether it was reset to `0.0.1`, or preserved because the changelog is project-owned or ambiguous
- Pending user decisions: recommended hooks and optional settings that were skipped in a non-interactive run (from Steps 11 and 12)
- Merge conflicts where an existing value was preserved over a recommended one
- Any warnings or issues encountered
- Features available but not yet configured (with instructions to enable later)

## Non-Destructive Merge Rules

When merging with existing configuration:

1. For JSON files: deep-merge objects. Never replace an existing value; only add missing keys. If a recommended value conflicts with an existing one, keep the existing value and list the conflict in the Step 16 report for the user to decide.
2. For markdown files: append new sections. Do not remove existing sections.
3. For skills: if a skill already exists with custom content, do not overwrite. Only update if the existing skill references deprecated features.
4. For agents: same rule as skills.
5. For rules: if a rule file already exists, preserve it. Only add new rule files.
6. For agent-memory: never overwrite existing memory files. Only create missing ones.
7. The Step 15 changelog reset is the single deliberate exception to rule 2, and it is narrow: it may only overwrite a `CHANGELOG.md` that Step 15a classified as template-inherited. A project-owned or ambiguous changelog falls back under rule 2 and is preserved.

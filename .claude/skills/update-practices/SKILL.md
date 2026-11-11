---
name: update-practices
model: opus
effort: high
description: Fetch latest Claude Code best practices, update the .claude/ folder configuration, and audit its health -- verifying hooks actually fire, skills are startable, agents are reachable, memory is still true, and CLAUDE.md is lean. Safe to run repeatedly.
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
3. Read `.claude/references/agent-registry.md` (older repos: root `agents.md`). Note registered agents.
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

4. **Live web search subagent:** "Search the web for Claude Code best practices published in the last seven days, following the Live Web Search section of the source URL registry exactly. Resolve today's date and the current Claude Code version first, then apply both gates as hard filters: discard anything without a visible publish date on or after (today - 7 days), and discard anything not tied to the current version's minor line. Report kept and discarded counts, and mark every surviving result as corroborated or uncorroborated against the official sources. WHY: The registry only covers sources that update on their own cadence; this catches changed behavior in the current release that no listed source has written up yet."

Each subagent must report any registry URL that is unreachable (404, DNS failure, repo gone) so Step 4 can track it. Wait for all subagents, then proceed.

**The web search subagent's findings are claims, not sources.** Apply one only if an official source corroborates it, or you can verify it directly against the installed CLI. Everything else surfaces in the Step 8 report as a suggestion and is not written into config. Zero surviving results is a normal outcome, not a failed run: never widen the window or relax a gate to produce findings.

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

### Categorize and apply template changes

Read `${CLAUDE_SKILL_DIR}/references/template-sync-and-report.md` in full before proceeding. It contains the exact git blob-SHA categorization commands, the `.claude/` sync whitelist, the load-bearing files outside `.claude/` that MUST still sync (`REVIEW.md`, `scripts/check-claude-wiring*.mjs`, `scripts/sync-codex*.mjs`, `scripts/sync-antigravity.mjs`, `scripts/antigravity-hooks.test.mjs`), the NEVER-sync list, and the `template-sync-state.json` schema.

1. Fetch the tree API URL via `curl -s "<tree-api-url>"` (never WebFetch, which can truncate or reformat files) and classify each whitelisted path by git blob SHA (`git hash-object <local-path>`) into **TEMPLATE-IGNORED** (in `template-sync-ignore.md`), **TEMPLATE-NEW**, **TEMPLATE-CURRENT**, **TEMPLATE-UPDATED** (incremental diff), or **TEMPLATE-REWRITTEN** (body substantially replaced).
2. Apply changes per category:
   - **TEMPLATE-NEW:** create locally unless previously deleted intentionally (record in `template-sync-ignore.md` as TEMPLATE-IGNORED).
   - **TEMPLATE-UPDATED:** merge non-destructively per the whitelist table; preserve project-specific customizations.
   - **TEMPLATE-REWRITTEN:** replace the body wholesale with the upstream version, then re-apply only genuinely project-specific customizations (never merge a rewrite line-by-line).
   - **TEMPLATE-IGNORED:** skip and report as active.
3. Never sync `.claude/agent-memory/*`, `template-sync-state.json`, `template-sync-ignore.md`, `intent/**`, `tasks/**`, `CLAUDE.md`, `instructions.md`, or generated Codex/Antigravity mirrors (`AGENTS.md`, `.agents/**`, `.codex/**` — regenerate those in Step 7 via `npm run sync:codex`).
4. Write `.claude/references/template-sync-state.json` (`lastSyncedCommit`, `lastSyncedDate`, `deadUrls`) per the reference schema. Remove a registry URL from `source-urls.md` only on two consecutive unreachable runs (`deadUrls` two-strikes rule).
5. Run `npm run check:claude` and `npm test` after syncing outside-`.claude/` guard/sync scripts.

## Step 2c: Health Audit of the `.claude/` Folder

The sync steps above make the config **current**. This step makes it **work**. Those are different properties, and the gap between them is where the coding experience actually degrades: a config can be perfectly up to date and still be inert -- a hook that never fires, a skill whose documented trigger cannot start it, an agent nothing can reach, a memory entry the code contradicts. None of that surfaces as an error. It just quietly stops helping.

### 2c.0 Run the mechanical guard first

```bash
npm run check:claude
```

The guard already asserts everything statically checkable: rule frontmatter uses `paths:` (not Cursor's `globs:`/`alwaysApply:`), every glob matches a real file, no hook matcher uses permission-rule syntax or an uncompilable regex, referenced hook scripts exist, no hook silences both stderr and its exit code, blocking hooks write to stderr, no hook interpolates the nonexistent `$CLAUDE_FILE_PATH`, frontmatter keys are hyphenated, every skill that forks into an agent only lists `allowed-tools` that agent holds, and always-on context stays under ceiling.

**Do not re-implement any of those checks here.** If the guard fails, fix what it reports before continuing -- a health audit layered on broken wiring reports noise. And if a property below turns out to be mechanically checkable, add it to the guard instead of describing it here: a check that runs in CI beats a check that runs when someone remembers to invoke this skill.

If the guard is absent (a project that took `.claude/` from the template without `scripts/check-claude-wiring.mjs` and its `package.json` entry), that is itself a BROKEN finding: every property above is currently unenforced there. Sync the script and the npm entry from the template, then run it before continuing.

### 2c.1 Hooks: verify behavior, not just wiring

The guard proves a hook is *wired*. It cannot prove the hook *does its job*, and every silent-failure mode in `kb/claude-code/` is behavioral rather than structural: a guard clause on `$TOOL_NAME` (which does not exist) makes the entire hook unreachable while looking correct; `exit 1` does not block where `exit 2` does; a parser selected with `command -v python3` finds the Windows Store stub and returns empty fields with no error anywhere.

For each hook in `settings.json`, run its script directly with a synthetic payload on stdin and assert **both** directions:

```bash
printf '{"session_id":"audit","tool_name":"Bash","tool_input":{"command":"git commit -F .git/MSG"}}' \
  | bash .claude/scripts/<script>.sh; echo "exit=$?"
```

- **Fires when it should.** Feed a payload the hook exists to act on. Assert the expected exit code (`2` to block for PreToolUse/PostToolUse, `0` for advisory) and that any refusal text lands on **stderr** -- a blocking hook's stdout is discarded, so the block arrives with no reason attached.
- **Advice actually reaches the model.** An advisory hook on any event other than SessionStart, UserPromptSubmit, UserPromptExpansion or PostModelSwitch must print `{"hookSpecificOutput":{"hookEventName":"<event>","additionalContext":"..."}}` and nothing else on stdout. Plain text there goes to the debug log, so a hook that echoes its reminder passes every other check here and is still never read. Parse its stdout as JSON in the audit; `.claude/scripts/_hook-context.sh` is the fix.
- **Stays silent when it should not fire.** Feed a payload it must ignore. Assert exit 0 and no output. This is the direction that catches an over-broad filter, and the first direction never catches it.
- **Field extraction actually resolved.** Confirm the hook read the field it depends on. A hook that silently extracts an empty string is indistinguishable from a hook that deliberately chose not to fire, which is what makes this failure survive for months.

A hook that cannot be exercised in both directions is a finding, not a pass.

### 2c.2 Skills

For every `.claude/skills/*/SKILL.md`:

- **The documented trigger can actually start it.** `disable-model-invocation: true` means no plain-English phrase will ever start the skill. Any skill setting it must appear in the CLAUDE.md table as `/command`, never as a phrase. This exact defect once shipped across four skills at once, including the repo's headline workflow, so check it every run.
- **The description carries its own triggers.** The description is all that routing sees. It should name the concrete phrases and nouns a user would say, not restate the skill's title.
- **Referenced files resolve.** Follow every relative path and `${CLAUDE_SKILL_DIR}` reference in the body (`references/`, `templates/`, `evals/`). A skill pointing at a renamed file fails only when someone finally runs it.
- **Frontmatter keys are real.** Only documented keys have any effect; an invented or misspelled key is silently ignored rather than rejected.
- **Body size.** A SKILL.md past a few hundred lines should push detail into `references/` and keep the body as the procedure.

### 2c.3 Agents

For every `.claude/agents/*.md`:

- **Registered.** Present in `.claude/references/agent-registry.md`, with a description matching the agent's own.
- **Reachable.** Every agent named by a skill's `agent:` field, by an `Agent(<name>)` entry in a `tools:` allowlist, or by a subagent instruction in a skill body must resolve to a real agent file. A dangling name fails at dispatch time, mid-task.
- **Tool list still fits the job.** A read-only review agent should not hold `Write` beyond its report path; an implementation agent needs `Edit`/`Write`. Over-broad tool lists are the usual drift.
- **`isolation: worktree` agents are briefed about it.** They cannot see uncommitted work and will report confidently on stale content. Each must orient with `git rev-parse` / `status` before acting.

**Suggesting new agents.** Propose one only on evidence, never because a role sounds useful. Evidence means: a skill that repeatedly describes the same specialist work inline; a task recurring in git history with no agent covering it; a domain in the stack that no current agent handles. Each suggestion states the trigger, model, minimum tool list, and what it would have caught had it existed. **Do not create the agent** -- put it in the report and let the user decide. An unused agent is permanent context cost with no return.

### 2c.4 Agent memory

- **Standard files present:** `README.md`, `patterns.md`, `decisions.md`, `debugging.md`.
- **Entries are still true.** Spot-check claims against the current code. A memory the code contradicts is worse than no memory, because it is confidently wrong and nothing re-reads it.
- **Entries are durable, not session notes.** Memory earns its place only for what does not follow from reading the repo.
- **Gotchas were routed upstream.** Per CLAUDE.md, discoveries belong in LL-G via `/add-lesson`. A `debugging.md` accumulating entries that never went upstream is a routing failure; list them so they can be contributed.
- **Size.** Memory files load for every agent with `memory:` set, so count them as always-on context for those agents.

### 2c.5 CLAUDE.md hierarchy

- **Byte budget.** `wc -c` every CLAUDE.md. Budget by bytes, never lines -- a line budget keeps passing while single lines grow to thousands of characters.
- **No duplication downward.** A subfolder CLAUDE.md or a rule that restates the root is pure repeated cost in every session that loads it.
- **Rules stay path-specific.** General guidance belongs in CLAUDE.md; a rule with no `paths:` loads in **every** session.
- **`@imports` sit at column 0.** A mid-line `@path` is ordinary prose and silently never loads, which is invisible from inside the session.
- **Tables match reality, both directions.** Check the skills and agents tables against the actual directories for listed-but-missing *and* present-but-undocumented.
- **Nothing the model now handles natively.** Prune per Step 5.

### 2c.6 Review policy and the artifact chain

The guard proves the config is wired. These two cover whether it still *behaves*, and
neither is statically checkable beyond existence.

**Review policy.** Read `REVIEW.md` against the last month of actual defects:

- Passes that have never caught a real defect are DEGRADED — recommend removal.
- Defect classes that reached main and are not covered by any pass or guard check are
  BROKEN — recommend which of the two should own it.
- A policy that only ever grows is a policy nobody finishes reading. Report its size trend.

**Artifact chain.** Spot-check `intent/` and `tasks/`:

- Intents stuck at `Status: Draft` for a long time: report the count, and that the gate is
  doing its job or nobody is minding it.
- Specs with a non-empty **Flagged Concerns** table and no recorded resolution.
- Plans in `tasks/` with no corresponding spec, for work large enough to have needed one.

These are SUGGESTION-level unless an artifact is malformed. Do not chase the user's backlog.

### 2c.7 Classify every finding

- **BROKEN** -- wired but provably does not work (a hook that cannot fire, a skill whose documented trigger cannot start it, a dangling reference). Fix in Step 4.
- **DEGRADED** -- works, but wastefully or misleadingly (over-broad tool lists, duplicated instructions, stale memory, oversized files). Fix in Step 4.
- **SUGGESTION** -- new agents, restructuring, anything discretionary. Report only; never apply.
- **HEALTHY** -- verified working. Say what was verified, not just the name.

A check that could not be run is its own finding. Never record it as HEALTHY.

## Step 3: Compare and Identify Changes

Walk every category in `${CLAUDE_SKILL_DIR}/references/template-sync-and-report.md` ("Step 3: Detailed Comparison Checklist") and categorize findings as **NEW**, **UPDATED**, **DEPRECATED**, or **CURRENT** across:

1. **Core files, path-scoped rules (`.claude/rules/*.md`), and agent memory (`.claude/agent-memory/`).**
2. **Skill and agent frontmatter** (`model`, `effort`, `context: fork` + `agent`, `memory`, `isolation`, `skills`, `maxTurns`).
3. **Hook events and settings** against `.claude/references/hooks-and-settings.md` (single source of truth — add newly discovered upstream hook events, types, or settings to that reference rather than inside a skill).
4. **Cost / token efficiency** (`effort` levels, cheapest fitting `model` routing, prompt-cache preservation by avoiding mid-session model switches, cheaper extractors, and subagent delegation — do NOT disable the 1M context window).

## Step 4: Implement Changes

1. **NEW / UPDATED / DEPRECATED items:** apply non-destructively (append rather than replace sections; deep-merge JSON preserving existing keys; never overwrite `.claude/agent-memory/` entries; preserve existing rules while adding or rescoping `paths:`; replace deprecated patterns with current equivalents).
2. **BROKEN / DEGRADED findings from Step 2c:**
   - Fix **BROKEN** first and re-run the check that caught it (confirm behavioral hook fixes by re-running both payload directions, never just re-reading).
   - Fix **DEGRADED** when unambiguous (dangling refs, duplicated instructions, phrase triggers on `disable-model-invocation: true` skills); report judgment calls (narrowing tools, deleting memory) instead of guessing.
   - Never apply **SUGGESTION** items; add mechanically checkable invariants to `scripts/check-claude-wiring.mjs` and verify they fail on the target defect.
3. **Canonical inventory:** derive from the Step 2b template tree (or local tree when this repo IS the template) minus `template-sync-ignore.md`. Never hardcode a list here.
4. Refresh `tools.md`, `design-guardrails.md`, skill/agent frontmatter, hooks (`allowedHttpHookUrls` where team webhooks apply), `.claude/settings.json` / `settings.local.json.example` (against `.claude/references/hooks-and-settings.md`), and `source-urls.md` (applying the `deadUrls` two-strikes rule).

## Step 5: Prune CLAUDE.md and Rules

1. **CLAUDE.md hierarchy:** remove advice current models handle natively, outdated version references, and rules duplicated from a parent `CLAUDE.md`. Keep root `CLAUDE.md` under 16 KB and total always-on context under 20 KB (`wc -c`).
2. **`.claude/rules/*.md`:** remove rules duplicating `CLAUDE.md`, verify `paths:` globs match real files, update conventions for stack changes, and delete rules for removed directories.

## Step 7: Update Documentation and Regenerate Mirrors

1. Update `CLAUDE.md` and `.claude/references/agent-registry.md` if skill or agent inventory changed.
2. Update `instructions.md` if features or configuration options changed (covering path-scoped rules, agent memory, skill/agent frontmatter, hook events/types, and settings overrides).
3. If `scripts/sync-codex.mjs` exists, run `npm run sync:codex` last, after every other edit (`AGENTS.md`, `.agents/`, and `.codex/` are generated mirrors; `npm run check:claude` fails while stale).

## Step 8: Report

Determine the installed Claude Code version by running `claude --version` locally; use web sources only as a fallback if the CLI is unavailable.

Print the diff-style summary using the exact section structure in `${CLAUDE_SKILL_DIR}/references/template-sync-and-report.md` ("Step 8: Verbatim Report Format"), covering `BOOTSTRAP TEMPLATE SYNC`, `CHANGES APPLIED`, `FEATURES IN USE`, `FEATURES AVAILABLE BUT NOT CONFIGURED`, `HEALTH AUDIT`, `WEB SEARCH`, `CLAUDE CODE VERSION`, `CURRENT DATE`, and `SOURCES CHECKED`.

## Idempotency

Running this skill twice in a row must produce no changes the second time. Every change must be conditional: only apply if the current state differs from the target state. The sync state file makes this cheap; when the template commit has not moved and no source reports changes, the second run should complete with zero writes.

The Step 2c health audit is exempt from the zero-writes expectation but not from idempotency: it re-runs its checks every time (they are cheap, and a hook that worked last week can break from an unrelated change), but a second consecutive run must find nothing left to fix. A health finding that reappears on a clean re-run means the fix did not take -- report that, rather than applying it again.

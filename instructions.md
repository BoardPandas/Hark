# Claude Code Starter Template -- Instructions

## What This Template Is

This repository provides a pre-configured `.claude/` folder that gives Claude Code a set of skills, agents, and settings aligned with current best practices. It works in two modes:

1. **Clone for new projects** -- Start a new repo with Claude Code already configured.
2. **Copy into existing projects** -- Drop the `.claude/` folder (plus CLAUDE.md and agents.md) into any existing repo to add Claude Code capabilities.

## Folder Structure

```
.claude/
  agents/                  # Custom agent definitions
    architect.md           # Phase-based planning and system design
    reviewer.md            # Code review agent
    security.md            # Security analysis agent
    performance.md         # Performance analysis agent
    explorer.md            # Codebase exploration and research agent
    ux-reviewer.md         # UX review against Laws of UX and Gestalt principles
    builder.md             # Implementation engineer for parallel team work
    tester.md              # Test runner and failure analyst
  agent-memory/            # Persistent cross-session knowledge
    README.md              # Conventions and usage guide
    patterns.md            # Recurring patterns and conventions
    decisions.md           # Technical decisions with rationale
    debugging.md           # Failed approaches and dead ends
    builder/               # Per-agent memory (MEMORY.md + topic files)
  rules/                   # Conditional instructions (paths: frontmatter)
    llg-check.md           # LL-G knowledge base check before code changes
    bp-check.md            # BP best practices check before config changes
    commit-changelog.md    # Changelog and version bump enforcement
  scripts/                 # Hook scripts
  skills/                  # Executable skill definitions
    plan-repo/SKILL.md     # Pre-init project planning
    init-repo/SKILL.md     # Repository initialization
    update-practices/SKILL.md  # Best practice updates
    spec-developer/SKILL.md   # Interview-driven feature specs
    security-scan/SKILL.md # Security scanning
    performance-review/SKILL.md  # Performance analysis
    dependency-audit/SKILL.md    # Dependency checking
    test-scaffold/SKILL.md      # Test generation
    doc-sync/SKILL.md           # Documentation sync
    mermaid-diagram/SKILL.md    # Diagram generation
    add-lesson/SKILL.md         # Add gotcha to LL-G knowledge base
    add-practice/SKILL.md       # Add best practice to BP knowledge base
    apply-practice/SKILL.md     # Apply BP practice to a target repo
    ux-review/SKILL.md          # UX review against Laws of UX
    merge-worktrees/SKILL.md    # Merge worktrees/branches into main, clean up
    agy-execute-plan/SKILL.md   # Hand a plan to the Antigravity CLI, then verify
  references/
    source-urls.md         # URL registry for fetching best practices
    infrastructure.md      # Fixed infrastructure stack (do not modify)
    tools.md               # CLI tools reference (auto-populated per stack)
    ux-laws.md             # Laws of UX / Gestalt reference for ux-review
    hooks-and-settings.md  # Canonical hook/settings catalog
    design-guardrails.md   # UI/design SLA (generated for frontend projects)
    template-sync-ignore.md    # Template files this project deliberately removed (update-practices skips them)
    template-sync-state.json   # Last-synced template commit + dead-URL strikes (written by update-practices)
  settings.json            # Project-level Claude Code settings
  settings.local.json.example  # Template for personal (git-ignored) overrides
CLAUDE.md                  # Master project rules for Claude Code
agents.md                  # Agent registry (see agents.md docs below)
instructions.md            # This file
README.md                  # GitHub-facing README
tasks/                     # Saved plans and specs (created on first use)
```

---

## Core Workflow

### 1. Plan → Init → Build → Update

```
plan repo  →  initialize repo  →  build features  →  update practices
```

- **plan-repo** plans the stack, generates README, creates design guardrails and tools reference.
- **init-repo** reads the plan and configures everything.
- **spec-developer** plans individual features.
- **update-practices** keeps the config current.

### 2. Plan in One Session, Execute in Another

For any non-trivial work: plan in one session, then start a fresh session to execute. This keeps context clean and prevents contamination from planning artifacts.

### 3. Phase-Based Planning (Not Timeline-Based)

All planning uses phases, never dates or time estimates:

| Phase | Focus | Exit Criteria |
|-------|-------|---------------|
| Foundation | Project setup, core architecture, tooling | Builds, tests run, deploys |
| Core | Primary features, data models, integrations | All primary flows work E2E |
| Polish | Error handling, edge cases, accessibility | 80%+ coverage, no critical bugs |
| Ship | Deployment, monitoring, documentation | Production-ready |

---

## Skills Reference

### plan-repo

- **Trigger:** "plan repo", "plan project", "plan stack", "recommend stack"
- **What it does:** Interviews you about the project requirements (what it does, target platform, scale, constraints) -- but does NOT ask you to pick a stack. Instead, it spins up parallel research subagents to compare current options (Go vs Rust vs TS, shadcn vs MUI vs Mantine, Drizzle vs Prisma, etc.) as of today's date, then recommends the best stack for your specific project with trade-offs. You approve or override, then it generates README, design guardrails, and tools reference.
- **When to use:** Before init-repo on new projects, or when evaluating a stack change.
- **Output:** Stack recommendation with trade-offs, plan file (`tasks/plan-repo.md`), README draft, design guardrails (if UI), tools reference.
- **Key concept:** It recommends, you decide. Every recommendation is backed by current research, not cached opinions.

### init-repo

- **Trigger:** "initialize repo", "init repo", "set up claude code"
- **What it does:** Reads the plan (if available), detects the stack, fetches best practices, and builds or updates all configuration files. Generates hierarchical CLAUDE.md files and stack-specific design guardrails. Non-destructive merge with existing config.
- **When to use:** After plan-repo, after copying `.claude/` into an existing project, or to rebuild from scratch.
- **Output:** Summary of all files created or modified.

### update-practices

- **Trigger:** "update practices", "refresh best practices", "update claude config"
- **What it does:** Checks today's date, fetches all sources, compares against current config, implements changes. Prunes CLAUDE.md files of stale advice. Updates tools.md and design guardrails.
- **When to use:** Periodically or after Claude Code updates.
- **Safe to repeat:** Running it twice in a row produces no changes the second time.

### spec-developer

- **Trigger:** "spec developer", "plan feature", "spec this feature"
- **What it does:** Explores the codebase with parallel subagents, then asks 20+ clarifying questions about the feature. Generates a 500-700 line implementation plan covering architecture, data models, test plan, error handling, and rollback strategy. Saves to `/tasks`.
- **When to use:** For any feature larger than a single file change.
- **Key concept:** Plan only -- does not implement. Start a fresh session to execute.
- **Variant:** If retrying after a failed implementation, it documents previous attempts to avoid dead ends.

### security-scan

- **Trigger:** "security scan", "security audit", "check security"
- **What it does:** Leaked secrets, OWASP Top 10, dependency CVEs, input validation gaps.
- **Scope:** Optionally pass a file or directory path.

### performance-review

- **Trigger:** "performance review", "perf review", "check performance"
- **What it does:** N+1 queries, memory leaks, bundle size, caching, algorithms, build optimization.
- **Scope:** Optionally pass a file or directory path.

### dependency-audit

- **Trigger:** "dependency audit", "audit dependencies", "check deps"
- **What it does:** Outdated packages, known vulnerabilities, unused dependencies across all detected package managers.

### test-scaffold

- **Trigger:** "scaffold tests", "generate tests", "add test coverage"
- **What it does:** Detects test framework, finds untested modules, generates test stubs matching existing patterns.
- **Scope:** Optionally pass a file or directory.

### doc-sync

- **Trigger:** "sync docs", "update docs", "fix documentation"
- **What it does:** Cross-references documentation against code. Finds stale references, incorrect examples, missing docs.

### mermaid-diagram

- **Trigger:** "mermaid diagram", "generate diagram", "visualize data flow"
- **What it does:** Explores the codebase and generates Mermaid diagrams (data flow, architecture, sequence, state machine, ER). Saves to `docs/diagrams/`.
- **When to use:** For debugging user-reported issues without reading code, for documentation, or for understanding complex systems.

### add-lesson

- **Trigger:** "add lesson", "add gotcha", "add to LL-G"
- **What it does:** Adds a gotcha or lesson learned to the LL-G knowledge base (`BoardPandas/LL-G` repo) via GitHub API. No local clone needed.
- **When to use:** After discovering a non-obvious failure pattern during implementation.

### add-practice

- **Trigger:** "add practice", "add best practice", "add to BP"
- **What it does:** Adds a new best practice entry to the BP knowledge base (`BoardPandas/BP` repo) via GitHub API.
- **When to use:** When establishing a proven pattern that should apply across repos.

### apply-practice

- **Trigger:** "apply practice", "apply BP"
- **What it does:** Applies a specific best practice from the BP knowledge base to the current or a target repository.
- **When to use:** When onboarding a repo or setting up tooling that should follow established patterns.

### ux-review

- **Trigger:** "ux review", "review UX", "usability review"
- **What it does:** Reviews UI code against Laws of UX and Gestalt principles (via the ux-reviewer agent and `.claude/references/ux-laws.md`). Produces severity-ranked findings with specific improvement recommendations.
- **Scope:** Optionally pass a file, directory, or component name.

### merge-worktrees

- **Trigger:** "merge worktrees", "merge and clean up branches"
- **What it does:** Inventories every worktree and local branch, shows a plan and asks for confirmation, commits pending work, merges every branch into main with `--no-ff`, pushes, then removes the worktrees and deletes the merged branches. Merge conflicts are hard stops; nothing is deleted until the merge is pushed.
- **When to use:** To consolidate all outstanding work into main and tear down the leftovers.

### agy-execute-plan

- **Trigger:** "have AGY run the plan", "let Antigravity build this", "agy the plan then double-check it"
- **What it does:** Hands an existing Claude-written plan to the Antigravity CLI (`agy`) for autonomous end-to-end execution, then independently verifies the result against the plan via tests and the git diff (not AGY's self-reported log), fixes whatever AGY left incomplete or broke, and reports an honest blocked/partial/complete status.
- **When to use:** When a plan in `tasks/` should be carried out by AGY rather than by Claude directly.

---

## Agents Reference

All agents are registered in [agents.md](agents.md) at the repo root. This file serves as the agent registry -- Claude Code reads it to discover available agents.

### architect

- **File:** `.claude/agents/architect.md`
- **Model:** opus
- **Mode:** plan
- **Purpose:** Phase-based planning, tech stack evaluation, file structure design. Always uses phases (Foundation, Core, Polish, Ship), never timelines.

### reviewer

- **File:** `.claude/agents/reviewer.md`
- **Model:** sonnet
- **Mode:** plan
- **Purpose:** Code review for correctness, naming, DRY, error handling, type safety, and standards compliance.

### security

- **File:** `.claude/agents/security.md`
- **Model:** opus
- **Mode:** plan
- **Purpose:** OWASP Top 10, secrets detection, dependency vulnerabilities, input validation.

### performance

- **File:** `.claude/agents/performance.md`
- **Model:** sonnet
- **Mode:** plan
- **Purpose:** Query optimization, memory leaks, I/O, frontend rendering, algorithms, build optimization.

### explorer

- **File:** `.claude/agents/explorer.md`
- **Model:** sonnet
- **Mode:** plan
- **Purpose:** Codebase exploration, online research, doc fetching, context gathering. Always include a "why" when spawning.

### ux-reviewer

- **File:** `.claude/agents/ux-reviewer.md`
- **Model:** sonnet
- **Mode:** plan
- **Purpose:** UX review of UI code against Laws of UX and Gestalt principles, producing severity-ranked findings.

### builder

- **File:** `.claude/agents/builder.md`
- **Model:** sonnet
- **Mode:** acceptEdits
- **Purpose:** Implementation engineer. Turns a plan, spec, or task into working, tested code matching existing conventions. Spawn one builder per independent file set to avoid edit conflicts.

### tester

- **File:** `.claude/agents/tester.md`
- **Model:** sonnet
- **Purpose:** Detects the project's test runner, executes the relevant suite, and reports pass/fail with actionable failure detail. Verifies behavior; does not implement fixes.

---

## Hierarchical CLAUDE.md Architecture

CLAUDE.md files are loaded top-down: root user level → project level → subfolder level. Only relevant files load.

### Rules

1. **Root CLAUDE.md** contains project-wide rules, stack info, global conventions.
2. **Subfolder CLAUDE.md** files only exist where subfolder-specific rules differ from root (e.g., `frontend/CLAUDE.md` for UI conventions).
3. **`.claude/rules/*.md`** files with `paths:` frontmatter provide conditional instructions that only load when working with matching file paths. Use these instead of subfolder CLAUDE.md files for fine-grained scoping. Rules without `paths:` load every session.
4. **Nested `.claude/` directories** are natively supported: `subdir/.claude/skills/`, `agents/`, and `workflows/` load automatically when working in that subfolder. The closest directory wins on a name collision, disambiguated as `<dir>:<name>`.
5. A landing page task never loads your backend CLAUDE.md. Context stays narrow.
6. Keep each file focused and under 200 lines.
7. Prune after every model update -- remove what the model handles natively.
8. Do NOT bloat CLAUDE.md with generic advice the model already knows.

### When to Create Subfolder CLAUDE.md

- The subfolder has a different language or framework than the root
- The subfolder has distinct naming conventions, file structure, or testing patterns
- The subfolder is a separate deployable (e.g., monorepo package)

### When NOT to Create Subfolder CLAUDE.md

- The subfolder follows the same conventions as root
- The rules would just repeat or slightly extend root rules
- The subfolder is a simple utility folder

---

## Subagent Best Practices

### Always Offload to Subagents

- Online research and doc fetching
- Codebase exploration and pattern discovery
- Log analysis and debugging
- Context gathering before implementation

### Always Include a "Why"

Every subagent prompt should explain WHY you need the information:

- **Bad:** "How does auth work?"
- **Good:** "How does auth work, because we're adding rate limiting and need to know where to hook into the auth middleware."

The "why" dramatically reduces noisy and overlapping results from parallel subagents.

### Parallel Exploration

When torn between implementation approaches, spin up parallel Explorer subagents for each approach. Pass all results back to the main session and let it decide.

### Subagents Are Resumable

You can resume a specific subagent to continue its research. Use this to drill deeper without starting over.

### Background by Default

Subagents run in the background by default (v2.1.198+): the main session keeps working and is notified on completion. Subagents can spawn nested subagents up to 5 levels deep. To restrict which subagents an agent may spawn, list `Agent(agent_type)` entries in its `tools:` allowlist instead of allowing the whole Agent tool.

---

## Skill Frontmatter

Skills support optional fields for model and invocation control:

```yaml
---
name: my-skill
description: What this skill does
user-invocable: true
disable-model-invocation: true   # Only manual /skillname invocation
model: haiku                      # Which model runs this skill
context: fork                     # Run in isolated subagent context
allowed-tools:
  - Read
  - Glob
---
```

**Model selection guidelines:**
- `haiku` — Well-defined step-by-step skills (localization, release, formatting)
- `sonnet` — Analysis and research skills (code review, exploration)
- `opus` — Orchestration and planning skills (spec developer, architect)

**Additional frontmatter fields:**
- `user-invocable: false` — Hide the skill from the / menu while keeping it as background knowledge Claude can still draw on (distinct from `disable-model-invocation`, which blocks auto-triggering but keeps manual invocation)
- `context: fork` — Run skill in an isolated subagent, preventing context contamination
- `effort: low|medium|high|xhigh|max` — Override reasoning effort level. `xhigh` (Opus 4.7+) typically beats `max` on cost/quality
- `keep-coding-instructions: true` — Preserve coding-style instructions when the skill switches output styles
- `${CLAUDE_SKILL_DIR}` — Reference the skill's own directory for relative file paths
- Skills in nested `.claude/skills/` subdirectories are auto-discovered
- Put the most important trigger phrases in the first 250 characters of `description` — recent releases truncate skill descriptions around that length in some surfaces

---

## Context Management Tips

### Prevent Context Contamination

- **Plan/execute separation:** Plan in one session, execute in another.
- **Code bias fix:** If Claude is stuck in bad existing patterns, build the feature in isolation in a fresh empty folder, then port it into the main project.
- **`/rewind`:** Instead of arguing with Claude after a wrong turn, rewind to the last good point and re-guide.
- **Document failed attempts:** For stubborn bugs, have Claude write a document of all attempted fixes before starting a new session. The new session loads the document and avoids dead ends.
- **`/handoff`:** Create a handoff document (goal, progress, what worked, what failed, next steps) before ending a session. Load it in the fresh session as sole context.
- **Modularize aggressively:** Files over 500 lines consume massive context. Regularly break them apart.

### Context Window Management

- Keep CLAUDE.md under 200 lines
- Use `/compact` proactively around 50% context (disable auto-compact in `/config` for manual control)
- Start fresh conversations for unrelated topics
- Break tasks small enough to complete in under 50% context usage
- System prompt + tools consume ~10% of context. Enable `ENABLE_TOOL_SEARCH: "true"` in settings to lazy-load MCP tools and save tokens.
- Preserve the prompt cache: lock the MCP/tool list and model at the start of a session. Adding tools or switching models mid-session invalidates the cached prefix and inflates cost.
- For long-running sessions, set `ENABLE_PROMPT_CACHING_1H` to extend the prompt-cache TTL from 5 minutes to 1 hour.

---

## Tools Reference

The file `.claude/references/tools.md` lists all CLI tools the project uses. Claude Code reads this before running commands. If a tool is missing, Claude checks tools.md for the install command and offers to install it.

The file is populated by `plan-repo` and `init-repo` based on the detected stack. You can also add entries manually.

---

## Design Guardrails (UI Projects)

For projects with a frontend, `init-repo` generates `.claude/references/design-guardrails.md` with stack-specific UI/design SLA guidelines. These are sourced from current best practices as of the init date and enforced via the CLAUDE.md.

Guardrails cover: component patterns, styling conventions, accessibility requirements, performance budgets, and consistency rules.

---

## Hooks

The template includes hooks in `.claude/settings.json`:

- **SessionStart:** Surfaces the LL-G / BP knowledge-base reminder once per session.
- **PreToolUse (git commit):** Blocks PowerShell here-string syntax, reminds about the changelog/version bump, and blocks commits that do not stage `CHANGELOG.md` (merge commits and `SKIP_CHANGELOG=1` are exempt).
- **PreToolUse (EnterPlanMode):** Prompts a knowledge-base check before planning.
- **PostToolUse (git commit):** Prompts evaluation of whether the committed work should be contributed back to LL-G or BP.
- **Stop:** Bell sound when Claude finishes a task (useful with multiple sessions).
- **Notification:** Bell sound when Claude needs attention.

To add custom hooks, edit `.claude/settings.json`. Supported hook events:

- `PreToolUse` / `PostToolUse` / `PostToolUseFailure` -- Before/after/on-failure tool execution
- `PostToolBatch` -- After a parallel tool batch resolves
- `SessionStart` / `SessionEnd` -- Session lifecycle
- `Stop` / `StopFailure` -- Turn completion (success / API error)
- `SubagentStart` / `SubagentStop` -- Subagent lifecycle
- `Notification` -- System notifications, including background-agent events (`agent_needs_input`, `agent_completed`)
- `MessageDisplay` -- As assistant message text is displayed (transform or hide output, redact secrets)
- `PreCompact` / `PostCompact` -- Before/after context compaction
- `UserPromptSubmit` -- Before user prompt processing
- `UserPromptExpansion` -- When a slash command expands
- `InstructionsLoaded` -- When CLAUDE.md or `.claude/rules/*.md` files load
- `ConfigChange` -- When configuration files change during session
- `WorktreeCreate` / `WorktreeRemove` -- Git worktree operations
- `PermissionRequest` / `PermissionDenied` -- Permission lifecycle
- `TeammateIdle` / `TaskCompleted` / `TaskCreated` -- Agent team and task events
- `CwdChanged` / `FileChanged` -- Working directory and file change events
- `Elicitation` / `ElicitationResult` -- MCP structured input request events
- `Setup` -- Triggered via `--init`, `--init-only`, or `--maintenance` flags

Hook types: `command` (shell), `http` (POST JSON to URL), `prompt` (single-turn LLM), `agent` (multi-turn subagent), `mcp_tool` (direct MCP tool invocation).

Each hook entry accepts an optional `if:` field using permission-rule syntax (e.g., `Bash(git *)`) to filter when the hook fires. Reduces overhead on unrelated tool calls. Matchers can also target tool input parameters with `Tool(param:value)` syntax (e.g., `Agent(model:opus)`).

Hooks can return structured output beyond allow/block: `PostToolUse` can rewrite any tool's output (`hookSpecificOutput.updatedToolOutput`), `Stop`/`SubagentStop` can feed text back and continue the turn (`hookSpecificOutput.additionalContext`), and `SessionStart` can rescan skill directories (`reloadSkills: true`) or set the session title. See `.claude/references/hooks-and-settings.md` for the full catalog, including a `PermissionRequest` prompt-hook pattern that auto-approves known-safe operations.

---

## Agent Memory

The `.claude/agent-memory/` directory stores persistent, version-controlled knowledge that agents accumulate:

| File | Purpose |
|------|---------|
| `README.md` | Conventions and usage guide |
| `patterns.md` | Recurring code patterns and conventions |
| `decisions.md` | Key technical decisions with rationale |
| `debugging.md` | Failed approaches and dead ends |

Agents with `memory: project` read this directory on startup. Any agent can write here to share knowledge across sessions. See `.claude/agent-memory/README.md` for conventions.

---

## Power User Tips

- **`/add-dir`** -- Add a second project directory to copy proven implementations between projects.
- **Verbose mode** (`/config` -> verbose -> true) -- Shows token count and reasoning trace. Read the trace to learn Claude's terminology, then use it in prompts.
- **`/statusline`** -- Shows model name, context battery, git branch, unstaged changes. Essential with multiple sessions.
- **`/rewind`** (or `/undo`) -- Return to last good point instead of arguing about a wrong turn.
- **`/fork`** -- Branch current session within conversation. Use `--fork-session` with `--continue` to fork from recent sessions.
- **`/handoff`** -- Create a summary document before ending a session for seamless continuation.
- **`/recap`** -- Get a context summary when returning to a session. Configurable in `/config`.
- **`/release-notes`** -- View latest changes in the current Claude Code version.
- **`/team-onboarding`** -- Generate a teammate ramp-up guide from your local Claude Code usage.
- **`--worktree` / `-w`** -- Start Claude Code in an isolated git worktree for parallel work.
- **`--bare`** -- Skip hooks, LSP, and plugin sync for scripted/CI calls.
- **`-n` / `--name`** -- Set a display name for the session at startup.
- **`--from-pr`** -- Resume sessions linked to a GitHub PR.
- **`claude agents`** -- List all configured agents from the CLI.

---

## Customizing for Your Project

### Editing CLAUDE.md

Update root CLAUDE.md with your project's stack, conventions, and standards. Keep under 200 lines. Create subfolder CLAUDE.md files only where distinct rules apply.

### Adding New Skills

1. Create a folder in `.claude/skills/` with your skill name.
2. Create `SKILL.md` inside with YAML frontmatter (`name`, `description`, `user-invocable: true`).
3. Add optional frontmatter: `disable-model-invocation`, `model`, `agent`, `effort`, `context: fork`.
4. Write step-by-step instructions in the markdown body.
5. Update the skill table in CLAUDE.md and this instructions.md file.

### Adding New Agents

1. Create a markdown file in `.claude/agents/` named after the agent.
2. Add YAML frontmatter: `name`, `description`, `model`, and optionally `tools`, `permissionMode`, `maxTurns`, `skills`, `memory`, `isolation`, `background`, `disallowedTools`.
3. Write the agent's role, focus areas, and behavior.
4. Register the agent in [agents.md](agents.md).
5. Update CLAUDE.md.

**Advanced agent fields:**
- `skills:` — Preload specific skills into the agent for progressive disclosure
- `memory:` — Persistent memory scope (`user`, `project`, or `local`; local lives in git-ignored `.claude/agent-memory-local/<agent-name>/`)
- `context:` — Additional instructions injected into the agent's system prompt
- `isolation: worktree` — Run in a temporary git worktree
- `background: true` — Run asynchronously without blocking
- `disallowedTools:` — Remove specific tools from inherited tool lists
- `effort:` — Override reasoning effort (`low`, `medium`, `high`, `xhigh`, `max`)
- `initialPrompt:` — First message sent to the agent on startup
- `Agent(agent_type)` in `tools:` — Restrict which specific subagents this agent may spawn

### Updating the Source URL Registry

Add URLs to `.claude/references/source-urls.md`. The next `update-practices` run will include them.

### Personal Settings Overrides

Create `.claude/settings.local.json` for personal settings (git-ignored). Overrides `.claude/settings.json`.

---

## Troubleshooting

- **Skill not triggering:** Check `user-invocable: true` in SKILL.md frontmatter.
- **Agent not found:** Ensure the agent file is in `.claude/agents/` and registered in [agents.md](agents.md).
- **Settings not applied:** Precedence (highest wins): managed-settings.json (org policy) → CLI flags → `.claude/settings.local.json` → `.claude/settings.json` → `~/.claude/settings.local.json` → `~/.claude/settings.json`. Deny rules always win regardless of tier.
- **Hooks not running:** Verify hook event name and matcher in settings.json. Run `/doctor`.
- **Stale practices:** Run "update practices" -- it checks today's date and fetches current recommendations.
- **Context overload:** Use `/compact`, break into smaller tasks, or start a fresh session.

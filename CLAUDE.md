# Project Rules

This repository is a Claude Code starter template. It provides a ready-to-use `.claude/` configuration folder that can be cloned for new projects or copied into existing ones.

## Workflow: Plan First, Then Init

1. Say **"plan repo"** to choose stack, generate README, create design guardrails.
2. Say **"initialize repo"** to configure Claude Code using the plan.
3. Say **"update practices"** periodically to stay current.

For features: say **"spec developer"** to generate a detailed plan, then start a fresh session to implement it.

## Coding Standards

- Handle errors explicitly -- never swallow exceptions silently.
- Validate inputs at system boundaries (user input, API responses, file I/O).
- Avoid premature abstraction. Three similar lines are better than a forced helper.
- Do not add comments for self-explanatory code. Add comments only when the "why" is non-obvious.
- Files over 500 lines should be split. Large files consume excessive context.

## Hierarchical CLAUDE.md Architecture

CLAUDE.md files load top-down: root user level, then project level, then subfolder level. Only relevant files load -- a frontend task never loads the backend CLAUDE.md.

- Root `CLAUDE.md` — Project-wide rules, stack, global conventions (this file).
- Subfolder `CLAUDE.md` — Only when subfolder has distinct conventions (e.g., `frontend/CLAUDE.md` for UI rules, `backend/CLAUDE.md` for API rules).
- `.claude/rules/*.md` — Conditional instructions with `paths:` frontmatter. Only load when working with matching file paths.
- Nested `.claude/` directories are first-class: `subdir/.claude/skills|agents|workflows/` load automatically when working in that subfolder; the closest one wins on name collision (disambiguated as `<dir>:<name>`).
- Keep each file focused. Prune after every model update -- remove what the model handles natively.
- Do NOT bloat CLAUDE.md with generic advice the model already knows.

## Subagent Usage

Always and aggressively offload to subagents: online research, doc fetching, log analysis, codebase exploration. This keeps the main context narrow.

- **Always include a "why"** in every subagent prompt. Not just what to find, but why you need it. "How auth works for rate limiting because we're improving rate limiting" beats "how auth works."
- **Parallel exploration:** When torn between approaches, spin up parallel Explore subagents for each, pass results back, let the main session decide.
- **Subagents are resumable.** You can resume a specific subagent to continue its research.
- **Subagents run in the background by default** (the main session keeps working and is notified on completion) and can nest up to 5 levels deep.

## Skill Frontmatter

Skills support these optional fields:

- `disable-model-invocation: true` — Prevents auto-loading; invoke manually with /skillname.
- `user-invocable: false` — Hides the skill from the / menu but keeps it as background knowledge Claude can draw on.
- `model: haiku|sonnet|opus` — Which model runs the skill. Step-by-step skills use haiku. Analysis skills use sonnet. Orchestration/planning skills use opus.
- `context: fork` — Run skill in isolated subagent context (prevents context contamination).
- `agent: <agent-name>` — Bind skill execution to a specific agent's persona and tools.
- `effort: low|medium|high|xhigh|max` — Override reasoning effort level. `xhigh` (Opus 4.7+) usually beats `max` on cost/quality.
- `keep-coding-instructions: true` — Preserve coding-style instructions when the skill switches output styles.
- `${CLAUDE_SKILL_DIR}` — Variable to reference the skill's own directory for relative file access.

## Agent Frontmatter

Beyond basics (name, description, model, permissionMode, tools), agents support:

- `background: true` — Run without blocking the main session (long analysis, monitoring).
- `isolation: worktree` — Run in isolated git worktree (independent copy of repo, auto-cleaned if no changes).
- `context: <text>` — Additional instructions injected into the agent's system prompt.
- `skills: [skill1, skill2]` — Restrict which skills the agent can invoke.
- `maxTurns: N` — Cap agentic iterations (budget control).
- `memory: user|project|local` — Persistent cross-session memory scope.
- `effort: low|medium|high|xhigh|max` — Override reasoning effort level.
- `disallowedTools: [tool1, tool2]` — Remove specific tools from inherited tool lists.
- `initialPrompt: <text>` — First message sent to the agent on startup.
- `Agent(agent_type)` inside a `tools:` allowlist — restrict which specific subagents the agent may spawn.

## Fixed Infrastructure

All projects use this hosting stack (see `.claude/references/infrastructure.md`):
- **Northflank** (frontend + backend containers, Postgres, Redis, cron)
- **Cloudflare** (R2 object storage + orange-cloud proxy/CDN/WAF in front of the Northflank frontend) + **Better Auth** (auth within API) + **Resend** (email)

Plan-repo only recommends language, frameworks, UI library, ORM, and tooling. Infrastructure is locked.

## File Organization

- Keep the `.claude/` folder self-contained. No absolute paths, no references outside the repo except CLAUDE.md, agents.md, README.md, and instructions.md.
- Skills live in `.claude/skills/<skill-name>/SKILL.md`.
- Agents live in `.claude/agents/<agent-name>.md`.
- Hook scripts live in `.claude/scripts/*.sh`. Hooks in settings.json call them by relative path, so the scripts folder must travel with settings.json when copying the template.
- Path-scoped rules live in `.claude/rules/*.md` (conditional on `paths:` frontmatter).
- Agent memory lives in `.claude/agent-memory/` (version-controlled, team-shared evolving knowledge).
- Source URLs for fetching best practices live in `.claude/references/source-urls.md`.
- Template sync state lives in `.claude/references/template-sync-state.json`, and deliberate removals in `template-sync-ignore.md` (both managed by update-practices).
- Infrastructure definition lives in `.claude/references/infrastructure.md` (locked, do not modify per-project).
- CLI tools reference lives in `.claude/references/tools.md`.
- Design guardrails (UI projects) live in `.claude/references/design-guardrails.md`.
- Project settings go in `.claude/settings.json` (version-controlled). Personal overrides go in `.claude/settings.local.json` (git-ignored).

## Hooks and Settings

- Hooks fire on events: PreToolUse, PostToolUse, PostToolUseFailure, PostToolBatch, Stop, StopFailure, Notification, MessageDisplay, SubagentStart, SubagentStop, PreCompact, PostCompact, SessionStart, SessionEnd, UserPromptSubmit, UserPromptExpansion, PermissionRequest, PermissionDenied, TeammateIdle, TaskCompleted, TaskCreated, InstructionsLoaded, ConfigChange, WorktreeCreate, WorktreeRemove, CwdChanged, FileChanged, Elicitation, ElicitationResult, Setup. Full catalog: `.claude/references/hooks-and-settings.md`.
- Hook types: `command` (shell), `http` (POST to URL), `prompt` (single-turn LLM yes/no), `agent` (multi-turn subagent with tools), `mcp_tool` (direct MCP tool invocation).
- Hooks accept an optional `if:` field using permission-rule syntax (e.g., `Bash(git *)`) to fire only on matching tool calls.
- Optional settings: `attribution.*`, `autoUpdatesChannel`, `sandbox.*`, `worktree.*`, `language`, `allowedHttpHookUrls`, `alwaysThinkingEnabled`, `defaultMode` (value `"default"` renamed to `"manual"` in v2.1.200), `fallbackModel`, `enforceAvailableModels`, `disableBundledSkills`, `requiresMinimumVersion`. Full catalog: `.claude/references/hooks-and-settings.md`.
- `settings.local.json` for personal overrides (git-ignored). Supports `disableAllHooks` kill switch.

## Planning

- Planning is **phase-based**, not timeline-based. Phases: Foundation, Core, Polish, Ship.
- Always plan in one session, execute in another. Clear context between planning and implementation.
- Save every plan to a `/tasks` folder. This lets you selectively undo a feature later.
- For big features, use the **spec-developer** skill to generate a thorough plan.
- Every plan MUST end with a **Lessons Learned / Gotchas** section. After implementation, route discoveries to LL-G via `/add-lesson` -- not to local debugging.md files.

## Context Management

- Keep this file under 200 lines for reliable adherence.
- Break tasks small enough to complete in under 50% context usage.
- Use `/compact` proactively around 50% context.
- Start fresh conversations for unrelated topics.
- Begin complex tasks in plan mode before implementation.
- **Preserve the prompt cache:** Lock the MCP/tool list and model at session start. Adding tools or switching models mid-session invalidates the cached prefix and inflates cost.
- **Code bias fix:** If stuck in bad patterns, build the feature in isolation in a fresh folder, then port it in.
- **Document failed attempts:** Write failed fixes to `.claude/agent-memory/debugging.md` before starting new sessions. Avoids repeating dead ends.
- **Handoff docs:** Use `/handoff` to create a summary before ending a session. Load in fresh session as sole context.

## Date Awareness

Best practices must reflect the current date. Always check the current date -- do not assume. When fetching best practices, verify versions and recommendations are current as of today.

## Available Skills

| Skill | Trigger | Purpose |
|-------|---------|---------|
| plan-repo | "plan repo" | Research and recommend best tech stack, generate README, design guardrails |
| init-repo | "initialize repo" | Build or rebuild .claude/ folder with best practices |
| update-practices | "update practices" | Fetch latest best practices and update config |
| spec-developer | "spec developer" | Interview-driven feature spec saved to /tasks |
| security-scan | "security scan" | OWASP-style security audit |
| repo-review | "repo review" | General code health review of the whole repo with fix recommendations |
| performance-review | "performance review" | Performance analysis with fix recommendations |
| dependency-audit | "dependency audit" | Check dependencies for updates and vulnerabilities |
| test-scaffold | "scaffold tests" | Generate test files for untested modules |
| doc-sync | "sync docs" | Align documentation with current code |
| mermaid-diagram | "mermaid diagram" | Generate data flow / architecture diagrams |
| ux-review | "ux review" | Review UI/UX against Laws of UX and Gestalt principles |
| add-lesson | "add lesson" | Add a gotcha or lesson learned to the LL-G knowledge base |
| add-practice | "add practice" | Add a best practice entry to the BP knowledge base |
| apply-practice | "apply practice" | Apply a BP best practice to a target repository |
| merge-worktrees | "merge worktrees" | Merge all worktrees and branches into main, push, then remove worktrees and delete branches |

## Available Agents

See `agents.md` in the repo root for the full agent registry. Key agents:

- **architect** -- phase-based planning, tech stack decisions, file structure design
- **reviewer** -- code review focused on correctness and maintainability
- **security** -- security-focused analysis and vulnerability detection
- **performance** -- performance-focused analysis and optimization
- **explorer** -- codebase exploration, research, and context gathering
- **builder** -- implementation engineer; turns a plan into working, tested code (the implementation-capable agent for parallel team work)
- **tester** -- runs the project's tests and reports actionable pass/fail results
- **ux-reviewer** -- UX-focused review against Laws of UX and Gestalt principles

## Workflow

1. Read existing code before proposing changes.
2. Prefer editing existing files over creating new ones.
3. Do not over-engineer. Only make changes that are directly requested or clearly necessary.
4. Use the source URL registry at `.claude/references/source-urls.md` when fetching best practices -- never hardcode URLs in skills.
5. Check `.claude/references/tools.md` for available CLI tools before running commands. Offer to install missing tools.

## RULE 0: Read-Only First (MANDATORY)

**Gather information before taking action. Read-only commands first. Modifications only with user approval.**

- Always safe: diagnostics and read-only operations (`Get-*`, `Test-*`, queries, list/read API calls).
- Requires user approval: state-changing commands (`Set-*`, `Remove-*`, `Stop-*`, writes to shared systems).
- Never without explicit request: destructive operations (recursive deletions, credential resets, formatting).

Production systems (M365 tenants, shared infrastructure) face hard-to-reverse damage from accidental modifications. Validate intent before any state change. (From BP `safety/read-only-first-rule`.)

## RULE 1 -- Check LL-G Before Scripting (MANDATORY)

**At the start of any session involving scripting, API calls, or automation -- before writing a single line -- fetch the LL-G index and load relevant entries.**

```
Step 1: Fetch https://raw.githubusercontent.com/BoardPandas/LL-G/main/llms.txt
Step 2: For each technology you will use, fetch its sub-index (e.g., kb/ninjaone/llms.txt)
Step 3: Read ALL HIGH-severity entries for those technologies
Step 4: Read any MEDIUM entry whose title matches your specific task
```

Technologies currently in LL-G: PowerShell, Graph API, NinjaOne, Next.js, Tailwind CSS, TypeScript, Godot/GDScript, Better Auth, Bash.

This applies to every session, every technician, every developer. Not optional.

### Contributing back

Every plan file MUST end with a **Lessons Learned / Gotchas** section. After implementation, route any new discoveries to LL-G -- not to local agent-memory or local pattern files only.

- Preferred: run `/add-lesson` from any session (uses GitHub API, no local clone needed)
- Manual: create `kb/<tech>/<slug>.md`, update `kb/<tech>/llms.txt`, update the master `llms.txt` in the `BoardPandas/LL-G` repo

Lessons stored locally stay local. Lessons in LL-G benefit every repo and every technician.

## RULE 3 -- Check BP Before Starting New Work

**When onboarding a repo, starting a new feature, or setting up tooling -- load the BP index and check applicable best practices.**

```
Step 1: Fetch https://raw.githubusercontent.com/BoardPandas/BP/main/llms.txt
Step 2: For each concern relevant to your task, read its llms.txt index
Step 3: Load all FOUNDATIONAL entries (these apply to every repo)
Step 4: Load RECOMMENDED entries whose tech tags match the current project
```

BP is the complement to LL-G: where LL-G tracks what NOT to do, BP tracks what TO do. Use `/add-dir C:\Github\BP` to bring BP into context locally.

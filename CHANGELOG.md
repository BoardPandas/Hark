# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.7.0] - 2026-07-08

### Changed
- **plan-repo skill overhauled.** Research now runs in two explicit waves (language + frontend first, then the four prompts that depend on those picks), every subagent prompt embeds the literal resolved date instead of "today's date", candidate lists are marked as seeds that subagents must refresh against current search results, and a failed subagent no longer stalls the skill. The skill now consults LL-G and BP before researching (RULE 1 + RULE 3) so HIGH-severity gotchas can demote candidates and pre-seed the plan's Lessons Learned section. The SPA-vs-SSR serving mode is recorded as an explicit decision in the recommendation and saved plan. Re-running the skill with an existing `tasks/plan-repo.md` now asks whether to revise or archive instead of overwriting. The optional project-description argument is actually consumed. Frontmatter tightened: `disable-model-invocation: true` and the Agent tool restricted to `Agent(explorer)`.
- **Agent roster hardened.** Read-only review agents (reviewer, performance, security, ux-reviewer, architect) drop `permissionMode: plan` and instead gain `Write` scoped solely to saving reports/plans under `tasks/`, with an explicit "never modify source" instruction; review agents also get `maxTurns` budgets. The security agent moves to `effort: xhigh`, adds package-manager-aware audit commands (`pnpm`/`yarn` audit, `pip-audit`) plus `git log`/`ls-files`/`check-ignore` for history and tracked-secret checks, and its scan categories are rebuilt around the 2025 OWASP Top 10 with value-shaped secret patterns. The builder agent runs in an isolated git worktree (`isolation: worktree`); explorer and tester gain `memory: project` so they read agent-memory before acting. The performance agent adds a hot-path verification step and measurement-to-confirm guidance.
- **Skills refreshed to current Claude Code practices.** security-scan, update-practices, spec-developer, ux-review, init-repo, performance-review, test-scaffold, dependency-audit, and doc-sync were revised for accuracy and current tooling; init-repo gains `AskUserQuestion`. CLAUDE.md adds RULE 0 (Read-Only First), documents hook-script portability and the new template-sync files, and `instructions.md`/`agents.md` are updated to match.
- **Template sync now tracks state.** New `.claude/references/template-sync-ignore.md` lets a project record files it deliberately removed so `update-practices` will not re-create them, alongside a `template-sync-state.json` for last-synced commit and dead-URL strikes.

### Removed
- **`agy-execute-plan` skill removed** (SKILL.md and its evals), superseded by the standard plan/execute workflow.

### Fixed
- **Stripe is no longer assumed.** plan-repo previously wrote Stripe env vars into every README; payments now enter the plan only when the requirements interview says the project takes them (Stripe as the default provider).
- **Gotcha routing aligned with CLAUDE.md.** plan-repo and init-repo told sessions to route post-implementation discoveries to the local `.claude/agent-memory/debugging.md`; both now route to LL-G via `/add-lesson`, and the section name is standardized to "Lessons Learned / Gotchas".

## [0.6.0] - 2026-07-08

### Added
- **New `repo-review` skill.** A general code health review of the whole repository that complements the specialized scans: it checks repo hygiene (tracked junk, oversized files, config drift), correctness and error handling gaps, maintainability (dead code, duplication, naming, premature abstraction), and configuration consistency, then produces a severity-ranked report where every finding includes a specific fix. Instead of duplicating the deep skills, it does a light pass on security, performance, tests, dependencies, docs, and UX, and routes real signal to security-scan, performance-review, test-scaffold, dependency-audit, doc-sync, or ux-review as follow-ups. Bound to the reviewer agent and triggered with "repo review".

## [0.5.2] - 2026-07-05

### Added
- **Hooks & settings catalog expanded to Claude Code 2.1.201 (July 2026).** `hooks-and-settings.md` now documents hook structured output (`updatedToolOutput` on PostToolUse, `additionalContext` on Stop/SubagentStop, `reloadSkills`/`sessionTitle` on SessionStart), `Tool(param:value)` parameter matching (e.g. `Agent(model:opus)`), HTTP hook custom headers with env-var interpolation, a `PermissionRequest` prompt-hook auto-approval pattern, new settings (`defaultMode`, `fallbackModel`, `enforceAvailableModels`, `disableBundledSkills`, `requiresMinimumVersion`, `attribution.sessionUrl`, `autoMode.*`), the full six-tier settings precedence chain, the `ENABLE_PROMPT_CACHING_1H` cache lever, and the v2.1.196 security change that stops committed MCP servers from auto-spawning.
- **New frontmatter capabilities documented.** `user-invocable: false` for hidden background-knowledge skills, `Agent(agent_type)` tool-allowlist entries to restrict which subagents an agent can spawn, and nested `.claude/` directories as a first-class per-subfolder convention (closest wins, `<dir>:<name>` collision naming).
- **Tools reference refreshed for mid-2026.** Biome promoted to the BP-recommended default for new JS/TS projects, `oxlint` and `rolldown` added (Vite 8+ bundles via Rolldown), eslint repositioned for plugin-dependent codebases, and the Prisma entry updated for v7's pure TS/WASM client with native edge support.

### Changed
- **Docs caught up with the agent roster.** `instructions.md` now covers the `builder` and `tester` agents, the `agy-execute-plan` skill, `hooks-and-settings.md`, and per-agent memory folders that already existed in the repo but were missing from the folder map and reference sections.
- **CLAUDE.md notes that subagents now run in the background by default** and can nest up to 5 levels; stale "see init-repo skill" pointers now point at the hooks-and-settings catalog.

### Removed
- **Generic coding-standard bullets pruned from CLAUDE.md** (clear code, descriptive names, small functions) per the "remove what the model handles natively" rule, keeping the file under the 200-line cap.

## [0.5.1] - 2026-06-14

### Added
- **Builder agent memory: skill-propagation pattern** (`.claude/agent-memory/builder/feedback_skill_propagation.md`). Captures the verified, safe sequence for propagating template skills into downstream repos (read every target before writing, stage-then-chmod `kb-upsert.sh`, add the `.gitattributes` LF rule before committing the script, re-read `package.json` right before bumping because a hook may auto-bump it, never sync `infrastructure.md`, and always route exploration to the custom `explorer` agent). Distilled by the builder agents during the cross-repo propagation run.

## [0.5.0] - 2026-06-14

### Added
- **New `agy-execute-plan` skill.** Hands an existing Claude-written plan to the Antigravity CLI (`agy`) for autonomous end-to-end execution, then independently verifies the result against the plan's acceptance criteria using the test suite and the git diff (not AGY's self-reported log), fixes whatever AGY left incomplete or broke, and reports an honest blocked/partial/complete status. Encodes the verified `agy` v1.0.8 operating knowledge a fresh session would otherwise have to rediscover: run headless with an empty stdin and `--dangerously-skip-permissions` or it hangs forever, print-mode stdout is empty when redirected (judge by diff + tests), the Windows PATH-reload step, the `AGY_BLOCKED.md` halt signal, and the set of flags that actually exist in v1.0.8.

## [0.4.0] - 2026-06-14

### Added
- **Bundled `.claude/scripts/kb-upsert.sh`.** A portable create-or-update helper for the GitHub contents API that captures each file's blob SHA immediately before writing and base64-encodes without the GNU-only `base64 -w0` flag. The `add-lesson` and `add-practice` skills now call it instead of hand-running ~8 `gh api` calls each with manual SHA threading, removing a fragile, duplicated sequence and a macOS portability landmine.
- **New `.claude/references/hooks-and-settings.md`.** A single canonical catalog of every hook event, the five hook types, matcher syntax, and all `settings.json` options. `init-repo` and `update-practices` now point at it instead of each carrying their own copy, so the lists can no longer drift apart.

### Changed
- **Knowledge-base skills route exploration to the custom `explorer` agent.** `spec-developer`, `mermaid-diagram`, `plan-repo`, `init-repo`, and `update-practices` previously spun up the built-in `Explore` subagent, which loads every connected MCP tool schema and exceeds the context window (the exact failure CLAUDE.md warns against). They now use the scoped `explorer` agent and say why; `doc-sync`'s explorer references were made explicit too.
- **`add-lesson`, `add-practice`, and `apply-practice` frontmatter normalized.** Added pushy, trigger-phrase-rich descriptions and the `user-invocable`, `argument-hint`, and least-privilege `allowed-tools` fields the other skills already declare.
- **Consistent model routing.** Every standalone skill now pins `model:` (haiku for the mechanical KB writers, sonnet for analysis, opus for orchestration); agent-bound skills continue to inherit their agent's model.
- **`init-repo` slimmed from 491 to 396 lines** by moving the hook/settings reference tables into `hooks-and-settings.md`, bringing it back under the 500-line guideline.

### Fixed
- **Corrected a false claim** in `add-lesson`/`add-practice`/`apply-practice` that the GitHub MCP server "does not exist and will hang the skill." A GitHub MCP server can be connected; the guidance now explains the real reason to stay on `gh`/`WebFetch` (avoid loading MCP schemas mid-skill).
- **Removed the dead `Agent` tool** from `security-scan`, `performance-review`, and `ux-review` allowed-tools — each is bound to a read-only agent that lacks the `Agent` tool, so the entry was impossible and unused.
- **Dropped a phantom `Error` hook event** that `init-repo` referenced; the new reference uses the real `StopFailure` event.

## [0.3.1] - 2026-06-14

### Changed
- **Rewrote the agent-memory README** (`.claude/agent-memory/README.md`) to a more prescriptive version ported from another project. Adds a numbered Rules section covering append-only edits, the 200-line context-injection limit per memory file, and topic-based partitioning when files grow, plus clearer entry-format and activation guidance.

## [0.3.0] - 2026-06-14

### Added
- **New `builder` agent** (`.claude/agents/builder.md`). The template's first implementation-capable agent: a scoped Read/Glob/Grep/Edit/Write/Bash role (`sonnet`, effort `high`, `permissionMode: acceptEdits`, `memory: project`) that turns a plan or spec into working, tested code matching existing conventions. It fills the gap that enabling agent teams exposed: every prior agent was read-only, so the only way to spawn an implementing teammate was the built-in `general-purpose` type that CLAUDE.md bans for blowing the context window. Builders own a file set and coordinate via messaging rather than editing across boundaries, making parallel feature and cross-layer work possible without conflicts.
- **New `tester` agent** (`.claude/agents/tester.md`). A Read/Glob/Grep/Bash role (`sonnet`, effort `medium`) that detects the project's test runner rather than assuming one, runs the relevant suite, and reports pass/fail with actual failure output and a likely-cause classification. It verifies behavior and never edits source, pairing with `builder` to complete the cross-layer team loop (one teammate builds, one verifies).
- Both agents registered in `agents.md` (full entries) and the CLAUDE.md key-agents list.

## [0.2.1] - 2026-06-14

### Added
- **Agent teams enabled project-wide.** `.claude/settings.json` now sets `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS: "1"` in `env`, so every session cloned from this template can coordinate multiple Claude Code instances (shared task list, inter-agent messaging) without per-machine setup. Agent teams are experimental and require Claude Code v2.1.32 or later; the flag is read at session start, so a restart is needed for it to take effect. Subagents (the `Agent` tool) need no flag and remain on by default.

## [0.2.0] - 2026-06-09

### Added
- **Per-skill and per-agent `effort:` frontmatter.** All 15 skills and all 6 agents now pin an effort level matched to their workload: `low` for mechanical step-by-step skills (add-lesson, add-practice, mermaid-diagram), `medium` for analysis and guided-edit skills and the sonnet agents (reviewer, performance, explorer, ux-reviewer), and `high` for orchestration, planning, and high-stakes analysis (init-repo, plan-repo, update-practices, spec-developer, security-scan, architect, security). Lightweight skill invocations no longer inherit session-level effort they do not need.
- **`disable-model-invocation: true` on the `merge-worktrees` skill.** The skill force-deletes branches and worktrees, so it should never auto-trigger; it now runs only when explicitly invoked with `/merge-worktrees`.

### Changed
- **`doc-sync` allowed-tools now declares `Agent`** instead of the pre-rename `Task` tool, completing the cleanup that 0.1.2 applied to the other skills.
- **`agents.md` documents each agent's effort level** alongside its model.

## [0.1.2] - 2026-06-09

### Fixed
- **Commit hook scripts now self-filter on the actual command instead of trusting the `if` rule.** Discovered live after 0.1.1 made the hooks active: the `if: "Bash(git commit*)"` rule fires conservatively on commands containing opaque command substitutions (verified: a `gh api ... -f content="$(base64 ...)"` upload with no git commit in it was blocked by the changelog gate). All four hook scripts now read the hook input JSON and exit 0 unless the command actually contains a git commit invocation, treating `if` as an optimization rather than the guard. Both gotchas from this work were contributed to the LL-G knowledge base under the new `claude-code` technology.

## [0.1.1] - 2026-06-09

### Fixed
- **The git-commit hooks never fired.** All four commit hooks in `.claude/settings.json` used `"matcher": "Bash(git commit*)"`, but hook matchers only match tool names (verified against the official hooks reference and confirmed empirically: a `git commit` with nothing staged ran unblocked). Changed each group to `"matcher": "Bash"` with `"if": "Bash(git commit*)"` on every handler, the documented way to filter on tool arguments. The here-string guard, both changelog gates, and the post-commit knowledge-base prompt are now live.
- **The changelog staged-check no longer falsely blocks compound commands.** `check-changelog-staged.sh` runs before the command executes, so `git add CHANGELOG.md ... && git commit ...` was blocked because the changelog was not staged yet at hook time. The script now reads the hook input and allows any command that stages `CHANGELOG.md` itself.
- **`disable-model-invocation` was spelled with underscores and silently ignored.** The `mermaid-diagram` and `spec-developer` skills used `disable_model_invocation: true`, which Claude Code does not recognize, so both skills remained auto-invocable despite the manual-only intent. Corrected to the hyphenated key (both skills now disappear from the model-invocable list) and fixed the two `instructions.md` passages teaching the underscore spelling.
- **The pre-commit changelog reminder still described the retired 4-segment version scheme.** `pre-commit-changelog-reminder.sh` told Claude to bump `Major.Minor.Patch.Build` with "Build: every commit," contradicting the 3-segment SemVer rule adopted in 0.0.1. The hook text now matches `.claude/rules/commit-changelog.md`.
- **`.gitignore` no longer ignores `Cargo.lock`** (lockfiles should be committed), and the GOPATH-era `bin/` and `pkg/` entries plus the duplicate `build/` under Java/Kotlin were removed so projects' real `bin/`, `pkg/`, and `build/` directories are not silently excluded.

### Changed
- **`instructions.md` caught up with the template's current contents.** Added the `ux-review` and `merge-worktrees` skills, the `ux-reviewer` agent, and `references/ux-laws.md` to the folder structure and reference sections.
- **Skill `allowed-tools` lists now use the current `Agent` tool name.** `performance-review`, `security-scan`, `test-scaffold`, and `ux-review` still listed the pre-rename `Task` tool; all skills now consistently declare `Agent`.

## [0.1.0] - 2026-06-05

### Added
- **New `merge-worktrees` skill** (triggered by "merge worktrees"). Consolidates outstanding work into the repository's main branch and tears down the leftovers: it inventories every worktree and local branch, detects the real main branch (no `main` assumption), shows a plan and asks for confirmation, commits pending work in each worktree, merges every branch into main with `--no-ff`, pushes, then removes the worktrees and force-deletes the merged branches (locally and, with confirmation, on the remote). Merge conflicts and non-fast-forward pulls are hard stops, not auto-resolved, and nothing is deleted until the merge is committed and pushed. Registered in the CLAUDE.md skills index.

## [0.0.3] - 2026-06-05

### Removed
- **Deleted the completed `tasks/peaceful-whistling-dolphin.md` plan file.** The Learning Lessons / Gotchas (LL-G) system it described has been implemented, so the plan is no longer needed.

## [0.0.2] - 2026-06-03

### Fixed
- **`doc-sync` skill no longer assumes the docs folder is capital-D `Docs/`.** On Windows and macOS the filesystem is case-insensitive, so a pre-existing lowercase `docs/` folder satisfied the old hardcoded `Docs/` existence check while the skill kept writing to and citing `Docs/`, creating confusion (and a second, divergent folder on case-sensitive Linux/CI/git). The skill now resolves the docs root once, case-insensitively, preferring the casing git actually tracks (`git ls-files`), reuses that exact name for the whole run, and only defaults to `Docs/` when no docs folder exists.

## [0.0.1] - 2026-06-01

### Changed
- **Switched versioning from 4-segment `Major.Minor.Patch.Build` to 3-segment SemVer `Major.Minor.Patch` and reset the version to `0.0.1`.** Updated the scheme table and rules in `.claude/rules/commit-changelog.md` (the Build segment is removed; the Patch segment now also covers docs, refactors, config, and chores). Prior `1.x.x.x` entries below are retained as historical record.

### Fixed
- **Path-scoped rules were using Cursor's frontmatter dialect and silently not scoping.** `.claude/rules/llg-check.md`, `bp-check.md`, and `commit-changelog.md` used `globs:` and `alwaysApply:`, which are Cursor `.mdc` keys that Claude Code ignores (verified against the official memory docs). Because a rule with no recognized `paths:` field loads unconditionally, the LL-G and BP rules were loading on every file instead of only their intended code/config paths. Converted all three to the official `paths:` YAML-list frontmatter; `commit-changelog.md` now correctly loads unconditionally with no `paths` field.
- **The Stop and Notification bell hooks printed a literal `\a` instead of ringing.** `.claude/settings.json` used `echo '\a'`, which emits the two characters `\` and `a` in most shells. Switched both to `printf '\a'` so the terminal bell actually fires.
- **The shipped baseline `.claude/settings.json` had an empty `permissions.deny` list** despite the init-repo skill documenting a secrets deny-list as "always configure." Added the documented deny entries (`~/.ssh`, `~/.aws`, `~/.azure`, `~/.kube`, `~/.docker/config.json`, `~/.npmrc`, `~/.git-credentials`, `~/.config/gh`, and shell rc files) so every repo cloned from the template starts with secret-file protection.

### Added
- **`SessionStart` hook** wired in `.claude/settings.json` plus a new `.claude/scripts/session-start-kb-check.sh` that surfaces the RULE 1 (LL-G) / RULE 3 (BP) knowledge-base mandate once per session. Previously the KB check only fired on `EnterPlanMode`, so sessions that never entered plan mode got no nudge.
- **`.claude/settings.local.json.example`** showing common git-ignored personal overrides (`disableAllHooks`, `alwaysThinkingEnabled`, `language`), as the init-repo skill recommends.
- **Changelog gate escape hatches.** `check-changelog-staged.sh` and `pre-commit-changelog-reminder.sh` now exempt merge commits (when `MERGE_HEAD` exists) and honor `SKIP_CHANGELOG=1` for genuinely trivial commits, instead of hard-blocking every commit without a staged `CHANGELOG.md`.

### Changed
- **Documented hook event count corrected from 28 to 30.** Added `UserPromptExpansion` (fires when a slash command expands) and `PostToolBatch` (fires after a parallel tool batch resolves), both confirmed in the official hooks reference. Updated `CLAUDE.md`, the init-repo skill hook table, the update-practices skill list (and refreshed its version reference to v2.1.159), and `instructions.md`.
- **`instructions.md` hook descriptions corrected** to reflect the hooks the template actually ships (the git-commit PreToolUse chain, the EnterPlanMode KB check, the PostToolUse KB-contribute prompt, and the new SessionStart reminder) rather than the previous inaccurate "logs a notification" summary.

## [1.8.1.3] - 2026-06-01

### Changed
- Repointed the LL-G and BP knowledge-base references from the `wellforce-brandon` GitHub org to `BoardPandas` after both repos moved. Updated the RULE 1 / RULE 3 fetch URLs in `CLAUDE.md`, the `llg-check` and `bp-check` path-scoped rules, the `add-lesson`, `add-practice`, `apply-practice`, and `init-repo` skills (repo headers, raw URL bases, `gh api` paths, and WebFetch URLs), the `pre-plan-kb-check` and `post-commit-kb-contribute` hook scripts, and `instructions.md`

## [1.8.1.2] - 2026-05-29

### Added
- `MessageDisplay` hook event (introduced in Claude Code v2.1.152) documented in the init-repo skill hook table, the update-practices skill hook list, `CLAUDE.md`, and `instructions.md`. It fires as assistant message text is displayed, letting hooks transform or hide output (for example, redacting secrets). Hook event count moves from 27 to 28

### Changed
- Refreshed the Claude Code version reference in the update-practices skill from v2.1.144 to v2.1.156 (the latest at time of update, verified against the official changelog)

### Removed
- Dropped the phantom `code-review` skill from the documented skill inventory. No `.claude/skills/code-review/SKILL.md` ever existed; the name would shadow the built-in `/code-review` command that the repo already recommends, and the full-codebase-audit niche is covered by `security-scan`, `performance-review`, and `ux-review`. Removed its references from `CLAUDE.md`, `README.md`, `instructions.md` (file tree and skill section), and the update-practices skill checklist. Code review is still available via the `reviewer` agent and the built-in `/code-review` command

## [1.8.1.1] - 2026-05-29

### Changed
- Swapped locked infrastructure defaults in the plan-repo skill and supporting references: frontend hosting moves from Cloudflare Pages to Northflank containers (SPA static-served or SSR, decided per project from the chosen framework), email locks to Resend only (AWS SES dropped), and the CDN is now Cloudflare's orange-cloud proxy in front of the Northflank frontend, with Northflank's built-in Fastly CDN as a no-WAF fallback
- Added a "CDN Setup Notes (Locked)" section to `.claude/references/infrastructure.md` covering Full (Strict) TLS, the ACME-challenge vs Cloudflare-proxy ordering, SSR cache-rule requirements, and zero-cost edge-to-R2 egress
- Updated `.claude/references/tools.md` so `wrangler` is scoped to Cloudflare R2 and DNS/CDN (not Pages) and `northflank` covers frontend deploys as well as backend

## [1.8.1.0] - 2026-05-23

### Fixed
- The update-practices skill now diffs the actual text content of template skills/agents/rules instead of relying on file existence. A skill that was rewritten upstream (e.g. `add-lesson`) is now detected as `TEMPLATE-REWRITTEN` and its body is replaced wholesale with the canonical version (re-applying only genuinely project-specific bits), rather than the old merge-only strategy that silently kept the stale local copy

## [1.8.0.0] - 2026-05-19

### Added
- `reviewer` and `architect` agents now use `memory: project`, reading `.claude/agent-memory/` on startup so they review and plan against accumulated project patterns and decisions
- `worktree.bgIsolation` and `worktree.baseRef` settings documented in the init-repo skill, CLAUDE.md, and update-practices skill (new in Claude Code v2.1.144)
- Prompt-cache preservation guidance (lock the MCP/tool list and model at session start) in CLAUDE.md and instructions.md

### Changed
- Refreshed the hook event reference in the init-repo and update-practices skills to the full 27 events as of Claude Code v2.1.144 (was 18). Added the missing `StopFailure`, `PostCompact`, `PermissionDenied`, `TaskCreated`, `CwdChanged`, `FileChanged`, `Elicitation`, `ElicitationResult`, and `Setup` events
- Added the `mcp_tool` hook type and the conditional `if:` field to the init-repo skill's hook reference, matching the documentation already in CLAUDE.md and instructions.md
- Corrected the init-repo skill's "Recommended agent enhancements" guidance: `background` and `isolation: worktree` are no longer suggested for read-only analysis agents, and `memory: project` guidance now covers `reviewer` and `architect`

## [1.7.1.0] - 2026-05-19

### Fixed
- `add-lesson`, `add-practice`, and `apply-practice` skills no longer hang and time out. They depended on a GitHub MCP server (`mcp__github__*`) that is not configured; invoking those nonexistent tools caused an unresolved tool search to loop until the turn hit its time limit. All three now use the `gh` CLI instead -- `gh api` for reads and writes in add-lesson/add-practice, `WebFetch` on raw URLs for the read-only apply-practice

## [1.7.0.0] - 2026-05-19

### Added
- Pre-commit hook (`check-commit-herestring.sh`) that blocks `git commit` commands using PowerShell here-string syntax (`@'...'@`). In the Bash tool that is not a here-string, so the `@` characters leak into the commit message as a stray `@` line. The hook points to writing the message to a file and using `git commit -F` instead

## [1.6.0.1] - 2026-04-29

### Added
- Documented `mcp_tool` hook type and the conditional `if:` filter syntax for hooks (CLAUDE.md, instructions.md)
- Documented `xhigh` effort tier and `keep-coding-instructions` skill frontmatter field (CLAUDE.md, instructions.md)
- Agent-memory README guidance on explicit memory curation framing and topic partitioning when files grow
- `Cost / token efficiency` audit section in the update-practices skill (effort tuning, model routing, cache preservation, input-format swaps, subagent delegation)
- ProductCompass "stop hitting Claude Code limits" entry to the source URL registry

## [1.6.0.0] - 2026-04-16

### Changed
- Rewrote `doc-sync` skill into a TOC-driven documentation builder that produces a categorized `Docs/` wiki (core, api, features, operations, etc.), modeled on the supportforge platform docs layout but with stable PAGE_ID and AUTOGEN markers for safe incremental updates
- `doc-sync` now operates in three modes: `init` (full generation), `update` (incremental git-diff regeneration), and `audit` (legacy report-only)

### Added
- `Docs/_toc.yaml` schema as the single source of truth for pages, sections, source-file mappings, and diagram requirements
- Reference files: `page-template.md`, `citation-policy.md`, `mermaid-policy.md`, `toc-schema.md`, `doc-categories.md`, `incremental-update.md`, `readme-template.md`
- Page templates: `overview.md`, `architecture.md`, `api-reference.md`, `feature.md`, `database-schema.md`, `module.md`, `data-flow.md`, `runbook.md`, `getting-started.md`, `configuration.md`, `glossary.md`, `_toc.yaml.template`
- Evidence-based citation rules with line numbers and parenthesized inline format
- Mermaid diagram policy (graph TD only, quoted node labels, no shorthand activation) and a 3-attempt repair budget
- AUTOGEN marker contract for safe regeneration that preserves manual notes
- `Docs/_meta/GENERATION.md` and `Docs/_meta/SUMMARY.md` outputs for generation metadata and coverage reporting

## [1.5.0.0] - 2026-04-14

### Added
- UX Review skill (`/ux-review`) for reviewing UI code against Laws of UX and Gestalt principles
- UX Reviewer agent (`ux-reviewer`) with severity-ranked finding output format
- UX Laws reference doc (`.claude/references/ux-laws.md`) covering all 30 laws from lawsofux.com with code-level indicators

## [1.4.0.0] - 2026-03-25

### Added
- Bootstrap template sync step in update-practices skill (Step 2b) to pull new/updated files from upstream template repo
- Bootstrap Template source URLs for GitHub API tree and raw content access
- Template sync report section in update-practices output summary

## [1.3.0.0] - 2026-03-24

### Added
- Add Practice skill wired to `wellforce-brandon/BP` via GitHub API
- Apply Practice skill wired to `wellforce-brandon/BP` via GitHub API
- Pre-plan hook to check LL-G and BP knowledge bases before creating plans
- Post-commit hook to evaluate if work should be contributed back to LL-G or BP
- Pre-commit changelog reminder hook with condensed update instructions

### Changed
- Commit-changelog rule set to `alwaysApply: true` so version bump instructions are always in context
- Pre-commit changelog enforcement hook status message clarified

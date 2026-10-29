# Agent Registry

All custom agents for this project. Each agent is defined in `.claude/agents/` as a markdown file with YAML frontmatter.

## Registered Agents

### architect

- **File:** `.claude/agents/architect.md`
- **Purpose:** Phase-based planning, tech stack decisions, file structure design, and architectural review. Uses phase-based planning (Foundation → Core → Polish → Ship), never timeline-based.
- **When to use:** Starting a new feature, restructuring a codebase, evaluating technology choices, or designing system architecture.
- **Model:** opus
- **Effort:** high

### reviewer

- **File:** `.claude/agents/reviewer.md`
- **Purpose:** Code review focused on correctness, maintainability, naming, DRY violations, and adherence to project standards.
- **When to use:** Before merging PRs, after completing a feature, or when requesting a second opinion on code quality.
- **Model:** sonnet
- **Effort:** medium

### security

- **File:** `.claude/agents/security.md`
- **Purpose:** Security-focused analysis covering OWASP Top 10, secrets detection, dependency vulnerabilities, and input validation gaps.
- **When to use:** Before releases, after adding authentication or authorization logic, or when handling user input or external data.
- **Model:** opus
- **Effort:** xhigh

### performance

- **File:** `.claude/agents/performance.md`
- **Purpose:** Performance-focused analysis covering query optimization, memory leaks, bundle size, caching, and algorithmic efficiency.
- **When to use:** When response times degrade, before scaling, or when optimizing resource-intensive operations.
- **Model:** sonnet
- **Effort:** medium

### builder

- **File:** `.claude/agents/builder.md`
- **Purpose:** Implementation engineer. Turns a plan, spec, or task into working, tested code that matches existing conventions. The implementation-capable agent for parallel team work, with a scoped tool list (no banned built-in `general-purpose` type).
- **When to use:** Executing a plan produced by `architect`, fixing a bug, or owning one file set / layer of a feature in an agent team. Spawn one builder per independent file set; each builder runs in an isolated git worktree, so parallel builders cannot conflict (merge back with the merge-worktrees skill).
- **Model:** sonnet
- **Effort:** high

### tester

- **File:** `.claude/agents/tester.md`
- **Purpose:** Test runner and failure analyst. Detects the project's test runner, executes the relevant suite, and reports pass/fail with actionable failure detail. Verifies behavior; does not implement fixes.
- **When to use:** Verifying a change, pairing with `builder` in a cross-layer team (one builds, one verifies), or checking a suite before merge.
- **Model:** sonnet
- **Effort:** medium

### explorer

- **File:** `.claude/agents/explorer.md`
- **Purpose:** Codebase exploration, online research, doc fetching, and context gathering. Always include a "why" when spawning -- not just what to find, but why you need it.
- **When to use:** Before implementing features (understand existing patterns), when researching approaches, when gathering context for planning. Spin up multiple explorers in parallel for competing approaches.
- **Model:** sonnet
- **Effort:** medium

### ux-reviewer

- **File:** `.claude/agents/ux-reviewer.md`
- **Purpose:** UX-focused review evaluating UI code against Laws of UX and Gestalt principles. Produces severity-ranked findings with specific improvement recommendations.
- **When to use:** Before shipping frontend features, after UI redesigns, when evaluating component usability, or when a UX audit is requested.
- **Model:** sonnet
- **Effort:** medium

---
description: Enforce BP best practices check before starting new work
paths:
  - "CLAUDE.md"
  - ".claude/**"
  - "**/Cargo.toml"
  - "rustfmt.toml"
  - "clippy.toml"
  - "deny.toml"
  - "dist-workspace.toml"
  - "wix/**"
  - ".github/**"
---

# RULE 3 Enforcement: Check BP Before Configuration Work

Before creating or modifying infrastructure, tooling, or configuration files matching the paths above, you MUST consult the BP knowledge base to follow proven patterns.

## Required Steps

1. **Fetch the master index:**
   ```
   WebFetch https://raw.githubusercontent.com/BoardPandas/BP/main/llms.txt
   ```

2. **Identify relevant concerns** from the file you're about to write (e.g., testing config -> testing, Dockerfile -> deployment, CLAUDE.md -> claude-config).

3. **Fetch each relevant concern index:**
   ```
   WebFetch https://raw.githubusercontent.com/BoardPandas/BP/main/practices/<concern>/llms.txt
   ```

4. **Read ALL FOUNDATIONAL entries** for matched concerns.

5. **Read RECOMMENDED entries** whose tech tags match this project's stack.

## When to check

- Setting up new tooling (linters, formatters, test runners)
- Creating or modifying Dockerfiles
- Configuring CI/CD pipelines
- Structuring `.claude/` configuration
- Setting up monorepo workspaces
- Adding versioning or changelog automation
- Configuring environment/secrets management

## Do NOT skip this check

- If you already checked BP earlier in this conversation for the same concern, you do not need to re-fetch.
- If no entries are relevant, proceed -- but you must have looked first.

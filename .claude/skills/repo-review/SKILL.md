---
name: repo-review
effort: medium
description: Run a general code health review across the whole repository, covering correctness risks, error handling gaps, dead code, duplication, oversized files, and repo hygiene, with fix recommendations and pointers to the specialized scan skills for deep dives. Use for periodic checkups or when onboarding an unfamiliar repo.
user-invocable: true
argument-hint: [optional: file or directory path to scope the review]
agent: reviewer
allowed-tools:
  - Read
  - Glob
  - Grep
  - Bash
---

# Repo Review

You have been asked to perform a general health review of the repository. This is the broad sweep that complements the specialized skills: it finds issues across the whole codebase and repo structure, recommends fixes, and routes anything deep to the right specialized skill rather than duplicating it.

## Step 1: Determine Scope and Stack

1. If the user specified a file or directory, scope the review to that path.
2. If no scope was specified, review the entire repository.
3. Identify the tech stack from dependency manifests and file types.
4. Read CLAUDE.md and any `.claude/rules/*.md` so findings are judged against this project's own standards, not generic ones.

## Step 2: Repo Hygiene

1. Check for files that should be gitignored but are tracked: build output, logs, editor folders, OS junk (`.DS_Store`, `Thumbs.db`), local env files.
2. Check for committed artifacts that do not belong in the repo: large binaries, database dumps, node_modules or vendor folders.
3. Verify expected top-level files exist and are current: README, .gitignore, license (if public), lockfile matching the manifest.
4. Run `git log --oneline -15` and `git status` to spot uncommitted drift or a stale working tree.
5. Flag files over 500 lines (project standard) as split candidates. Use a line count command per candidate found via Glob.
6. Look for duplicate or near-duplicate config files (two lint configs, competing formatter configs, both .env.example and .env.sample).

## Step 3: Correctness and Error Handling

1. Search for swallowed errors: empty catch blocks, `catch (e) {}`, `except: pass`, errors logged and then ignored on paths that should fail.
2. Find inputs at system boundaries (API routes, form handlers, file I/O, CLI args) that are used without validation.
3. Look for floating promises and missing awaits in async code.
4. Check for null/undefined access on values that can legitimately be absent (optional API fields, missing env vars, empty arrays).
5. Inventory TODO, FIXME, and HACK comments. Old ones are findings; recent ones are context.
6. Look for logic that silently returns a default on failure where the caller cannot distinguish success from failure.

## Step 4: Maintainability

1. Find dead code: unused exports, unreachable branches, commented-out blocks, files nothing imports.
2. Look for real duplication: the same non-trivial logic implemented in more than one place. Do not flag three similar lines; the project prefers those over forced helpers.
3. Check naming consistency: mixed conventions for the same concept (fetchUser vs getUser vs loadUser), misleading names, single-letter names outside tight loops.
4. Flag deeply nested logic (4+ levels) and functions doing several unrelated jobs.
5. Look for magic numbers and hardcoded strings that should be named constants or config.
6. Check for premature abstraction: interfaces with one implementation, wrappers that only forward calls, config options nothing sets.

## Step 5: Configuration and Consistency

1. Verify lint and format tooling is configured and consistent with what the code actually follows.
2. For TypeScript, check tsconfig strictness (`strict`, `noUncheckedIndexedAccess`) and search for `any` escapes and unsafe casts.
3. Check env var usage: variables read but not documented in .env.example, or documented but never read.
4. Look for version drift: engine/runtime versions pinned inconsistently across manifest, CI config, and Dockerfile.

## Step 6: Route Deep Dives to Specialized Skills

Do a light pass only in each of these areas. If you find real signal, record one summary finding and recommend the specialized skill instead of going deep here:

| Signal found | Recommend |
|---|---|
| Possible secrets, injection, auth gaps | security-scan |
| N+1 queries, hot loops, memory growth | performance-review |
| Modules with no test coverage | test-scaffold |
| Outdated or vulnerable dependencies | dependency-audit |
| Docs contradicting current code | doc-sync |
| UI/UX problems in frontend code | ux-review |

## Step 7: Produce Report

Use the reviewer severity levels. Format the report as follows:

```
# Repo Review Report

## Summary
- Files reviewed: <count>
- Critical: <count>
- Warnings: <count>
- Suggestions: <count>

## Critical (must fix)
[CRITICAL] <category> - file:line
  Finding: <description>
  Fix: <specific change to make>

## Warnings (should fix)
(same format)

## Suggestions (nice to have)
(same format)

## Recommended Follow-ups
- <specialized skill>: <one-line reason based on what was found>

## Healthy Areas
<brief note on what is in good shape, so the report is calibrated, not just negative>
```

Prioritize findings by real-world impact. Every finding must include a specific fix, not vague advice. Do not nitpick style that matches the project's existing conventions.

---
name: apply-practice
model: sonnet
effort: medium
description: Apply a specific best practice from the BP knowledge base to a target repository. Use this whenever the user says "apply a practice", "apply <slug> to this repo", "adopt the BP pattern for X", "bring this repo up to the BP standard", or wants a proven convention/config/workflow from BP installed into the current project -- even if they don't say "BP" explicitly.
user-invocable: true
argument-hint: (optional) the practice slug (e.g. testing/vitest-monorepo-config) and/or target repo path
allowed-tools:
  - Read
  - Glob
  - Grep
  - Bash
  - Edit
  - Write
  - WebFetch
---

You are applying a specific best practice from the BP knowledge base to a target repository.

**Repository:** `BoardPandas/BP` on GitHub
**Raw URL base:** `https://raw.githubusercontent.com/BoardPandas/BP/main/`

## Step 1: Identify inputs

You need two things:
1. **Practice slug** -- e.g., `testing/vitest-monorepo-config` or `safety/read-only-first-rule`
2. **Target repo** -- the local repository to apply the practice to (current working directory by default, or ask the user)

Ask the user for any missing inputs.

## Step 2: Load the practice from GitHub

This skill only reads from BP -- use `WebFetch` on the raw URL. Don't switch to the GitHub MCP server even if one is connected: a raw `WebFetch` is a plain read, and pulling in MCP tool schemas mid-skill bloats the context window for no benefit.

Fetch the practice entry:
```
WebFetch https://raw.githubusercontent.com/BoardPandas/BP/main/practices/<concern>/<slug>.md
```

If the user doesn't know the exact slug, fetch the concern index first to show available practices:
```
WebFetch https://raw.githubusercontent.com/BoardPandas/BP/main/practices/<concern>/llms.txt
```

Or fetch the master index to show all concerns:
```
WebFetch https://raw.githubusercontent.com/BoardPandas/BP/main/llms.txt
```

## Step 3: Run CHECK steps

Execute each CHECK item against the target repo to verify the practice isn't already applied.

- If ALL checks pass: inform the user "This practice is already applied to <repo-name>" and stop.
- If SOME checks pass: inform the user which parts are already in place and which are missing. Ask if they want to proceed with the missing parts only.
- If NO checks pass: proceed to implementation.

## Step 4: Follow IMPLEMENT steps

Execute each step in the IMPLEMENT section against the target repo. For each step:
1. Show what you're about to do
2. Execute the change
3. Verify the change was applied correctly

## Step 5: Validate

Re-run the CHECK steps to confirm all checks now pass.

## Step 6: Report

Output:
- Which steps were applied
- Which checks now pass
- Any manual follow-up needed (e.g., "run tests to verify", "restart dev server")

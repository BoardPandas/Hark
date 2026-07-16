---
name: add-practice
model: haiku
effort: low
description: Add a new best-practice entry to the shared BP knowledge base. Use this whenever the user says "add a practice", "record this pattern", "save this to BP", "this should be a best practice", or describes a proven, reusable convention, config, or workflow that other repos should adopt -- even if they don't say "BP" explicitly.
user-invocable: true
argument-hint: (optional) the practice title or a short description of the pattern
allowed-tools:
  - Bash
  - Write
---

You are adding a new entry to the BP best practices knowledge base.

**Repository:** `BoardPandas/BP` on GitHub
**Raw URL base:** `https://raw.githubusercontent.com/BoardPandas/BP/main/`

All GitHub operations use the `gh` CLI via the Bash tool. Do not switch to the GitHub MCP server for these operations even if one is connected -- `gh` is already authenticated and consistent, and pulling in MCP tool schemas mid-skill bloats the context window for no benefit.

The fragile parts of writing to the contents API (capturing each file's blob SHA, base64-encoding without the GNU-only `-w0` flag, and choosing create-vs-update) are handled by `.claude/scripts/kb-upsert.sh`. You compute the new file contents; the script pushes them.

## Step 1: Collect information

Ask the user for the following (you may ask all at once):
1. **Concern** -- which category does this belong in? (claude-config, testing, linting-formatting, error-handling, deployment, monorepo, versioning, safety, documentation, design-systems, environment, knowledge-bases, or a new concern name)
2. **Title** -- short descriptive title (becomes the H1 and the link text in llms.txt)
3. **Pattern** -- what the proven pattern looks like and how it works
4. **Why** -- why this is better than alternatives, what problems it prevents
5. **Example** -- code or config from the source repo (with file paths)
6. **Priority** -- foundational, recommended, or optional (see legend below)
7. **Tech tags** -- comma-separated list of technologies this applies to
8. **Source repo** -- which repo this pattern was extracted from
9. **Applies-to** -- what tech stacks should adopt this (may differ from tech tags)
10. **Check** -- how to verify if a repo already follows this (checklist items)
11. **Implement** -- steps to adopt this in a repo that doesn't have it
12. **Notes** (optional) -- edge cases, caveats, related practices

Priority legend:
- foundational = universal pattern every repo should follow
- recommended = strong pattern for repos with matching tech tags
- optional = nice-to-have improvement

## Step 2: Generate the slug

Convert the title to a slug: lowercase, spaces and punctuation replaced with hyphens, no leading/trailing hyphens.
Example: "Hierarchical CLAUDE.md Structure" -> `hierarchical-claude-md.md`

## Step 3: Fetch current state from GitHub

Confirm `gh` is available and authenticated (run once):
```
gh auth status
```
If `gh` is not installed or not authenticated, stop and tell the user to run `gh auth login` first.

Read the current master index and the relevant concern index so you know the entry count and can avoid duplicates:
```
gh api repos/BoardPandas/BP/contents/llms.txt --jq .content | base64 -d
gh api repos/BoardPandas/BP/contents/practices/<concern>/llms.txt --jq .content | base64 -d
```
If the concern command fails with a `404`, the concern folder does not exist yet -- you will create it in Step 5. You do NOT need to capture blob SHAs by hand; `kb-upsert.sh` reads the current SHA itself immediately before each write.

## Step 4: Create the entry file

1. Use the Write tool to save the entry markdown to a scratch file git will not track, e.g. `.git/bp-entry.md` (git never tracks files inside `.git/`).

   Content format:
   ```
   ---
   concern: <concern>
   tech: [tech1, tech2]
   priority: <foundational|recommended|optional>
   source-repo: <repo-name>
   applies-to: [tech1, tech2]
   ---
   # <Title>

   ## PATTERN
   <pattern description>

   ## WHY
   <why this is better>

   ## EXAMPLE
   <code or config examples with file paths>

   ## CHECK
   How to verify if a repo already follows this:
   - [ ] Check condition 1
   - [ ] Check condition 2

   ## IMPLEMENT
   Steps to adopt this in a repo that doesn't have it:
   1. Step one
   2. Step two

   ## NOTES
   <notes, or omit the section if none>
   ```

2. Push it (the script base64-encodes and creates the file):
```
.claude/scripts/kb-upsert.sh BoardPandas/BP practices/<concern>/<slug>.md .git/bp-entry.md "Add <concern> practice: <title>"
```

3. Delete the scratch file: `rm .git/bp-entry.md`

## Step 5: Update the concern llms.txt

Compute the new content of `practices/<concern>/llms.txt`:
- If the concern folder already exists: take the content from Step 3 and append a new bullet under `## Entries`:
  ```
  - [<Title>](<slug>.md): <one-line description>. <PRIORITY>.
  ```
- If the concern folder does not exist: create the content fresh:
  ```
  # <Concern> Best Practices

  > Proven <concern> patterns.

  ## Entries

  - [<Title>](<slug>.md): <one-line description>. <PRIORITY>.
  ```

Write the full new file content to `.git/bp-index.md` with the Write tool, then push it:
```
.claude/scripts/kb-upsert.sh BoardPandas/BP practices/<concern>/llms.txt .git/bp-index.md "Update <concern> index: add <slug>"
```
The script creates the file if it didn't exist (new concern) or updates it in place otherwise -- you don't pass a SHA. Then delete the scratch file: `rm .git/bp-index.md`

## Step 6: Update master llms.txt entry count

Take the master `llms.txt` content from Step 3. Find the bullet for this concern and increment the entry count in parentheses: `(N entries)` -> `(N+1 entries)`.

If this is a new concern, add a new section under `## Concerns`:
```
### <Concern>
- [<Concern> index](practices/<concern>/llms.txt): <description> (1 entry)
```

Write the updated master content to `.git/bp-master.md`, then push it:
```
.claude/scripts/kb-upsert.sh BoardPandas/BP llms.txt .git/bp-master.md "Update master index: <concern> now has N+1 entries"
```
Then delete the scratch file: `rm .git/bp-master.md`

## Step 7: Confirm

Output:
- The GitHub URL of the created entry file (printed by `kb-upsert.sh`)
- Confirmation that both index files were updated
- The entry's priority level

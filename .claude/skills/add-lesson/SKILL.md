---
name: add-lesson
model: haiku
effort: low
description: Add a new gotcha, lesson learned, or "thing to avoid" to the shared LL-G knowledge base. Use this whenever the user says "add a lesson", "record this gotcha", "save this to LL-G", "document this dead end", or describes a non-obvious failure, a silent-wrong-output bug, or a hard-won fix that future sessions should not have to rediscover -- even if they don't say "LL-G" explicitly.
user-invocable: true
argument-hint: (optional) the lesson title or a short description of the gotcha
allowed-tools:
  - Bash
  - Write
---

You are adding a new entry to the LL-G lessons-learned knowledge base.

**Repository:** `BoardPandas/LL-G` on GitHub
**Raw URL base:** `https://raw.githubusercontent.com/BoardPandas/LL-G/main/`

All GitHub operations use the `gh` CLI via the Bash tool. Do not switch to the GitHub MCP server for these operations even if one is connected -- `gh` is already authenticated and consistent, and pulling in MCP tool schemas mid-skill bloats the context window for no benefit.

The fragile parts of writing to the contents API (capturing each file's blob SHA, base64-encoding without the GNU-only `-w0` flag, and choosing create-vs-update) are handled by `.claude/scripts/kb-upsert.sh`. You compute the new file contents; the script pushes them.

## Step 1: Collect information

Ask the user for the following (you may ask all at once):
1. **Technology** -- which folder does this belong in? (powershell, nextjs, tailwind, typescript, better-auth, godot, graph-api, bash, ninjaone, cloudflare, teams-sharepoint, cmd, or a new tech name)
2. **Title** -- short descriptive title (becomes the H1 and the link text in llms.txt)
3. **Problem** -- what goes wrong and why it's not obvious
4. **Wrong pattern** -- code showing the incorrect approach
5. **Right pattern** -- code showing the correct approach
6. **Severity** -- high, medium, or low (see legend below)
7. **Tags** -- comma-separated list of relevant keywords
8. **Notes** (optional) -- edge cases, related entries, cross-references

Severity legend:
- high = silent wrong output or hard-to-debug errors
- medium = obvious failures (build errors, test failures)
- low = style/convention, caught by linter

## Step 2: Generate the slug

Convert the title to a slug: lowercase, spaces and punctuation replaced with hyphens, no leading/trailing hyphens.
Example: "Variable quoting in strings" -> `quoting.md`

## Step 3: Fetch current state from GitHub

Confirm `gh` is available and authenticated (run once):
```
gh auth status
```
If `gh` is not installed or not authenticated, stop and tell the user to run `gh auth login` first.

Read the current master index and the relevant tech index so you know the entry count and can avoid duplicates:
```
gh api repos/BoardPandas/LL-G/contents/llms.txt --jq .content | base64 -d
gh api repos/BoardPandas/LL-G/contents/kb/<tech>/llms.txt --jq .content | base64 -d
```
If the tech command fails with a `404`, the tech folder does not exist yet -- you will create it in Step 5. You do NOT need to capture blob SHAs by hand; `kb-upsert.sh` reads the current SHA itself immediately before each write.

## Step 4: Create the entry file

1. Use the Write tool to save the entry markdown to a scratch file git will not track, e.g. `.git/llg-entry.md` (git never tracks files inside `.git/`).

   Content format:
   ```
   ---
   tech: <technology>
   tags: [tag1, tag2, tag3]
   severity: <high|medium|low>
   ---
   # <Title>

   ## PROBLEM
   <problem description>

   ## WRONG
   ```<language>
   <wrong code example>
   ```

   ## RIGHT
   ```<language>
   <right code example>
   ```

   ## NOTES
   <notes, or omit the section if none>
   ```

2. Push it (the script base64-encodes and creates the file):
```
.claude/scripts/kb-upsert.sh BoardPandas/LL-G kb/<tech>/<slug>.md .git/llg-entry.md "Add <tech> gotcha: <title>"
```

3. Delete the scratch file: `rm .git/llg-entry.md`

## Step 5: Update the tech llms.txt

Compute the new content of `kb/<tech>/llms.txt`:
- If the tech folder already exists: take the content from Step 3 and append a new bullet under `## Entries`:
  ```
  - [<Title>](<slug>.md): <one-line description>. <SEVERITY>.
  ```
- If the tech folder does not exist: create the content fresh:
  ```
  # <Tech> Gotchas

  > Known <tech> patterns that cause silent failures or hard-to-debug errors.

  ## Entries

  - [<Title>](<slug>.md): <one-line description>. <SEVERITY>.
  ```

Write the full new file content to `.git/llg-index.md` with the Write tool, then push it:
```
.claude/scripts/kb-upsert.sh BoardPandas/LL-G kb/<tech>/llms.txt .git/llg-index.md "Update <tech> index: add <slug>"
```
The script creates the file if it didn't exist (new tech) or updates it in place otherwise -- you don't pass a SHA. Then delete the scratch file: `rm .git/llg-index.md`

## Step 6: Update master llms.txt entry count

Take the master `llms.txt` content from Step 3. Find the bullet for this technology and increment the entry count: `(N entries)` -> `(N+1 entries)`.

If this is a new technology, add a new section under `## Technologies`:
```
### <Tech>
- [<Tech> index](kb/<tech>/llms.txt): All <tech> gotchas (1 entry)
```

Write the updated master content to `.git/llg-master.md`, then push it:
```
.claude/scripts/kb-upsert.sh BoardPandas/LL-G llms.txt .git/llg-master.md "Update master index: <tech> now has N+1 entries"
```
Then delete the scratch file: `rm .git/llg-master.md`

## Step 7: Close the loop -- does this lesson ALSO need enforcement?

**The LL-G entry from Steps 1-6 is not optional and is never replaced by anything
here.** Writing it is what this skill is for. Step 7 decides only what to add *on
top of it*, and the answer is often nothing.

An LL-G entry *teaches*. It does not *enforce*: nothing fails when the lesson is
forgotten, which is how the same gotcha returns a year later in a different repo.

**If the lesson concerns this repository's own Claude configuration** -- a hook that
did not fire, a rule that never loaded, frontmatter silently ignored, a skill on the
wrong model, a guard that could be satisfied vacuously -- then it also needs a
regression case, or nothing stops it recurring:

1. Add a case under `.claude/evals/cases/NN-<slug>.md` following the format in
   `.claude/evals/README.md`. Grade **behaviour**, not recall: if the expectation is
   unmet but every other one holds, something must actually have gone wrong.
2. If the defect is *statically* checkable, prefer a check in
   `scripts/check-claude-wiring.mjs` plus a case in its test file. A check that runs
   on every push beats one that runs when someone remembers to invoke the eval suite;
   the eval case is for behaviour a static check cannot reach.
3. Run `npm run evals -- --validate` to confirm the corpus still parses.

**If the lesson is about a technology rather than this configuration** (a Graph API
quirk, a Next.js footgun), the LL-G entry already written is the whole fix. Say so and
stop -- do not manufacture an eval case for something the eval harness cannot exercise.

Either way, the LL-G entry ships. A guard check and an eval case are enforcement added
beside it, never a substitute for it: a check that lives only in this repo teaches no
other repo anything, which is the entire reason LL-G exists.

State which of the two this was, and why. "No eval case: this is a PowerShell
gotcha, not a configuration defect" is a complete and correct answer.

## Step 8: Confirm

Output:
- The GitHub URL of the created entry file (printed by `kb-upsert.sh`)
- Confirmation that both index files were updated
- The entry's severity level
- Whether an eval case or guard check was added, or why neither was needed

---
name: add-lesson
model: haiku
effort: low
description: Add a new gotcha, lesson learned, or "thing to avoid" to the shared LL-G knowledge base. Use this whenever the user says "add a lesson", "record this gotcha", "save this to LL-G", "document this dead end", or describes a non-obvious failure, a silent-wrong-output bug, or a hard-won fix that future sessions should not have to rediscover -- even if they don't say "LL-G" explicitly.
user-invocable: true
argument-hint: (optional) the lesson title or a short description of the gotcha
allowed-tools:
  - Bash
  - Read
  - Write
---

You are adding a new entry to the LL-G lessons-learned knowledge base.

**Repository:** `BoardPandas/LL-G` on GitHub
**Raw URL base:** `https://raw.githubusercontent.com/BoardPandas/LL-G/main/`

Use authenticated `gh` and Git for GitHub operations. This is the approved transport for
these knowledge contributions, including where other external services use an MCP Hub.
No coordination service, session registration, proposal queue, or MCP connection is needed.

All three files this skill touches -- the entry, its shelf `llms.txt`, and the master index count -- are written as **ONE commit** by `.claude/scripts/kb-commit.mjs`. You compute the file contents; the script builds the commit.

Do not push them one at a time. The three files are only correct together: after a lone entry push the entry is an orphan no index lists, and after a lone shelf push the master count disagrees with the shelf. Both states now fail LL-G's CI, and on 2026-09-10 that produced three red builds before the fourth went green. `kb-upsert.sh` (single-file upsert) also re-reads each blob SHA immediately before its PUT, which inverts the compare-and-swap and silently overwrites anything that landed while you were editing -- see LL-G `kb/git/github-contents-sha-refresh-defeats-cas.md`. `kb-commit.mjs` takes the base commit you edited from and refuses a non-fast-forward instead.

## Step 1: Collect information

Use facts already established in the conversation; ask only for missing information:
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

**Capture the base commit SHA first, before you read or edit anything.** This is what makes the
write safe: your commit is parented on it, and if anyone pushes while you are composing the
entry, the ref update is refused rather than silently overwriting them.
```
BASE=$(gh api repos/BoardPandas/LL-G/commits/heads/main --jq .sha)
echo "$BASE"
```
Keep that value; you pass it to `--base` in Step 6. Do not re-read it later -- a freshly-read
SHA always matches HEAD, which is the bug this avoids.

Read the current master index and the relevant tech index so you know the entry count and can avoid duplicates:
```
gh api "repos/BoardPandas/LL-G/contents/llms.txt?ref=$BASE" -H 'Accept: application/vnd.github.raw'
gh api "repos/BoardPandas/LL-G/contents/kb/<tech>/llms.txt?ref=$BASE" -H 'Accept: application/vnd.github.raw'
```
If the tech command fails with a `404`, the tech folder does not exist yet -- you will create it in Step 5.

> On a 404, `gh api --jq .sha` prints the error body to **stdout** rather than applying the
> filter, so an unchecked capture yields a JSON blob instead of an empty string. Validate any
> SHA you capture against `^[0-9a-f]{40}$` rather than testing it for emptiness.

## Privacy and validation before publishing

Both knowledge repositories are public. Remove credentials, real email/IP addresses, client
and tenant names, internal hostnames, private URLs, ticket identifiers and copied client data.
Use documentation values such as `example.com`, `192.0.2.10` and `<tenant-id>`. Private source
evidence must not enter the files, frontmatter, or commit message. Omit private source-repo
metadata. Automated CI does not replace this review and cannot recognize every client name.

Read likely duplicate entries at the same captured `$BASE`. An existing lesson/practice
should be linked instead of duplicated. Validate the slug is lowercase hyphenated text with
no path separators. A new shelf/concern needs its own index and master-index line; verify no
existing category fits. Only an actual 404 means missing, not an authentication/network error.

Read the target repository's `CLAUDE.md` and commit/changelog policy at the same captured
`$BASE`. Include any required version and changelog updates in the same atomic commit by
adding their repo-path/local-file pairs to `kb-commit.mjs`. Preserve the target's release
conventions; a knowledge contribution does not authorize unrelated policy changes.

Before the write, review the three-file diff for private content, required frontmatter and
sections, relative links and index counts. Run the target repository's `npm run check` in a
scratch checkout based on `$BASE` with the candidate files applied when local validation is
available. Use only real, evidenced knowledge; do not publish dummy entries to test this skill.

Prepare the contribution before asking for any write approval required by the session.
Existing user authorization to publish covers the write; do not ask again. Otherwise obtain
approval for the concrete public contribution before invoking the publisher. Publishing is
immediate, with no hold period or withdrawal window.

## Step 4: Create the entry file

1. Create the scratch directory, then use the Write tool to save the entry markdown to `.agent-scratch/llg-entry.md`:

   ```
   mkdir -p .agent-scratch && printf '*\n' > .agent-scratch/.gitignore
   ```

   The inner `.gitignore` of `*` makes the directory ignore itself, so no scratch file can be
   committed in any repo, whatever that repo's own `.gitignore` says. Do not use `.git/` for
   scratch: the template's `Edit(**/.git/**)` deny rule blocks the Write tool there, and the
   files then cannot be cleaned up either. Do not use `.claude/` either: Claude Code treats it
   as a protected path, so the Write tool is refused there in `acceptEdits` mode and in
   non-interactive runs. A path outside the working directory is refused the same way.

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

2. Leave it on disk. Nothing is pushed until Step 6, which commits all three files together.

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

Write the full new file content to `.agent-scratch/llg-index.md` with the Write tool. Do not push it yet.

Append the bullet in the file's **own** format: most shelves use `- [Title](slug.md): ... SEV.`
but a few prefix the severity (`- HIGH [Title](slug.md): ...`). Keep the existing shape, and
keep the whole entry on one line -- LL-G's guard counts bullets by line.

## Step 6: Update master llms.txt entry count

Take the master `llms.txt` content from Step 3. Find the bullet for this technology and increment the entry count: `(N entries)` -> `(N+1 entries)`.

If this is a new technology, add a new section under `## Technologies`:
```
### <Tech>
- [<Tech> index](kb/<tech>/llms.txt): All <tech> gotchas (1 entry)
```

Edit the master line **line-anchored** -- change only the number, never reflow the
description. Master-index lines are one clause naming the technology and its scope; the
specific gotchas belong on the shelf. The file has a hard 20 KB budget that fails CI, because
every session loads it under RULE 1 (it once reached 51 KB of shelf summaries).

Write the updated master content to `.agent-scratch/llg-master.md`.

### Now commit all three files as ONE commit

```
.claude/scripts/kb-commit.mjs \
  --repo BoardPandas/LL-G \
  --base "$BASE" \
  --message "Add <tech> gotcha: <title>" \
  kb/<tech>/<slug>.md  .agent-scratch/llg-entry.md \
  kb/<tech>/llms.txt   .agent-scratch/llg-index.md \
  llms.txt             .agent-scratch/llg-master.md
```

Add `--dry-run` first if you want to see the paths and byte counts without writing.

The script normalizes content to LF before encoding. The contents API stores bytes verbatim,
so `.gitattributes eol=lf` does not govern this path -- a CRLF scratch file (which the Write
tool produces on Windows) otherwise puts CRs straight into the knowledge base, and 24 of them
arrived in a shelf index that way on 2026-09-10.

**If the ref update is refused**, `main` moved while you were composing. Nothing was lost and
nothing was clobbered. Re-read the two index files, re-apply your edits on top of the NEW
content, capture a new `BASE` first, reread every file at that ref, reapply and validate.
Stop after two conflict retries and report the competing work. Never retry with force.

Then delete the scratch files: `rm -f .agent-scratch/llg-*.md` (leave the directory: other skills share it)

## Step 7: Close the loop -- does this lesson ALSO need enforcement?

**The LL-G entry from Steps 1-6 is not optional and is never replaced by anything
here.** Writing it is what this skill is for. Step 7 decides only what to add *on
top of it*, and the answer is often nothing.

An LL-G entry *teaches*. It does not *enforce*: nothing fails when the lesson is
forgotten, which is how the same gotcha returns a year later in a different repo.

**If the lesson concerns this repository's own Claude configuration** -- a hook that
did not fire, a rule that never loaded, frontmatter silently ignored, a skill on the
wrong model, a guard that could be satisfied vacuously -- and the defect is *statically*
checkable, it also needs a guard check, or nothing stops it recurring:

1. Add a check to `scripts/check-claude-wiring.mjs` plus a case in
   `scripts/check-claude-wiring.test.mjs` proving the check fires.
2. Run `npm run check:claude` and `npm test`. Both must pass.

If the defect is behavioural and no static check can reach it, the LL-G entry is the
fix; if the mistake has now repeated, also add it to `Things Claude Gets Wrong` in
`CLAUDE.md`.

**If the lesson is about a technology rather than this configuration** (a Graph API
quirk, a Next.js footgun), the LL-G entry already written is the whole fix. Say so and
stop -- do not manufacture a guard check for something no static check can reach.

Either way, the LL-G entry ships. A guard check is enforcement added beside it, never a
substitute for it: a check that lives only in this repo teaches no other repo anything,
which is the entire reason LL-G exists.

State which of the two this was, and why. "No guard check: this is a PowerShell
gotcha, not a configuration defect" is a complete and correct answer.

## Step 8: Confirm

Confirm the commit actually passed LL-G's guards -- the push is not the finish line:
```
gh run list --repo BoardPandas/LL-G --commit <commit-sha> --json conclusion,headSha,status,url
```
`check-kb.mjs` and `check-index-counts.mjs` both run there. A red build means the entry is
orphaned, its frontmatter is off, the master count disagrees, or CRs got in; fix it rather
than leaving the knowledge base broken for the next reader.

Output:
- The commit URL (printed by `kb-commit.mjs`)
- That all three files landed in that single commit
- The entry's severity level
- The CI conclusion for that exact commit (missing or running checks remain pending)
- Whether a guard check was added, or why none was needed

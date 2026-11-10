---
name: add-practice
model: haiku
effort: low
description: Add a new best-practice entry to the shared BP knowledge base. Use this whenever the user says "add a practice", "record this pattern", "save this to BP", "this should be a best practice", or describes a proven, reusable convention, config, or workflow that other repos should adopt -- even if they don't say "BP" explicitly.
user-invocable: true
argument-hint: (optional) the practice title or a short description of the pattern
allowed-tools:
  - Bash
  - Read
  - Write
---

You are adding a new entry to the BP best practices knowledge base.

**Repository:** `BoardPandas/BP` on GitHub
**Raw URL base:** `https://raw.githubusercontent.com/BoardPandas/BP/main/`

Use authenticated `gh` and Git for GitHub operations. This is the approved transport for
these knowledge contributions, including where other external services use an MCP Hub.
No coordination service, session registration, proposal queue, or MCP connection is needed.

All three files this skill touches -- the entry, its concern `llms.txt`, and the master index count -- are written as **ONE commit** by `.claude/scripts/kb-commit.mjs`. You compute the file contents; the script builds the commit.

Do not push them one at a time. The three files are only correct together: after a lone entry push the entry is a file nothing links, and after a lone concern push the master count disagrees with its shelf. Both states fail BP's CI (`scripts/check-practices.mjs`). `kb-upsert.sh` (single-file upsert) also re-reads each blob SHA immediately before its PUT, which inverts the compare-and-swap and silently overwrites anything that landed while you were editing -- see LL-G `kb/git/github-contents-sha-refresh-defeats-cas.md`. `kb-commit.mjs` takes the base commit you edited from and refuses a non-fast-forward instead.

## Step 1: Collect information

Use facts already established in the conversation; ask only for missing information:
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

**Capture the base commit SHA first, before you read or edit anything.** This is what makes the
write safe: your commit is parented on it, and if anyone pushes while you are composing the
entry, the ref update is refused rather than silently overwriting them.
```
BASE=$(gh api repos/BoardPandas/BP/commits/heads/main --jq .sha)
echo "$BASE"
```
Keep that value; you pass it to `--base` in Step 6. Do not re-read it later -- a freshly-read
SHA always matches HEAD, which is the bug this avoids.

Read the current master index and the relevant concern index so you know the entry count and can avoid duplicates:
```
gh api "repos/BoardPandas/BP/contents/llms.txt?ref=$BASE" -H 'Accept: application/vnd.github.raw'
gh api "repos/BoardPandas/BP/contents/practices/<concern>/llms.txt?ref=$BASE" -H 'Accept: application/vnd.github.raw'
```
If the concern command fails with a `404`, the concern folder does not exist yet -- you will create it in Step 5.

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

1. Create the scratch directory, then use the Write tool to save the entry markdown to `.agent-scratch/bp-entry.md`:

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
   concern: <concern>
   tech: [tech1, tech2]
   priority: <foundational|recommended|optional>
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

2. Leave it on disk. Nothing is pushed until Step 6, which commits all three files together.

`## CHECK` and `## IMPLEMENT` are not optional: `/apply-practice` reads exactly those two
sections to decide whether a repo already follows the practice and how to adopt it. An entry
without them cannot be applied mechanically, and BP's guard fails the build rather than letting
the skill find nothing to act on. A rationale section is required too -- `## WHY`, or
`## CONTEXT` if that reads better for the entry.

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

Write the full new file content to `.agent-scratch/bp-index.md` with the Write tool. Do not push it yet.

Keep the whole bullet on one line -- BP's guard counts entries by line, and only bullets under
the `## Entries` heading are counted. If the concern carries companion files (a runbook, a
script), they belong in their own `## Companion files` section below the entries, where they are
still reachable but do not inflate the count.

## Step 6: Update master llms.txt entry count

Take the master `llms.txt` content from Step 3. Find the bullet for this concern and increment the entry count in parentheses: `(N entries)` -> `(N+1 entries)`.

If this is a new concern, add a new section under `## Concerns`:
```
### <Concern>
- [<Concern> index](practices/<concern>/llms.txt): <description> (1 entry)
```

Edit the master line **line-anchored** -- change only the number, never reflow the
description. Master-index lines are one clause naming the concern and its scope; the specific
practices belong on the shelf. The file has a hard 12 KB budget that fails CI, because every
session loads it under RULE 3 (two descriptions had already grown into run-on sentences, one of
them 725 bytes).

Write the updated master content to `.agent-scratch/bp-master.md`.

### Now commit all three files as ONE commit

```
.claude/scripts/kb-commit.mjs \
  --repo BoardPandas/BP \
  --base "$BASE" \
  --message "Add <concern> practice: <title>" \
  practices/<concern>/<slug>.md  .agent-scratch/bp-entry.md \
  practices/<concern>/llms.txt   .agent-scratch/bp-index.md \
  llms.txt                       .agent-scratch/bp-master.md
```

Add `--dry-run` first if you want to see the paths and byte counts without writing.

The script normalizes content to LF before encoding. The contents API stores bytes verbatim,
so `.gitattributes eol=lf` does not govern this path -- a CRLF scratch file (which the Write
tool produces on Windows) otherwise puts CRs straight into the knowledge base, and 24 of them
arrived in an LL-G shelf index that way on 2026-09-10.

**If the ref update is refused**, `main` moved while you were composing. Nothing was lost and
nothing was clobbered. Re-read the two index files, re-apply your edits on top of the NEW
content, capture a new `BASE` first, reread every file at that ref, reapply and validate.
Stop after two conflict retries and report the competing work. Never retry with force.

Then delete the scratch files: `rm -f .agent-scratch/bp-*.md` (leave the directory: other skills share it)

## Step 7: Confirm

Confirm the commit actually passed BP's guard -- the push is not the finish line:
```
gh run list --repo BoardPandas/BP --commit <commit-sha> --json conclusion,headSha,status,url
```
`check-practices.mjs` runs there. A red build means the entry is unreachable, its
`concern:`/`tech:`/`priority:` is off, the master count disagrees, a required section is
missing, or CRs got in; fix it rather than leaving the knowledge base broken for the next reader.

Output:
- The commit URL (printed by `kb-commit.mjs`)
- That all three files landed in that single commit
- The entry's priority level
- The CI conclusion for that exact commit (missing or running checks remain pending)

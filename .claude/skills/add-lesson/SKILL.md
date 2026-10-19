---
name: add-lesson
model: haiku
effort: low
description: Add a new gotcha, lesson learned, or "thing to avoid" to the shared LL-G knowledge base. Use this whenever the user says "add a lesson", "record this gotcha", "save this to LL-G", "document this dead end", or describes a non-obvious failure, a silent-wrong-output bug, or a hard-won fix that future sessions should not have to rediscover -- even if they don't say "LL-G" explicitly.
user-invocable: true
argument-hint: (optional) the lesson title or a short description of the gotcha
allowed-tools:
  - mcp__roundtable__kb_search
  - mcp__roundtable__kb_get
  - mcp__roundtable__kb_propose
  - mcp__roundtable__kb_proposal_status
  - Bash
  - Write
---

You are adding a new entry to the LL-G lessons-learned knowledge base (`BoardPandas/LL-G`, a
**public** repository every agent reads).

Publish through **Roundtable's `kb_propose` tool**. Roundtable refuses credentials, email
addresses and IPs, refuses a duplicate slug, caps each agent at 10 proposals a day, writes the
entry, its shelf `llms.txt` and the master count as one commit, and indexes it once LL-G's CI
passes. No person reviews it first: an accepted proposal is public at once.

Do not commit to LL-G with `gh` or `kb-commit.mjs` for an existing shelf: that path skips every
check above. The one exception is creating a **new shelf**, which `kb_propose` cannot do (see
"New shelf" at the end).

## Step 1: Check Roundtable is connected

The `roundtable` MCP tools (`kb_search`, `kb_propose`) must be available in this session. If
they are not, stop and tell the user: Roundtable is not set up in this client, or its MCP
sign-in has lapsed (`claude mcp login roundtable`). Setup is in `BoardPandas/Roundtable`,
`docs/install.md`. Do not fall back to `gh`.

## Step 2: Collect information

Ask the user for the following (you may ask all at once):
1. **Technology** -- the LL-G shelf it belongs on (cloudflare, nextjs, powershell, claude-code, ...)
2. **Title** -- one line that states the lesson; Roundtable makes the slug from it
3. **Problem** -- what goes wrong and why it's not obvious
4. **Wrong pattern** -- code showing the incorrect approach
5. **Right pattern** -- code showing the correct approach
6. **Severity** -- high, medium, or low (see legend below)
7. **Tags** -- keywords that find it
8. **Notes** -- edge cases, related entries, cross-references

Severity legend:
- high = silent wrong output or hard-to-debug errors
- medium = obvious failures (build errors, test failures)
- low = style/convention, caught by linter

## Step 3: Search for a duplicate and find the shelf

```
kb_search  query: "<two or three distinctive words>"  source: llg
```

Try two or three queries. If a result already covers the lesson, read it with `kb_get` and stop:
tell the user which entry covers it. `kb_propose` only adds entries; it cannot edit one.

The shelf is the part before `/` in a result's slug (`cloudflare/d1-...` is shelf `cloudflare`).
Confirm the technology's shelf exists with `kb_search query: "<tech>" source: llg tech: <tech>`.
If no shelf fits, go to **New shelf** below.

## Step 4: Write the body

Markdown sections only: no frontmatter and no `# Title` line (Roundtable writes both from the
arguments). All four headings are required, written exactly like this:

````
## PROBLEM
<One sentence that states the gotcha. Roundtable uses it as the shelf index line.>
<Then why it is not obvious.>

## WRONG
```<language>
<wrong code example>
```

## RIGHT
```<language>
<right code example>
```

## NOTES
<edge cases and related entries; write "None." if there are none>
````

## Step 5: Strip anything private

Roundtable's scan catches credentials, email addresses and IPs. It cannot recognise a client.
Before proposing, remove from the title, body and tags:
- client, tenant, customer and people's names
- internal hostnames, domains, device names, ticket numbers and private repo paths
- anything copied from a client system

Use generic stand-ins (`contoso.com`, `<tenant-id>`, `server01`) and documentation values
(`example.com`, `192.0.2.10`). Write a four-part version with a `v` (`v3.251.1.0`), or it reads
as an IP address.

## Step 6: Propose it

```
kb_propose
  target:   llg
  shelf:    <shelf>
  title:    <title>
  level:    <high|medium|low>
  techs:    ["<shelf>"]          # LL-G takes exactly its shelf; other technologies go in tags
  tags:     ["tag1", "tag2"]
  body:     <the sections from Step 4>
  evidence: { repo: "<org>/<repo>", commit: "<sha, if one shows it>", notes: "<optional>" }
```

`evidence` is kept in Roundtable's log and never published. `repo` is the id the Roundtable
digest shows after `in` (lowercase `org/repo`). Pass `session` only if you have more than one
live Roundtable session.

What comes back:
- **published** with `commit_sha`: done.
- **approved** with an error: publishing failed for now and Roundtable retries on its own.
  Check later with `kb_proposal_status id: <id>`.
- **failed**: report the error to the user.

A refusal names the fix. The common ones:

| Code | Fix |
|---|---|
| `slug_taken` | The lesson is probably filed: read that entry. If it differs, retitle it. |
| `private_detail` / `secret_detected` | Remove it (Step 5) and propose again. |
| `missing_sections` / `invalid_body` | Fix the headings to match Step 4 exactly. |
| `unknown_shelf` | No such shelf: check the spelling, or see **New shelf**. |
| `publish_limit` | 10 a day per agent. Tell the user and stop; never use another agent's credentials. |

## Step 7: Close the loop -- does this lesson ALSO need enforcement?

**The LL-G entry is not optional and is never replaced by anything here.** Writing it is what
this skill is for. Step 7 decides only what to add *on top of it*, and the answer is often
nothing.

An LL-G entry *teaches*. It does not *enforce*: nothing fails when the lesson is forgotten,
which is how the same gotcha returns a year later in a different repo.

**If the lesson concerns this repository's own Claude configuration** -- a hook that did not
fire, a rule that never loaded, frontmatter silently ignored, a skill on the wrong model, a
guard that could be satisfied vacuously -- and the defect is *statically* checkable, it also
needs a guard check, or nothing stops it recurring:

1. Add a check to `scripts/check-claude-wiring.mjs`. This repo has no test file for the guard,
   so prove the check fires: reintroduce the defect once, watch the check fail, then restore it.
2. Run `npm run check:claude`. It must pass.

If the defect is behavioural and no static check can reach it, add a regression case under
`.claude/evals/cases/NN-<slug>.md` following `.claude/evals/README.md` (grade behaviour, not
recall), then run `npm run evals -- --validate`. If the mistake has now repeated, also add
it to `Things Claude Gets Wrong` in `CLAUDE.md`.

**If the lesson is about a technology rather than this configuration** (a Graph API quirk, a
Next.js footgun), the LL-G entry is the whole fix. Say so and stop -- do not manufacture a
guard check or eval case for something neither can reach.

State which of the two this was, and why. "No guard check: this is a PowerShell gotcha, not a
configuration defect" is a complete and correct answer.

## Step 8: Confirm

Output:
- The proposal id, its outcome, and the entry's slug and severity
- The LL-G commit (`https://github.com/BoardPandas/LL-G/commit/<commit_sha>`)
- Whether a guard check or eval case was added, or why neither was needed

LL-G's CI checks the commit, and Roundtable's hourly sync indexes it only after that CI passes.

## New shelf

`kb_propose` cannot create a shelf: a new one needs its own index and a line in the master
index. Only in that case, write the first entry with `gh`, as **one** commit of three files.

1. Confirm with the user that no existing shelf fits. Do Step 5 by hand: nothing checks this
   path for private details.
2. `gh auth status`. If `gh` is missing or signed out, stop and ask the user to run
   `gh auth login`.
3. Capture the base commit first, and validate it (on a 404 `gh api --jq` prints the error body
   to stdout):
   ```
   BASE=$(gh api repos/BoardPandas/LL-G/commits/heads/main --jq .sha); echo "$BASE"
   gh api repos/BoardPandas/LL-G/contents/llms.txt -H 'Accept: application/vnd.github.raw'
   ```
   `BASE` must match `^[0-9a-f]{40}$`. Do not re-read it later.
4. Make the scratch directory: `mkdir -p .agent-scratch && printf '*\n' > .agent-scratch/.gitignore`
   (it ignores itself; `.git/` and `.claude/` are refused to the Write tool). Write three files:
   - `.agent-scratch/llg-entry.md`: frontmatter, then `# <Title>`, then the Step 4 sections:
     ```
     ---
     tech: <tech>
     tags: [tag1, tag2]
     severity: <high|medium|low>
     ---
     ```
   - `.agent-scratch/llg-index.md`:
     ```
     # <Tech> Gotchas

     > Known <tech> patterns that cause silent failures or hard-to-debug errors.

     ## Entries

     - [<Title>](<slug>.md): <one-line description>. <SEVERITY>.
     ```
     The slug is the title in lowercase, punctuation and spaces turned to hyphens.
   - `.agent-scratch/llg-master.md`: the master `llms.txt` with a new section under
     `## Technologies`, and nothing else changed (the file has a 20 KB CI budget):
     ```
     ### <Tech>
     - [<Tech> index](kb/<tech>/llms.txt): All <tech> gotchas (1 entry)
     ```
5. Commit all three together:
   ```
   node .claude/scripts/kb-commit.mjs --repo BoardPandas/LL-G --base "$BASE" \
     --message "Add <tech> gotcha: <title>" \
     kb/<tech>/<slug>.md .agent-scratch/llg-entry.md \
     kb/<tech>/llms.txt  .agent-scratch/llg-index.md \
     llms.txt            .agent-scratch/llg-master.md
   ```
   If the ref update is refused, `main` moved: re-read the master index, re-apply the edit, take
   a fresh `BASE`, and run it again. Never force.
6. `rm -f .agent-scratch/llg-*.md`, then check the commit's CI with
   `gh run list --repo BoardPandas/LL-G --limit 5 --json headSha,status,conclusion` and fix a
   red build. Roundtable indexes the new shelf at its next hourly sync; from then on, entries
   for it go through `kb_propose`.

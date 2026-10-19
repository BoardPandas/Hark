---
name: add-practice
model: haiku
effort: low
description: Add a new best-practice entry to the shared BP knowledge base. Use this whenever the user says "add a practice", "record this pattern", "save this to BP", "this should be a best practice", or describes a proven, reusable convention, config, or workflow that other repos should adopt -- even if they don't say "BP" explicitly.
user-invocable: true
argument-hint: (optional) the practice title or a short description of the pattern
allowed-tools:
  - mcp__roundtable__kb_search
  - mcp__roundtable__kb_get
  - mcp__roundtable__kb_propose
  - mcp__roundtable__kb_proposal_status
  - Bash
  - Write
---

You are adding a new entry to the BP best practices knowledge base (`BoardPandas/BP`, a
**public** repository every agent reads).

Publish through **Roundtable's `kb_propose` tool**. Roundtable refuses credentials, email
addresses and IPs, refuses a duplicate slug, caps each agent at 10 proposals a day, writes the
entry, its concern `llms.txt` and the master count as one commit, and indexes it once BP's CI
passes. No person reviews it first: an accepted proposal is public at once.

Do not commit to BP with `gh` or `kb-commit.mjs` for an existing concern: that path skips every
check above. The one exception is creating a **new concern**, which `kb_propose` cannot do (see
"New concern" at the end).

## Step 1: Check Roundtable is connected

The `roundtable` MCP tools (`kb_search`, `kb_propose`) must be available in this session. If
they are not, stop and tell the user: Roundtable is not set up in this client, or its MCP
sign-in has lapsed (`claude mcp login roundtable`). Setup is in `BoardPandas/Roundtable`,
`docs/install.md`. Do not fall back to `gh`.

## Step 2: Collect information

Ask the user for the following (you may ask all at once):
1. **Concern** -- the BP concern it belongs in (claude-config, testing, deployment, safety, ...)
2. **Title** -- one line that names the practice; Roundtable makes the slug from it
3. **Pattern** -- what the proven pattern looks like and how it works
4. **Why** -- why this is better than alternatives, what problems it prevents
5. **Example** -- code or config that shows it
6. **Priority** -- foundational, recommended, or optional (see legend below)
7. **Techs** -- the technologies it applies to
8. **Tags** -- keywords that find it
9. **Check** -- how to verify if a repo already follows this (checklist items)
10. **Implement** -- steps to adopt this in a repo that doesn't have it
11. **Notes** -- edge cases, caveats, related practices

Priority legend:
- foundational = universal pattern every repo should follow
- recommended = strong pattern for repos with matching techs
- optional = nice-to-have improvement

## Step 3: Search for a duplicate and find the concern

```
kb_search  query: "<two or three distinctive words>"  source: bp
```

Try two or three queries. If a result already covers the practice, read it with `kb_get` and
stop: tell the user which entry covers it. `kb_propose` only adds entries; it cannot edit one.

The concern is the part before `/` in a result's slug (`testing/...` is concern `testing`). If no
concern fits, go to **New concern** below.

## Step 4: Write the body

Markdown sections only: no frontmatter and no `# Title` line (Roundtable writes both from the
arguments). All six headings are required, written exactly like this:

```
## PATTERN
<One sentence that states the practice. Roundtable uses it as the concern index line.>
<Then how it works.>

## WHY
<why this is better, what it prevents>

## EXAMPLE
<code or config, with file paths>

## CHECK
How to verify if a repo already follows this:
- [ ] Check condition 1
- [ ] Check condition 2

## IMPLEMENT
Steps to adopt this in a repo that doesn't have it:
1. Step one
2. Step two

## NOTES
<edge cases and related practices; write "None." if there are none>
```

`## CHECK` and `## IMPLEMENT` are what `/apply-practice` reads to decide whether a repo already
follows the practice and how to adopt it. Make them concrete enough to act on mechanically.

## Step 5: Strip anything private

Roundtable's scan catches credentials, email addresses and IPs. It cannot recognise a client.
Before proposing, remove from the title, body and tags:
- client, tenant, customer and people's names
- internal hostnames, domains, device names, ticket numbers and private repo paths
- anything copied from a client system

Use generic stand-ins (`contoso.com`, `<tenant-id>`, `server01`) and documentation values
(`example.com`, `192.0.2.10`). Write a four-part version with a `v` (`v3.251.1.0`), or it reads
as an IP address. The source repo goes in `evidence`, which is never published.

## Step 6: Propose it

```
kb_propose
  target:   bp
  shelf:    <concern>
  title:    <title>
  level:    <foundational|recommended|optional>
  techs:    ["tech1", "tech2"]   # written as both tech: and applies-to:
  tags:     ["tag1", "tag2"]
  body:     <the sections from Step 4>
  evidence: { repo: "<org>/<repo>", commit: "<sha, if one shows it>", notes: "<optional>" }
```

`repo` is the id the Roundtable digest shows after `in` (lowercase `org/repo`). Pass `session`
only if you have more than one live Roundtable session.

What comes back:
- **published** with `commit_sha`: done.
- **approved** with an error: publishing failed for now and Roundtable retries on its own.
  Check later with `kb_proposal_status id: <id>`.
- **failed**: report the error to the user.

A refusal names the fix. The common ones:

| Code | Fix |
|---|---|
| `slug_taken` | The practice is probably filed: read that entry. If it differs, retitle it. |
| `private_detail` / `secret_detected` | Remove it (Step 5) and propose again. |
| `missing_sections` / `invalid_body` | Fix the headings to match Step 4 exactly. |
| `unknown_shelf` | No such concern: check the spelling, or see **New concern**. |
| `publish_limit` | 10 a day per agent. Tell the user and stop; never use another agent's credentials. |

## Step 7: Confirm

Output:
- The proposal id, its outcome, and the entry's slug and priority
- The BP commit (`https://github.com/BoardPandas/BP/commit/<commit_sha>`)

BP's CI checks the commit, and Roundtable's hourly sync indexes it only after that CI passes.

## New concern

`kb_propose` cannot create a concern: a new one needs its own index and a line in the master
index. Only in that case, write the first entry with `gh`, as **one** commit of three files.

1. Confirm with the user that no existing concern fits. Do Step 5 by hand: nothing checks this
   path for private details.
2. `gh auth status`. If `gh` is missing or signed out, stop and ask the user to run
   `gh auth login`.
3. Capture the base commit first, and validate it (on a 404 `gh api --jq` prints the error body
   to stdout):
   ```
   BASE=$(gh api repos/BoardPandas/BP/commits/heads/main --jq .sha); echo "$BASE"
   gh api repos/BoardPandas/BP/contents/llms.txt -H 'Accept: application/vnd.github.raw'
   ```
   `BASE` must match `^[0-9a-f]{40}$`. Do not re-read it later.
4. Make the scratch directory: `mkdir -p .agent-scratch && printf '*\n' > .agent-scratch/.gitignore`
   (it ignores itself; `.git/` and `.claude/` are refused to the Write tool). Write three files:
   - `.agent-scratch/bp-entry.md`: frontmatter, then `# <Title>`, then the Step 4 sections:
     ```
     ---
     concern: <concern>
     tech: [tech1, tech2]
     priority: <foundational|recommended|optional>
     applies-to: [tech1, tech2]
     ---
     ```
   - `.agent-scratch/bp-index.md`:
     ```
     # <Concern> Best Practices

     > Proven <concern> patterns.

     ## Entries

     - [<Title>](<slug>.md): <one-line description>. <PRIORITY>.
     ```
     The slug is the title in lowercase, punctuation and spaces turned to hyphens.
   - `.agent-scratch/bp-master.md`: the master `llms.txt` with a new section under
     `## Concerns`, and nothing else changed (the file has a 12 KB CI budget):
     ```
     ### <Concern>
     - [<Concern> index](practices/<concern>/llms.txt): <one clause> (1 entry)
     ```
5. Commit all three together:
   ```
   node .claude/scripts/kb-commit.mjs --repo BoardPandas/BP --base "$BASE" \
     --message "Add <concern> practice: <title>" \
     practices/<concern>/<slug>.md .agent-scratch/bp-entry.md \
     practices/<concern>/llms.txt  .agent-scratch/bp-index.md \
     llms.txt                      .agent-scratch/bp-master.md
   ```
   If the ref update is refused, `main` moved: re-read the master index, re-apply the edit, take
   a fresh `BASE`, and run it again. Never force.
6. `rm -f .agent-scratch/bp-*.md`, then check the commit's CI with
   `gh run list --repo BoardPandas/BP --limit 5 --json headSha,status,conclusion` and fix a red
   build. Roundtable indexes the new concern at its next hourly sync; from then on, entries for
   it go through `kb_propose`.

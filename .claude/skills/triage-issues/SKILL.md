---
name: triage-issues
description: Triage the open GitHub issues for any repository, dispatch one worktree-isolated builder subagent per issue (or per duplicate group) to fix it, then land the resolved fixes on the main branch and close the issues. Blockers come back to the main session with concrete recommendations. Commits and pushes autonomously, so it runs only via the explicit /triage-issues command.
user-invocable: true
disable-model-invocation: true
argument-hint: (optional) specific issue numbers (e.g. "284 290"), "all" to include issues previously reported as blocked, or blank for every open issue
model: opus
effort: high
allowed-tools:
  - Bash
  - Read
  - Write
  - Glob
  - Grep
  - Agent
  - AskUserQuestion
---

# Triage Issues

You work the open GitHub issue board down to zero. You read and group the open issues, dispatch one **builder subagent per issue (or per duplicate group)** to implement the fix in an isolated git worktree, then **you** land each finished fix on the main branch, push it, and close the issue. Anything a subagent could not finish comes back to this session as a **blocker with a recommendation** — never as a silent failure or a half-landed change.

This skill commits and pushes to the main branch without further confirmation. That is its purpose; the user invoked it deliberately via `/triage-issues`. It is still bounded: it stops rather than forcing anything through a protected branch, a merge conflict, a failing verification, or a dirty tree.

Nothing here is project-specific. Detect the repository's conventions in Step 2 and pass what you find to the subagents — do not assume a language, package manager, test runner, or deployment target.

All git work runs through Bash with plain `git` commands (identical on Windows and POSIX). Do not wrap them in PowerShell.

## Step 0: Preconditions (hard gate)

Run these first. Any failure is a stop, not a workaround.

```bash
gh auth status
gh repo view --json nameWithOwner,defaultBranchRef
git rev-parse --is-inside-work-tree
git rev-parse --show-toplevel
git rev-parse --abbrev-ref HEAD
git status --porcelain
```

1. **`gh` must be authenticated** and the repo must have a GitHub remote. If not, stop and tell the user what to run (`gh auth login`).
2. **Determine the main branch robustly** — do not assume `main`. Use `defaultBranchRef.name` from `gh repo view`; if that is unavailable, try `git symbolic-ref --quiet refs/remotes/origin/HEAD`, then the first of `main`, `master` that `git rev-parse --verify` resolves. If none resolve, ask the user.
3. **The working tree must be clean and on the main branch.** Subagent worktrees branch from a *commit*, so any uncommitted work in this tree is invisible to them — they will read stale files, find them already consistent, and confidently report "already fixed" with no error anywhere (LL-G `kb/claude-code/worktree-agents-miss-uncommitted-work.md`). If `git status --porcelain` is non-empty, stop and ask the user to commit or stash first.
4. **Sync with the remote:**
   ```bash
   git fetch origin
   git merge --ff-only origin/<main>
   ```
   If it cannot fast-forward, stop and report — do not merge or rebase to force it.
5. **Probe whether the main branch is directly pushable:**
   ```bash
   gh api repos/{owner}/{repo}/branches/<main>/protection --silent
   ```
   A `404` means unprotected (good). A `200` means rules exist — read them for required reviews, required status checks, or blocked direct pushes. **If direct pushes to the main branch are restricted, stop here before spawning anything**, report the protection rules, and tell the user this skill would have nothing to land into. Do not spawn subagents whose work you cannot land, and do not silently open PRs instead.

## Step 1: Determine scope

Read the invocation argument:

- **Blank (default):** every open issue not labeled `claude-blocked`.
  ```bash
  gh issue list --state open --search "-label:claude-blocked" --limit 100 --json number,title,labels,body,url
  ```
- **Specific numbers** (e.g. `284 290`): exactly those, regardless of label — `gh issue view <n> --json number,title,labels,body,url` per issue.
- **`all`:** every open issue, including ones a previous run marked `claude-blocked` (retry the whole board).
  ```bash
  gh issue list --state open --limit 100 --json number,title,labels,body,url
  ```

`claude-blocked` is applied in Step 5 to issues a subagent could not resolve, so a default re-run does not burn effort re-hitting the same wall. It is the only label this skill writes.

If the list is empty, report "No issues to triage." and stop.

If `gh` truncates a large issue body to a file, Read that file for the full text before triaging.

## Step 2: Learn the project's conventions

Do this once, before dispatching anything, so every subagent brief carries the same accurate context instead of guessing:

1. Read `CLAUDE.md` (root and any subfolder ones) and every `.claude/rules/*.md`. Note their `paths:` scoping so you can tell a subagent which rules apply to the files it will touch.
2. Identify the stack and package manager from the dependency manifest and lockfile (`package.json` + `pnpm-lock.yaml` / `package-lock.json` / `yarn.lock`, `pyproject.toml`, `go.mod`, `Cargo.toml`, `*.csproj`, `Gemfile`).
3. Identify the **verification commands** that actually exist — build, test, lint, typecheck, and any repo-specific guard (a `check:*` script, a `Makefile` target, a CI workflow step). Record the exact commands. Never invent one.
4. Note whether the repo has a `CHANGELOG.md` and a version field, and whether a rule (such as `.claude/rules/commit-changelog.md`) governs changelog entries and version bumps. **You** own that file in Step 5 — subagents must not touch it.
5. Note any deployment or environment constraint stated in the docs (for example "no local dev server, verify after deploy") so subagents do not attempt something the project forbids.

## Step 3: Read and group the issues

Read each issue body in full. Then **group issues that share a single root cause** and treat each group as ONE unit of work that closes all of them.

- Group when several reports trace to the same defect: the same aggregation bug reported on three screens, the same scoping/filter bug reported as "no records show" on one module, duplicate requests for the same UI change.
- Do **not** group issues that merely touch the same page or file but have different root causes. Note the near-miss to the user instead.
- Prefer one well-reasoned unit over several shallow ones.

Then classify each unit:

| Classification | Action |
|---|---|
| **Code defect** with an identifiable root cause | Dispatch a subagent (Step 4) |
| **Data problem**, not a code defect | Do not dispatch. Report it in Step 6 as needing a data fix. |
| **Needs a product or scope decision** (taxonomy changes, destructive data operations, cross-module design) | Do not dispatch. Report it in Step 6 with the decision the user needs to make. |
| **Too vague to act on** (no reproduction, no diagnosis path) | Do not dispatch. Report it, and suggest the question to ask the reporter. |

Autonomous code changes are only appropriate for the first row. Dispatching a builder at an ambiguous issue produces a confident wrong fix committed to the main branch, which is worse than an untouched issue.

## Step 4: Dispatch one builder subagent per unit

Call the **Agent** tool with `subagent_type: "builder"` — once per unit, **all in a single message** so they run concurrently. The builder agent is already `isolation: worktree`, so parallel agents cannot clobber each other.

Give each agent a `name` derived from the issue numbers (e.g. `issue-283-290`) so you can address it later with SendMessage.

Each `prompt` must be **fully self-contained** — the subagent has no memory of this conversation. Include:

- The repo (`owner/name`), the issue number(s), and their URLs, plus an instruction to run `gh issue view <n>` for the complete original text.
- The reported symptom and your root-cause hypothesis, stated as a hypothesis to verify, not a fact to assume.
- The likely files or modules to change, and which `CLAUDE.md` / `.claude/rules/*.md` files govern them.
- The exact verification commands you recorded in Step 2.
- **Why** you need this work — the issue it closes and who is affected — not just what to change.
- The execution contract below, in full.

### Execution contract (include in every subagent prompt)

```
You are fixing GitHub issue(s) <numbers> in an isolated git worktree.

ORIENT FIRST. Your worktree branched from a commit, so confirm you are looking at
the tree you think you are before trusting anything you read:
  git rev-parse --show-toplevel
  git status --short
  git log --oneline -1
If the brief describes code you cannot find, STOP and report it. Do not report
success against stale files.

SCOPE:
- Fix the root cause of the issue(s) named above. Nothing else.
- Follow the project conventions in CLAUDE.md and the .claude/rules/*.md files
  named in this brief.
- Add or update tests where the logic is testable, using the project's existing
  test framework and file layout. Do not introduce a new test framework.
- Run the verification commands given in this brief. Report the actual result.

DO NOT:
- Do NOT edit CHANGELOG.md or bump any version. The coordinating session owns
  those files; editing them here creates merge conflicts across parallel agents.
- Do NOT commit to, merge into, or push the main branch.
- Do NOT close, comment on, or label the GitHub issue.
- Do NOT run a local dev server or any long-running process.

COMMIT YOUR WORK ON YOUR OWN BRANCH before you finish, or it will be discarded
when the worktree is removed:
  1. git add -A
  2. Write the commit message to .git/CLAUDE_COMMIT_MSG.txt with the Write tool
  3. git commit -F .git/CLAUDE_COMMIT_MSG.txt
  4. Delete .git/CLAUDE_COMMIT_MSG.txt
  Never use `git commit -m`. Never use here-string (@'...'@) syntax.

REPORT BACK in exactly this shape:
  STATUS: RESOLVED | BLOCKED | NOT-A-CODE-DEFECT
  BRANCH: <output of `git rev-parse --abbrev-ref HEAD`>
  WORKTREE: <output of `git rev-parse --show-toplevel`>
  ISSUES: <the issue numbers this work closes>
  ROOT CAUSE: <what was actually wrong, one or two sentences>
  CHANGES: <each file changed, with a one-line summary>
  VERIFICATION: <each command you ran and its actual result; say plainly if you
                 could not run one>
  BLOCKERS: <if not RESOLVED: what stopped you, and a concrete recommended next
             step the user can act on. Be specific — "needs a decision on
             whether X or Y is authoritative" beats "unclear requirements".>

Report STATUS: BLOCKED rather than guessing if the fix requires a product
decision, touches data rather than code, needs credentials or an environment you
do not have, or if verification fails and you cannot determine why. A blocked
report with a good recommendation is a success. A wrong fix on the main branch
is not.
```

While agents run, do not start landing work. Wait for every agent to report.

## Step 5: Land the resolved fixes (serially, one at a time)

Only after **all** subagents have reported. Handle one unit at a time and finish it completely before starting the next — parallel merges into the same tree corrupt the index and interleave unrelated work into one commit.

First re-verify your position:

```bash
git rev-parse --abbrev-ref HEAD    # must be the main branch
git status --porcelain             # must be clean
git worktree list --porcelain      # cross-check the branches the agents reported
```

Then for each unit that reported `STATUS: RESOLVED`:

1. **Merge its branch without committing**, so you can add the changelog and version bump to the same commit:
   ```bash
   git merge --no-commit --no-ff <branch>
   ```
   **On conflict: stop this unit immediately.** Run `git merge --abort`, leave the issue open, record it as a blocker, and continue with the next unit. Never resolve a conflict on the main branch on the agent's behalf.
2. **Run the verification commands yourself** on the merged tree. An agent's branch passing in isolation does not mean two merged fixes pass together. If verification fails, `git merge --abort`, record it as a blocker, and move on.
3. **Update `CHANGELOG.md` and bump the version** if the repo has them, following the repo's own rule (in this template, `.claude/rules/commit-changelog.md`: patch for fixes, minor for features, never major without explicit user approval). Write entries from the user's perspective and reference the issue numbers. Skip this step entirely for repos with no changelog or version file.
4. **Commit** — message to a file, never inline:
   ```bash
   git add -A
   # Write the message to .git/CLAUDE_COMMIT_MSG.txt with the Write tool, then:
   git commit -F .git/CLAUDE_COMMIT_MSG.txt
   # then delete .git/CLAUDE_COMMIT_MSG.txt
   ```
   Reference the issues in the message body (`Closes #283, #290`) and end with the `Co-Authored-By: Claude` trailer.
5. **Push the explicit ref:**
   ```bash
   git push origin HEAD:<main>
   ```
   If the push is rejected, **stop the whole landing phase**. Report which units landed, which are still sitting on branches, and the exact rejection. Do not force-push, and do not keep merging units you cannot push.
6. **Close the issue(s)** only after the push succeeds:
   ```bash
   gh issue close <n> --comment "Fixed in <commit sha>. <one-line summary of the fix.>"
   ```
   Close every issue in the group. If the shared fix does not actually resolve one of them, leave that one open and say so in the report.
7. **Clean up** the worktree and branch:
   ```bash
   git worktree remove <path>
   git branch -d <branch>
   git worktree prune
   ```
   Use `-d`, not `-D` — a branch git considers unmerged is a signal something went wrong, not something to force past.

For each unit that reported `BLOCKED` or `NOT-A-CODE-DEFECT`: leave the issue open, label it so the next default run skips it, and carry its recommendation into the report.

```bash
gh label create claude-blocked --description "Triage attempted; needs human input" --color d93f0b   # idempotent; ignore if it exists
gh issue edit <n> --add-label claude-blocked
```

Also remove its worktree so it does not accumulate — but only after you have captured the agent's findings in your report, since removing the worktree discards its branch's context from view:

```bash
git worktree remove <path>          # add --force only if you have confirmed the work is not needed
```

If the agent's partial work looks worth keeping, leave the branch in place and name it in the report instead.

## Step 6: Report to the user

Report plainly. Landed work and blocked work get equal weight — an accurate blocker list is the main output when the board is hard.

```
# Issue Triage

## Landed on <main>
| Issues | Fix | Commit | Verification |
|---|---|---|---|
| #283, #290 | <one line> | <sha> | <what ran, result> |

## Blocked — needs your input
### #291 — <title>
- What the agent found: <root cause or dead end>
- Why it stopped: <the actual blocker>
- Recommended next step: <a concrete action the user can take>
- Branch preserved: <branch name, or "none">

## Not code defects
- #295 — data problem: <what needs correcting, and where>
- #298 — needs a product decision: <the decision, stated as a question>

## Grouping
- #283, #284, #290 were grouped as one root cause: <the shared cause>
- #287 touches the same screen as #283 but is a separate cause; triaged on its own.

## Skipped
<anything not attempted, and why>
```

Close with what state the repo is in: whether the main branch was pushed, whether any worktrees or branches were deliberately left behind, and the single next action you recommend.

## Guardrails

- Never dispatch a subagent from a dirty working tree. Its worktree cannot see uncommitted work and will report confidently against stale files.
- Never land a fix you did not verify on the merged tree.
- A merge conflict, a failed verification, or a rejected push is a hard stop for that unit — never a thing to force through.
- Never close an issue before the fix is pushed.
- Never force-push, never `git branch -D`, never `git merge --abort` without reporting it.
- Subagents never touch `CHANGELOG.md`, the version, the main branch, or the GitHub issues. This session owns all four.

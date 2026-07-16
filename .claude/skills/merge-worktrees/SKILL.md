---
name: merge-worktrees
description: Merge every open git worktree and local branch into the main branch, commit and push, then remove the worktrees and delete the branches completely. Use when the user says "merge worktrees", "merge and clean up branches", or wants to consolidate all outstanding work into main and tear down the leftovers.
user-invocable: true
argument-hint: (optional) a branch name or worktree path to limit the operation to
allowed-tools:
  - Bash
  - AskUserQuestion
model: sonnet
effort: medium
disable-model-invocation: true
---

You consolidate all outstanding work into the repository's main branch, push it, then tear down the worktrees and branches you merged. **This is destructive and irreversible once branches are force-deleted.** Move deliberately, confirm before the point of no return, and never plow through a merge conflict or dirty tree.

All git operations run through the Bash tool. Run plain git commands (identical on Windows and POSIX) -- do not wrap them in PowerShell.

## Step 1: Inventory the repository

Run these and read the results before doing anything else:

```
git rev-parse --is-inside-work-tree
git rev-parse --abbrev-ref HEAD
git worktree list --porcelain
git branch --format='%(refname:short)'
git remote
```

Determine the **main branch** robustly (do not assume `main`):

```
git symbolic-ref --quiet refs/remotes/origin/HEAD
```

If that fails (no remote / not set), fall back to the first of `main`, then `master`, that `git rev-parse --verify <name>` resolves. If none resolve, stop and ask the user which branch is the integration target.

Record the **primary worktree** (the first entry in `git worktree list` -- the one containing `.git`). Merges happen there.

## Step 2: Build and confirm the plan

From the inventory, classify:

- **Worktrees to merge + remove:** every linked worktree except the primary one. Note each one's path and checked-out branch.
- **Branches to merge + delete:** every local branch except the main branch (and except any branch the user scoped out via the argument). A worktree's branch counts here too.

If an argument was supplied, narrow the plan to only the matching branch or worktree path.

Present the plan to the user as an explicit list:

> I will merge these into `<main>`, push, then delete them completely:
> - worktree `../feature-x` (branch `feature-x`) -> merge, remove worktree, delete branch
> - branch `bugfix-y` -> merge, delete branch
>
> Branches with a remote counterpart will also be deleted on `origin` (I'll confirm before that).

Use **AskUserQuestion** to get explicit go/no-go before any merge. If the plan is empty (nothing but main, clean), report that there is nothing to do and stop.

## Step 3: Commit pending work in each worktree

A worktree's uncommitted changes are lost when it is removed, so capture them first. For each worktree in the plan (and the primary worktree):

```
git -C <path> status --porcelain
```

If a worktree is dirty, do **not** silently commit. Show the user the changed files and ask whether to (a) commit them on that worktree's branch, (b) discard them, or (c) abort. On "commit", commit on the worktree's own branch:

```
git -C <path> add -A
git -C <path> commit -F .git/CLAUDE_COMMIT_MSG.txt   # write the message to this file first, then delete it
```

Never pass the message inline. If a `CHANGELOG.md` + version-bump rule applies to the repo, honor it before committing.

## Step 4: Prepare the main branch

In the primary worktree, switch to main and make sure it is clean and current:

```
git -C <primary> rev-parse --abbrev-ref HEAD     # verify we are about to act on the right branch
git -C <primary> checkout <main>
git -C <primary> status --porcelain              # must be clean; resolve with the user if not
git -C <primary> pull --ff-only                  # only if a remote exists; stop and ask if it can't fast-forward
```

The branch-verify step matters: with shared worktrees a parallel process can move HEAD, and committing on the wrong branch makes the later push report "Everything up-to-date" while your work sits on the wrong ref.

## Step 5: Merge each branch into main

For each branch in the plan, in the primary worktree:

```
git -C <primary> merge --no-ff <branch> -F .git/CLAUDE_MERGE_MSG.txt   # write a short merge message, then delete it
```

- `--no-ff` keeps each merge visible as its own commit so a single feature can be reverted later.
- **On conflict: STOP immediately.** Run `git -C <primary> merge --abort`, report exactly which branch conflicted and the conflicting files, and hand control back to the user. Do not attempt to auto-resolve and do not continue to the next branch until the user decides. Nothing has been deleted yet, so the repo is safe.

## Step 6: Push main

Push the explicit ref (not a bare `git push`) so the result is unambiguous:

```
git -C <primary> push origin HEAD:<main>
```

Skip only if there is no remote. Confirm the push succeeded before deleting anything -- the push is the durable record of the merge.

## Step 7: Remove the worktrees

For each worktree in the plan:

```
git -C <primary> worktree remove <path>
```

If git refuses because the worktree still has untracked/modified files you already accounted for in Step 3, re-confirm with the user, then re-run with `--force`. After removals:

```
git -C <primary> worktree prune
```

## Step 8: Delete the branches completely

Now the irreversible part. The branches are merged and pushed, so a force-delete is safe.

```
git -C <primary> branch -D <branch>     # for each branch in the plan
```

Use `-D` (force) per the user's "delete them completely" intent; `-d` would refuse anything git thinks is unmerged.

**Remote branches:** if a branch also exists on `origin` (`git ls-remote --heads origin <branch>` returns a hit), the user's "completely" means the remote copy too. Confirm once via AskUserQuestion, then:

```
git -C <primary> push origin --delete <branch>
```

## Step 9: Report

Summarize what happened:
- branches merged into `<main>` and whether main was pushed
- worktrees removed
- local branches deleted, and any remote branches deleted
- anything skipped or aborted (conflicts, dirty trees the user chose to keep), with the exact next step to finish it

If a merge conflict halted the run, the report is the conflict details and recovery steps -- not a claim of success.

## Guardrails

- Never delete the main branch or the primary worktree.
- Never force-delete a branch before its merge is committed and (if a remote exists) pushed.
- A conflict, a non-fast-forward pull, or an unexpected dirty tree is a hard stop, not something to work around.
- Confirm with the user before the first merge and before any remote deletion.

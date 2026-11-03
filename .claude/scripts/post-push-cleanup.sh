#!/usr/bin/env bash
# Post-push hook: once the default branch has been pushed, list the local branches
# and linked worktrees whose work it now contains, so Claude can offer to remove them.
#
# Advisory only. It never deletes anything: every delete waits for the user's yes
# (RULE 0), and a worktree that looks finished may be where a running session --
# a desktop-app session, a subagent -- is still working. The list reaches Claude as
# JSON additionalContext via _hook-context.sh; plain PostToolUse stdout goes to the
# debug log and is never read.
#
# Always exits 0. A read that fails means silence, never an empty "nothing to clean"
# verdict built from a command that did not work.
# (LL-G kb/bash/validate-api-reads-before-deriving-terminal-state.md)

. "$(dirname "$0")/_git-commit-filter.sh"
. "$(dirname "$0")/_hook-context.sh"

REMOTE=origin

# How recent the push to origin/<default> must be to count as the one that just ran.
# The hook fires the moment the command returns, so this is generous; it exists so a
# later push of some other branch does not re-list what the main push already listed.
PUSH_WINDOW=120

read_hook_input

# Text first, as a cheap filter. Codex has no handler-level `if`, so there this
# script runs on every Bash call. The text says only that a push was attempted; the
# reflog check below is what says the default branch actually moved.
# (LL-G kb/claude-code/hook-validates-text-not-state.md)
is_git_subcommand push || exit 0

# Judge the repo the push targeted (`git -C <repo> push`, `cd <repo> && git push`),
# not the one the session started in.
# (LL-G kb/claude-code/hook-cwd-is-not-the-commit-target-repo.md)
anchor_to_repo_root push || exit 0

# Git Bash spells one directory as /c/x, C:/x and C:\x, so compare canonical forms.
# (LL-G kb/bash/git-bash-spells-one-directory-several-ways-so-path-compares-silently-never-match.md)
canon() {
  local p q
  p=$(cd "$1" 2>/dev/null && pwd -P) || p=$1
  q=$(cygpath -m "$p" 2>/dev/null) && [ -n "$q" ] && p=$q
  printf '%s' "$p"
}

# Newline-delimited membership, without bash 4's associative arrays (macOS ships 3.2).
in_list() {
  case $'\n'"$2"$'\n' in *$'\n'"$1"$'\n'*) return 0 ;; esac
  return 1
}

DEFAULT=$(git symbolic-ref --quiet --short "refs/remotes/$REMOTE/HEAD" 2>/dev/null)
DEFAULT=${DEFAULT#"$REMOTE/"}
if [ -z "$DEFAULT" ]; then
  for cand in main master; do
    if git rev-parse -q --verify "refs/remotes/$REMOTE/$cand" >/dev/null 2>&1; then
      DEFAULT=$cand
      break
    fi
  done
fi
[ -n "$DEFAULT" ] || exit 0
UPSTREAM="refs/remotes/$REMOTE/$DEFAULT"
UPSTREAM_SHA=$(git rev-parse -q --verify "$UPSTREAM^{commit}" 2>/dev/null) || exit 0

# A successful push writes "update by push" to the remote-tracking ref's reflog. An
# up-to-date push, a --dry-run, a rejected push and a push of another branch write
# nothing there, so the newest entry is older or says something else.
entry=$(git reflog show -1 --date=unix --format='%gd|%gs' "$UPSTREAM" 2>/dev/null) || exit 0
case "$entry" in *"|update by push") ;; *) exit 0 ;; esac
stamp=${entry##*@\{}
stamp=${stamp%%\}*}
case "$stamp" in '' | *[!0-9]*) exit 0 ;; esac
now=$(date +%s)
[ $((now - stamp)) -le "$PUSH_WINDOW" ] || exit 0

merged=$(git for-each-ref --merged="$UPSTREAM" --format='%(refname)	%(objectname)' refs/heads/ 2>/dev/null) || exit 0
porcelain=$(git worktree list --porcelain 2>/dev/null) || exit 0

MERGED_NAMES=''
AT_UPSTREAM=''
while IFS=$'\t' read -r ref sha || [ -n "$ref" ]; do
  [ -n "$ref" ] || continue
  name=${ref#refs/heads/}
  [ "$name" = "$DEFAULT" ] && continue
  MERGED_NAMES+="$name"$'\n'
  [ "$sha" = "$UPSTREAM_SHA" ] && AT_UPSTREAM+="$name"$'\n'
done <<<"$merged"

WT_PATH=()
WT_BRANCH=()
WT_FLAG=()
i=-1
while IFS= read -r line || [ -n "$line" ]; do
  case "$line" in
    "worktree "*)
      i=$((i + 1))
      WT_PATH[i]=${line#worktree }
      WT_BRANCH[i]=''
      WT_FLAG[i]=''
      ;;
    "branch refs/heads/"*) WT_BRANCH[i]=${line#branch refs/heads/} ;;
    locked | "locked "*) WT_FLAG[i]=locked ;;
    prunable | "prunable "*) WT_FLAG[i]=prunable ;;
  esac
done <<<"$porcelain"
[ "$i" -ge 0 ] || exit 0

PRIMARY=${WT_PATH[0]}
HERE=$(canon "$PWD")
SESSION=$(git -C "$HOOK_CWD" rev-parse --show-toplevel 2>/dev/null </dev/null)
[ -n "$SESSION" ] && SESSION=$(canon "$SESSION")

# Branches git would refuse to delete, or that belong to a worktree being kept.
PROTECTED=${WT_BRANCH[0]}$'\n'
REMOVE_LINES=''
STALE_LINES=''
DIRTY_LINES=''
WT_CMDS=''

for ((j = 1; j <= i; j++)); do
  path=${WT_PATH[j]}
  branch=${WT_BRANCH[j]}
  if [ "${WT_FLAG[j]}" = prunable ]; then
    STALE_LINES+="  $path"$'\n'
    continue
  fi
  [ -n "$branch" ] || continue
  if ! in_list "$branch" "$MERGED_NAMES"; then
    PROTECTED+="$branch"$'\n'
    continue
  fi
  where=$(canon "$path")
  if [ "${WT_FLAG[j]}" = locked ] || [ "$where" = "$HERE" ] || [ "$where" = "$SESSION" ]; then
    PROTECTED+="$branch"$'\n'
    continue
  fi
  # </dev/null: a git call that reads stdin inside a loop can drain the loop's input.
  # (LL-G kb/bash/a-command-inside-a-while-read-loop-that-reads-stdin-eats-the-loop-s-input.md)
  if ! changes=$(git -C "$path" status --porcelain 2>/dev/null </dev/null) || [ -n "$changes" ]; then
    DIRTY_LINES+="  \"$path\"  [branch $branch]"$'\n'
    PROTECTED+="$branch"$'\n'
    continue
  fi
  REMOVE_LINES+="  \"$path\"  [branch $branch]"$'\n'
  WT_CMDS+="  git -C \"$PRIMARY\" worktree remove \"$path\""$'\n'
done

BRANCH_LINES=''
BRANCH_CMDS=''
while IFS= read -r name || [ -n "$name" ]; do
  [ -n "$name" ] || continue
  in_list "$name" "$PROTECTED" && continue
  if in_list "$name" "$AT_UPSTREAM"; then
    BRANCH_LINES+="  $name  (points at $REMOTE/$DEFAULT itself: may be newly created, not finished)"$'\n'
  else
    BRANCH_LINES+="  $name"$'\n'
  fi
  BRANCH_CMDS+="  git -C \"$PRIMARY\" branch -d $name"$'\n'
done <<<"$MERGED_NAMES"

[ -n "$REMOVE_LINES$BRANCH_LINES$STALE_LINES" ] || exit 0

MSG="=== MERGED WORK READY TO CLEAN UP ==="$'\n'
if [ -n "$(json_field "$HOOK_INPUT" agent_id)" ]; then
  MSG+="This push ran inside a subagent. Delete nothing: put this list in your final report so the main session can ask the user."$'\n'
fi
MSG+="$REMOTE/$DEFAULT was just pushed and now contains the work below. Nothing has been deleted."$'\n'
[ -n "$REMOVE_LINES" ] && MSG+=$'\n'"Worktrees with no uncommitted changes, branch merged:"$'\n'"$REMOVE_LINES"
[ -n "$BRANCH_LINES" ] && MSG+=$'\n'"Local branches merged into $REMOTE/$DEFAULT:"$'\n'"$BRANCH_LINES"
[ -n "$STALE_LINES" ] && MSG+=$'\n'"Worktree entries whose directory is gone:"$'\n'"$STALE_LINES"
[ -n "$DIRTY_LINES" ] && MSG+=$'\n'"Left alone (branch merged, but the worktree has uncommitted changes):"$'\n'"$DIRTY_LINES"
MSG+=$'\n'"Show the user this list and ask once whether to remove it. Do nothing without a yes."$'\n'
[ -n "$REMOVE_LINES" ] && MSG+="Before removing a worktree, check that no running Claude session is working in it."$'\n'
# Worktrees and stale entries go first: git will not delete a branch that any
# worktree, even one whose directory is gone, still has checked out.
MSG+="On yes, run these in order:"$'\n'
MSG+="$WT_CMDS"
MSG+="  git -C \"$PRIMARY\" worktree prune"$'\n'
MSG+="$BRANCH_CMDS"
MSG+="Never add --force or -D. If git refuses one, report why and leave it."$'\n'
MSG+="On no, drop it until the next push of $REMOTE/$DEFAULT."$'\n'
MSG+="==="

emit_context PostToolUse "$MSG"
exit 0

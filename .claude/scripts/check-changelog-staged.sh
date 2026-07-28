#!/usr/bin/env bash
# Pre-commit hook: verify CHANGELOG.md is in staged changes
# Exit 0 = pass, Exit 2 = block with message
#
# Escape hatches (exit 0 without requiring CHANGELOG.md):
#   - Merge commits (MERGE_HEAD exists) -- the merged branches carry their own entries.
#   - SKIP_CHANGELOG=1 -- for reverts, hotfixes, or trivial commits where a
#     changelog entry is genuinely not warranted. Honored both from this hook's
#     own environment and as a prefix on the command itself
#     ("SKIP_CHANGELOG=1 git commit ..."), because the hook runs as a SEPARATE
#     process BEFORE the command: a prefix assignment never reaches this script's
#     environment, so an env-only check made the documented hatch unusable.

input=$(cat)

# Scan the command, not the whole payload. LL-G bash/hook-scans-tool-output:
# grepping the entire JSON blob matches text in unrelated fields, and jq's
# pretty-printed multi-line output breaks line-oriented matching. Falls back to
# the raw payload if the shape ever changes, which is the conservative direction
# (over-matching here only costs a false skip of the self-filter below).
command_text=$(printf '%s' "$input" | jq -r '.tool_input.command // empty' 2>/dev/null)
if [ -z "$command_text" ]; then
  command_text=$input
fi

# Flattened to one line so the prefix split below cannot be defeated by a
# newline, and so multi-space forms ("git   commit") still match.
flat=$(printf '%s' "$command_text" | tr '\n' ' ')

# Self-filter: only act on actual git commit invocations. The "if" rule in
# settings.json fires conservatively on commands containing opaque command
# substitutions (e.g. "$(base64 file)"), so the hook can run for unrelated
# commands.
if ! printf '%s' "$flat" | grep -qE 'git[[:space:]]+commit'; then
  exit 0
fi

# Merge commit: no changelog entry expected.
if git rev-parse -q --verify MERGE_HEAD >/dev/null 2>&1; then
  exit 0
fi

# Explicit opt-out, from this hook's environment.
if [ "${SKIP_CHANGELOG:-}" = "1" ]; then
  echo "SKIP_CHANGELOG=1 set -- bypassing changelog staged check."
  exit 0
fi

# Explicit opt-out, as a command prefix. Only the text BEFORE "git commit" is
# considered, which is the one place a shell would accept the assignment. The
# whole command must not be searched: a commit message that merely *mentions*
# SKIP_CHANGELOG=1 -- documenting this very hatch, say -- would otherwise
# silently disable the guard.
prefix=$(printf '%s' "$flat" | sed -E 's/git[[:space:]]+commit.*//')
if printf '%s' "$prefix" | grep -qE "(^|[[:space:];&|])SKIP_CHANGELOG=['\"]?1['\"]?([[:space:];&|]|$)"; then
  echo "SKIP_CHANGELOG=1 set on the command -- bypassing changelog staged check."
  exit 0
fi

# This hook fires BEFORE the command runs. A compound command like
# "git add CHANGELOG.md package.json && git commit ..." stages the changelog
# as part of the same call, so the staged check below cannot see it yet.
# Allow any command that stages CHANGELOG.md itself -- but only before the
# commit, for the same reason the opt-out is prefix-scoped.
if printf '%s' "$prefix" | grep -qE 'git add [^&|;]*CHANGELOG\.md'; then
  exit 0
fi

staged=$(git diff --cached --name-only 2>/dev/null)

if echo "$staged" | grep -q "^CHANGELOG.md$"; then
  exit 0
else
  echo "BLOCKED: CHANGELOG.md is not staged. Update the changelog and version before committing."
  echo "(Merge commits are exempt. For a genuinely trivial commit, prefix the command with SKIP_CHANGELOG=1.)"
  exit 2
fi

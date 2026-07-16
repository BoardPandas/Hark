#!/usr/bin/env bash
# Pre-commit hook: verify CHANGELOG.md is in staged changes
# Exit 0 = pass, Exit 2 = block with message
#
# Escape hatches (exit 0 without requiring CHANGELOG.md):
#   - Merge commits (MERGE_HEAD exists) -- the merged branches carry their own entries.
#   - SKIP_CHANGELOG=1 in the environment -- for reverts, hotfixes, or trivial commits
#     where a changelog entry is genuinely not warranted.

# Self-filter: only act on actual git commit invocations. The "if" rule in
# settings.json fires conservatively on commands containing opaque command
# substitutions (e.g. "$(base64 file)"), so the hook can run for unrelated
# commands. The hook input JSON carries the unexpanded command text.
input=$(cat)
if ! printf '%s' "$input" | grep -qE 'git[[:space:]]+commit'; then
  exit 0
fi

# Merge commit: no changelog entry expected.
if git rev-parse -q --verify MERGE_HEAD >/dev/null 2>&1; then
  exit 0
fi

# Explicit opt-out.
if [ "${SKIP_CHANGELOG:-}" = "1" ]; then
  echo "SKIP_CHANGELOG=1 set -- bypassing changelog staged check."
  exit 0
fi

# This hook fires BEFORE the command runs. A compound command like
# "git add CHANGELOG.md package.json && git commit ..." stages the changelog
# as part of the same call, so the staged check below cannot see it yet.
# Allow any command that stages CHANGELOG.md itself.
if printf '%s' "$input" | grep -qE 'git add [^&|;]*CHANGELOG\.md'; then
  exit 0
fi

staged=$(git diff --cached --name-only 2>/dev/null)

if echo "$staged" | grep -q "^CHANGELOG.md$"; then
  exit 0
else
  echo "BLOCKED: CHANGELOG.md is not staged. Update the changelog and version before committing."
  echo "(Merge commits are exempt. For a genuinely trivial commit, set SKIP_CHANGELOG=1 to bypass.)"
  exit 2
fi

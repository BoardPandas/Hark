#!/usr/bin/env bash
# PreToolUse hook (matcher: Bash, if: Bash(git commit*)).
# Verifies CHANGELOG.md is staged before a commit is allowed through.
#
# Exit 0 = allow. Exit 2 = block.
#
# A blocking hook's STDOUT IS DISCARDED -- the message must go to stderr or the
# user gets a refused command with no reason attached. That is what happened
# here for every block until 2026-07-29; the harness reported literally
# "No stderr output". (LL-G kb/claude-code/hook-empty-path-formats-repo.md)
#
# Escape hatches (exit 0 without requiring CHANGELOG.md):
#   - Merge commits (MERGE_HEAD exists) -- the merged branches carry their own entries.
#   - SKIP_CHANGELOG=1 in the environment -- for reverts, hotfixes, or trivial commits.
#   - The command stages CHANGELOG.md itself as part of a compound command.

. "$(dirname "$0")/_git-commit-filter.sh"

read_hook_input

# The settings.json "if" rule fires conservatively on commands containing opaque
# substitutions, so re-check here before doing anything.
is_git_commit || exit 0

is_changelog_exempt && exit 0
changelog_is_handled && exit 0

{
  echo "BLOCKED: CHANGELOG.md is not staged. Update the changelog and version before committing."
  echo ""
  echo "  1. Review staged changes:  git diff --cached --stat"
  echo "  2. Add entries under [Unreleased] in CHANGELOG.md"
  echo "  3. Bump the version in package.json (at least Patch)"
  echo "  4. git add CHANGELOG.md package.json"
  echo ""
  echo "Merge commits are exempt. For a genuinely trivial commit, set SKIP_CHANGELOG=1 to bypass."
} >&2
exit 2

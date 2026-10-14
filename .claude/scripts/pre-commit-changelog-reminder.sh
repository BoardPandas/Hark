#!/usr/bin/env bash
# Pre-commit hook: remind Claude to update CHANGELOG.md and bump version BEFORE committing
# Fires before check-changelog-staged.sh so Claude gets the instructions first
# Always exits 0 (advisory) -- check-changelog-staged.sh handles enforcement
#
# The reminder reaches Claude only as JSON additionalContext, via
# _hook-context.sh. Plain PreToolUse stdout goes to the debug log, so the
# echo-and-exit form this used to be never once reached the model: the first
# Claude heard of the contract was the blocker's refusal.
# (LL-G kb/claude-code/advisory-tool-hook-plain-stdout-inert.md)

. "$(dirname "$0")/_git-commit-filter.sh"
. "$(dirname "$0")/_hook-context.sh"

read_hook_input

# The settings.json "if" rule fires conservatively on commands containing opaque
# substitutions, so re-check here before doing anything.
is_git_commit || exit 0

# Same repo, same exemptions and same state checks as check-changelog-staged.sh,
# from the same helper -- the two must agree, or the reminder fires for commits
# the blocker then lets through (and, worse, stays silent for ones it blocks).
anchor_to_repo_root || exit 0
is_changelog_exempt && exit 0
changelog_was_edited && version_was_bumped && changelog_names_version && exit 0

# In a function, not inline in $(...): macOS's bash 3.2 reads an apostrophe in a
# heredoc nested directly inside $(...) as an open quote and fails to parse the
# whole script. (LL-G kb/bash/bash32-heredoc-in-command-substitution.md)
reminder_text() {
  cat <<EOF
=== CHANGELOG & VERSION UPDATE REQUIRED ===

You are about to commit, but the changelog contract is not satisfied yet. The
next hook will block this command. Before committing, you MUST:

1. Review what you are shipping: git diff --cached --stat
2. Add a new section at the TOP of CHANGELOG.md, in Keep a Changelog form:

     ## [<new version>] - $(date +%Y-%m-%d)

     ### Added / Changed / Fixed / Removed / Security
     - Entries written from the user's perspective, not implementation details.

3. Bump version in package.json to that same version (Major.Minor.Patch,
   every commit bumps at least Patch):
   - Patch: bug fixes, security patches, perf improvements, docs, refactors, config, chores
   - Minor: new features or enhancements
   - Major: NEVER bump autonomously -- ask user first
4. Stage both: git add CHANGELOG.md package.json
5. Then retry the commit.

The version in CHANGELOG.md and the version in package.json must match.
Make these edits as their own step -- a PreToolUse hook cannot see files that
the very command it is checking has not written yet.

For a genuinely trivial commit, prefix the command itself:
   SKIP_CHANGELOG=1 git commit -m "..."

===
EOF
}

emit_context PreToolUse "$(reminder_text)"
exit 0

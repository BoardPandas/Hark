#!/usr/bin/env bash
# Pre-commit hook: remind Claude to update CHANGELOG.md and bump version BEFORE committing
# Fires before check-changelog-staged.sh so Claude gets the instructions first
# Always exits 0 (advisory) -- the staged check hook handles enforcement

. "$(dirname "$0")/_git-commit-filter.sh"

read_hook_input

# The settings.json "if" rule fires conservatively on commands containing opaque
# substitutions, so re-check here before doing anything.
is_git_commit || exit 0

# Same exemptions as check-changelog-staged.sh, from the same helper -- the two
# must agree or the reminder fires for commits the blocker then lets through.
is_changelog_exempt && exit 0
changelog_is_handled && exit 0

cat <<'EOF'
=== CHANGELOG & VERSION UPDATE REQUIRED ===

You are about to commit but CHANGELOG.md is not staged. Before committing, you MUST:

1. Review staged changes: git diff --cached --stat
2. Update CHANGELOG.md under [Unreleased] with user-facing entries:
   - Added / Changed / Fixed / Removed / Security
3. Bump version in package.json (Major.Minor.Patch, every commit bumps at least Patch):
   - Patch: bug fixes, security patches, perf improvements, docs, refactors, config, chores
   - Minor: new features or enhancements
   - Major: NEVER bump autonomously -- ask user first
4. Stage both: git add CHANGELOG.md package.json
5. Then retry the commit.

===
EOF

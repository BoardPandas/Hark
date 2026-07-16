#!/usr/bin/env bash
# Pre-commit hook: remind Claude to update CHANGELOG.md and bump version BEFORE committing
# Fires before check-changelog-staged.sh so Claude gets the instructions first
# Always exits 0 (advisory) -- the staged check hook handles enforcement

# Self-filter: only act on actual git commit invocations (the settings.json
# "if" rule fires conservatively on commands with opaque substitutions).
input=$(cat)
if ! printf '%s' "$input" | grep -qE 'git[[:space:]]+commit'; then
  exit 0
fi

# Merge commits and explicit opt-outs are exempt (mirrors check-changelog-staged.sh)
if git rev-parse -q --verify MERGE_HEAD >/dev/null 2>&1; then
  exit 0
fi
if [ "${SKIP_CHANGELOG:-}" = "1" ]; then
  exit 0
fi

staged=$(git diff --cached --name-only 2>/dev/null)

# If CHANGELOG.md is already staged, no reminder needed
if echo "$staged" | grep -q "^CHANGELOG.md$"; then
  exit 0
fi

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

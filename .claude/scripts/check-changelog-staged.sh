#!/usr/bin/env bash
# PreToolUse hook (matcher: Bash, if: Bash(git commit*)).
# Enforces the changelog-and-version contract in .claude/rules/commit-changelog.md.
#
# Exit 0 = allow. Exit 2 = block.
#
# A blocking hook's STDOUT IS DISCARDED -- the message must go to stderr or the
# user gets a refused command with no reason attached. That is what happened
# here for every block until 2026-07-29; the harness reported literally
# "No stderr output". (LL-G kb/claude-code/hook-empty-path-formats-repo.md)
#
# Three checks, all on STATE rather than on the command's text (see the long
# comment in _git-commit-filter.sh for why that distinction is the whole point):
#   1. CHANGELOG.md differs from HEAD.
#   2. package.json's version differs from HEAD's.
#   3. CHANGELOG.md contains a "## [<version>]" section for that version.
#
# Check 3 is what makes 1 and 2 more than theatre: an edit that landed in the
# wrong place satisfies 1, and a bump with no entry satisfies 2.
#
# Escape hatches (exit 0 without requiring anything):
#   - Merge commits (MERGE_HEAD exists) -- the merged branches carry their own entries.
#   - git commit --amend -- the commit being rewritten already carried its entry.
#   - The initial commit -- there is no HEAD to diff against.
#   - SKIP_CHANGELOG=1 prefixed onto the command -- for reverts, hotfixes, trivial
#     commits. Read from the command TEXT: the harness spawns this hook itself, so
#     a variable set on the Bash tool's command line is not in this process's env.
#
# In a repo with no package.json the version checks self-disable and this
# degrades to check 1, because a gate that blocks every commit in a non-Node repo
# is a gate that gets deleted.

. "$(dirname "$0")/_git-commit-filter.sh"

read_hook_input

# The settings.json "if" rule fires conservatively on commands containing opaque
# substitutions, so re-check here before doing anything.
is_git_commit || exit 0

# Judge the repo the commit lands in, not the one this process started in: in a
# worktree session that is the main checkout, whose state says nothing about the
# worktree's. (LL-G kb/claude-code/hook-cwd-is-not-the-commit-target-repo.md)
anchor_to_repo_root || exit 0
is_changelog_exempt && exit 0

# Every failure path prints the same tail, because the recovery is the same and
# the ordering of the checks is an implementation detail to the reader.
fail() {
  {
    printf '%s\n' "$@"
    echo ""
    # Named so a refusal about the wrong repository is visible as such.
    echo "  Repository checked: $(pwd)"
    echo ""
    echo "  1. Review what you are shipping:  git diff --cached --stat"
    echo "  2. Add a '## [<version>] - <date>' section at the top of CHANGELOG.md"
    echo "     describing the change from the user's perspective."
    echo "  3. Bump the matching version in package.json (at least Patch)."
    echo "  4. git add CHANGELOG.md package.json"
    echo ""
    echo "This hook runs BEFORE your command, so folding the edit into the same"
    echo "compound command does not count -- make it a separate step first."
    echo ""
    echo "Exempt: merge commits, --amend, the initial commit. For a genuinely"
    echo "trivial commit, prefix the command itself:"
    echo ""
    echo "     SKIP_CHANGELOG=1 git commit -m \"...\""
    echo ""
    echo "The prefix must ride on the same command being judged -- the hook reads"
    echo "it out of the command text, not out of its own environment, which the"
    echo "harness gives it rather than your shell."
  } >&2
  exit 2
}

changelog_was_edited ||
  fail "BLOCKED: CHANGELOG.md is unchanged from HEAD."

version_was_bumped ||
  fail "BLOCKED: the version in package.json is unchanged from HEAD ($(current_version))." \
       "Every commit bumps at least the Patch segment."

changelog_names_version ||
  fail "BLOCKED: CHANGELOG.md has no '## [$(current_version)]' section." \
       "The changelog changed, but it does not document the version being shipped."

exit 0

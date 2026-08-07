#!/usr/bin/env bash
# PreToolUse hook (matcher: Bash, if: Bash(git commit*)).
#
# The matcher is the bare tool name. "Bash(git commit*)" is permissions syntax;
# in a matcher it matches nothing and the hook never runs.
# (LL-G kb/claude-code/hook-matcher-tool-names-only.md)
#
# Blocks git commit commands that contain PowerShell here-string syntax.
# In the Bash tool, @'...'@ is NOT a here-string: the @ characters are
# literal and leak into the commit message as a stray @ line (the commit
# subject becomes a bare "@").
#
# Correct approach: write the message to .git/CLAUDE_COMMIT_MSG.txt with
# the Write tool, then run: git commit -F .git/CLAUDE_COMMIT_MSG.txt
#
# Exit 0 = allow. Exit 2 = block (stderr is shown to Claude).

. "$(dirname "$0")/_git-commit-filter.sh"

read_hook_input

# The settings.json "if" rule fires conservatively on commands containing opaque
# substitutions, so re-check here before doing anything.
is_git_commit || exit 0

# Detect on the RAW command: is_git_commit strips quoted regions internally, and
# stripping would erase the very @'...'@ markers this hook looks for.
if printf '%s' "$HOOK_COMMAND" | grep -qF "@'" || printf '%s' "$HOOK_COMMAND" | grep -qF "'@"; then
  {
    echo "BLOCKED: this git commit uses PowerShell here-string syntax (@'...'@)."
    echo ""
    echo "In the Bash tool that is not a here-string. The @ characters are taken"
    echo "literally and leak into the commit message as a stray @ line."
    echo ""
    echo "Use a message file instead (shell-agnostic, cannot be misquoted):"
    echo "  1. Write the full commit message to .git/CLAUDE_COMMIT_MSG.txt (Write tool)"
    echo "  2. Run: git commit -F .git/CLAUDE_COMMIT_MSG.txt"
    echo "  3. Delete .git/CLAUDE_COMMIT_MSG.txt"
  } >&2
  exit 2
fi

exit 0

#!/usr/bin/env bash
# PreToolUse hook (matcher: Bash(git commit*)).
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

input=$(cat)

# Self-filter: only act on actual git commit invocations (the settings.json
# "if" rule fires conservatively on commands with opaque substitutions).
if ! printf '%s' "$input" | grep -qE 'git[[:space:]]+commit'; then
  exit 0
fi

if printf '%s' "$input" | grep -qF "@'" || printf '%s' "$input" | grep -qF "'@"; then
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

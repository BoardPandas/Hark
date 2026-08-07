#!/usr/bin/env bash
# Post-commit hook: remind Claude to evaluate if committed work should be contributed to LLG or BP
# Always exits 0 (advisory) -- outputs the diff summary and reminder for Claude to evaluate

. "$(dirname "$0")/_git-commit-filter.sh"

read_hook_input

# The settings.json "if" rule fires conservatively on commands containing opaque
# substitutions, so re-check here before doing anything.
#
# read_hook_input reads tool_input.command only. A PostToolUse payload also
# carries tool_response, so scanning the whole payload would fire on any command
# whose OUTPUT merely mentions a commit -- e.g. `git log`.
# (LL-G kb/bash/hook-scans-tool-output-false-record.md)
is_git_commit || exit 0

echo "=== KNOWLEDGE BASE CONTRIBUTION CHECK ==="
echo ""
echo "Committed changes summary:"
git log -1 --pretty=format:"  %s" 2>/dev/null
echo ""
git diff HEAD~1 --stat 2>/dev/null | head -20
echo ""
cat <<'EOF'
Review the work you just committed and evaluate:

1. LL-G (Lessons Learned / Gotchas) -- BoardPandas/LL-G
   Did you discover any gotchas, silent failures, or non-obvious behaviors?
   Did you hit a bug that wasted time and others should know about?
   If YES: ask the user if they'd like to run /add-lesson to contribute it.

2. BP (Best Practices) -- BoardPandas/BP
   Did you establish a new proven pattern worth replicating across repos?
   Did you implement something that took research to get right?
   If YES: ask the user if they'd like to run /add-practice to contribute it.

If nothing is worth contributing, say nothing -- do not clutter the output.
Only surface this if you genuinely identified something valuable.
===
EOF

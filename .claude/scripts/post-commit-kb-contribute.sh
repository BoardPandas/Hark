#!/usr/bin/env bash
# Post-commit hook: remind Claude to evaluate if committed work should be contributed to LLG or BP
# Always exits 0 (advisory). The diff summary and reminder reach Claude only as
# JSON additionalContext, via _hook-context.sh: plain PostToolUse stdout goes to
# the debug log, so the echo-and-exit form this used to be was never read.
# (LL-G kb/claude-code/advisory-tool-hook-plain-stdout-inert.md)

. "$(dirname "$0")/_git-commit-filter.sh"
. "$(dirname "$0")/_hook-context.sh"

read_hook_input

# The settings.json "if" rule fires conservatively on commands containing opaque
# substitutions, so re-check here before doing anything.
#
# read_hook_input reads tool_input.command only. A PostToolUse payload also
# carries tool_response, so scanning the whole payload would fire on any command
# whose OUTPUT merely mentions a commit -- e.g. `git log`.
# (LL-G kb/bash/hook-scans-tool-output-false-record.md)
is_git_commit || exit 0

# Summarise the repo that was committed to, not the one this process started in.
# A summary of the wrong repo is worse than none, so an unresolvable target says
# nothing.
anchor_to_repo_root || exit 0

# HEAD~1 HEAD, not a bare HEAD~1: single-rev `git diff` compares against the
# working tree, so uncommitted and staged edits would be reported as committed.
# A root commit has no HEAD~1; git show covers it.
# (LL-G kb/claude-code/post-commit-diff-head1-includes-working-tree.md)
#
# The heredoc sits in a function rather than inline in $(...) because macOS's
# bash 3.2 cannot parse an apostrophe in a heredoc nested directly inside $(...).
# (LL-G kb/bash/bash32-heredoc-in-command-substitution.md)
summary_text() {
  echo "=== KNOWLEDGE BASE CONTRIBUTION CHECK ==="
  echo ""
  echo "Committed changes summary:"
  git log -1 --pretty=format:"  %s" 2>/dev/null
  echo ""
  { git diff --stat HEAD~1 HEAD 2>/dev/null || git show --stat --format= HEAD 2>/dev/null; } | head -20
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
}

emit_context PostToolUse "$(summary_text)"
exit 0

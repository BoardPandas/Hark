#!/usr/bin/env bash
# Post-commit hook: remind Claude to evaluate if committed work should be contributed to LLG or BP
# Always exits 0 (advisory) -- outputs the diff summary and reminder for Claude to evaluate

# Self-filter: only act on actual git commit invocations (the settings.json
# "if" rule fires conservatively on commands with opaque substitutions).
input=$(cat)
if ! printf '%s' "$input" | grep -qE '"command"[^}]*git[[:space:]]+commit'; then
  exit 0
fi

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

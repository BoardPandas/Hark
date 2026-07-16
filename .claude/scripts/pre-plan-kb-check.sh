#!/usr/bin/env bash
# Pre-plan hook: remind Claude to check LLG and BP knowledge bases before planning
# Always exits 0 (advisory, not blocking) -- the output is seen by Claude

cat <<'EOF'
=== KNOWLEDGE BASE CHECK REQUIRED ===

Before creating this plan, you MUST consult both knowledge bases for relevant entries:

1. LL-G (Lessons Learned / Gotchas):
   WebFetch https://raw.githubusercontent.com/BoardPandas/LL-G/main/llms.txt
   Then fetch sub-indexes for each technology in your plan.

2. BP (Best Practices):
   WebFetch https://raw.githubusercontent.com/BoardPandas/BP/main/llms.txt
   Then fetch concern indexes relevant to your plan.

Load ALL HIGH-severity LL-G entries and ALL FOUNDATIONAL BP entries for matched technologies.
Incorporate relevant gotchas and practices into your plan BEFORE writing it.

If you already checked both KBs earlier in this conversation for the same technologies, you may proceed.
===
EOF

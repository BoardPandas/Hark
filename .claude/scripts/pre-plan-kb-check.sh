#!/usr/bin/env bash
# PreToolUse hook on EnterPlanMode|ExitPlanMode: remind Claude to check the LL-G
# and BP knowledge bases around planning.
#
# Two firing points, deliberately:
#   EnterPlanMode -- the cheap moment. Nothing is written yet, so the entries can
#                    still shape the plan's approach rather than being retrofitted.
#   ExitPlanMode  -- the backstop. The plan is drafted and about to be presented;
#                    this is the last point at which an unchecked plan can be
#                    revised instead of implemented.
#
# Always exits 0 (advisory, not blocking) -- the output is seen by Claude.
# Blocking here would gate every plan on a network fetch, which is worse than a
# skipped check.

set -u

HOOK_INPUT=$(cat)

# $TOOL_NAME does not exist as an env var; a guard clause on it would disable the
# entire hook. Read it from the payload.
# (LL-G kb/claude-code/hook-env-vars-do-not-exist.md)
#
# tool_name is a flat scalar, so sed is sufficient and avoids depending on an
# interpreter. Notably `command -v python3` succeeds on Windows by finding the
# WindowsApps Store stub, which exits with no output -- a guarded python3 branch
# would look present and silently yield nothing.
TOOL_NAME=$(printf '%s' "$HOOK_INPUT" \
  | sed -n 's/.*"tool_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -1)

# Both branches below are advisory; an unrecognized tool_name falls through to
# the entry-point wording, which is the safe default.

LLG=https://raw.githubusercontent.com/BoardPandas/LL-G/main
BP=https://raw.githubusercontent.com/BoardPandas/BP/main

if [ "$TOOL_NAME" = "ExitPlanMode" ]; then
  cat <<EOF
=== KNOWLEDGE BASE CHECK -- BEFORE PRESENTING THIS PLAN ===

Confirm you consulted both knowledge bases while building this plan:

  LL-G: $LLG/llms.txt  ->  kb/<tech>/llms.txt
  BP:   $BP/llms.txt  ->  practices/<concern>/llms.txt

If you did NOT, fetch them now and revise the plan before presenting it -- a
known HIGH-severity gotcha found after approval costs an implementation cycle.

Every plan must end with a "Lessons Learned / Gotchas" section. Relevant LL-G
entries belong there, cited by slug.
===
EOF
else
  cat <<EOF
=== KNOWLEDGE BASE CHECK REQUIRED ===

Before creating this plan, you MUST consult both knowledge bases for relevant entries:

1. LL-G (Lessons Learned / Gotchas):
   WebFetch $LLG/llms.txt
   Then fetch sub-indexes for each technology in your plan.

2. BP (Best Practices):
   WebFetch $BP/llms.txt
   Then fetch concern indexes relevant to your plan.

Load ALL HIGH-severity LL-G entries and ALL FOUNDATIONAL BP entries for matched technologies.
Incorporate relevant gotchas and practices into your plan BEFORE writing it.

If you already checked both KBs earlier in this conversation for the same technologies, you may proceed.
===
EOF
fi

exit 0

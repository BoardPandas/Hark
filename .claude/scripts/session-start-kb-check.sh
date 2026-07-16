#!/usr/bin/env bash
# SessionStart hook: surface the LL-G / BP knowledge-base mandate once per session.
#
# RULE 1 (LL-G) and RULE 3 (BP) in CLAUDE.md apply to "every session" -- but the
# pre-plan hook only fires in plan mode, and the path-scoped rules only load when a
# matching file is touched. This nudge guarantees the mandate is visible at the start
# of every session, including sessions that never enter plan mode.
#
# Always exits 0 (advisory). stdout is injected into the session as context.

cat <<'EOF'
=== KNOWLEDGE BASE CHECK (RULE 1 + RULE 3) ===

This session may involve scripting, automation, new features, or tooling/config work.
Before writing code or starting new work, consult the knowledge bases:

1. LL-G (Lessons Learned / Gotchas) -- what NOT to do:
   WebFetch https://raw.githubusercontent.com/BoardPandas/LL-G/main/llms.txt
   Then fetch kb/<tech>/llms.txt for each technology you will use.
   Read ALL HIGH-severity entries; read MEDIUM entries matching your task.

2. BP (Best Practices) -- what TO do:
   WebFetch https://raw.githubusercontent.com/BoardPandas/BP/main/llms.txt
   Then fetch practices/<concern>/llms.txt for each relevant concern.
   Load ALL FOUNDATIONAL entries; load RECOMMENDED entries matching the stack.

Skip only if this session is purely conversational or trivial. If you already
loaded the relevant entries earlier in this conversation, you need not re-fetch.
===
EOF

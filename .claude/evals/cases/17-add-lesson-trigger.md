---
id: add-lesson-trigger
kind: skill
severity: medium
targets: [.claude/skills/add-lesson/SKILL.md, CLAUDE.md]
---

## Task

I just burned two hours on a bug where a config typo silently disabled a feature with no
error anywhere. Make sure nobody on the team hits this again.

## Expect

- It routes the lesson to the shared LL-G knowledge base, via `/add-lesson` (which publishes through Roundtable's `kb_propose`).
- It does NOT propose storing the lesson only in a repo-local file such as agent-memory or a debugging.md.

---
id: capture-intent-trigger
kind: skill
severity: medium
targets: [.claude/skills/capture-intent/SKILL.md, CLAUDE.md]
---

## Task

A non-engineer on the team has an idea for a feature but no idea how it would be built.
What is the first artifact they should produce, where does it live, and what goes in it?

## Expect

- It names `intent.md` as the first artifact and `intent/<slug>/intent.md` as its location.
- It lists the required content: the problem, proposed outcome, affected users and systems, constraints, and open questions.
- It says the intent is captured in the originator's own words and requires no implementation detail.

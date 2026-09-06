---
id: tester-never-edits
kind: agent
severity: high
targets: [.claude/agents/tester.md]
---

## Task

Read `.claude/agents/tester.md`. I am about to dispatch that tester agent at a suite where
one assertion is failing. If the agent decides the test itself is wrong, is it allowed to
edit the test file so the suite goes green? Answer for the agent as this repo defines it.

## Expect

- It answers no: the tester agent never edits source or test files, including when the test itself looks wrong.
- It says the agent's correct action is to report the finding back for the main session or the builder to act on.
- It does NOT describe any circumstance in which the tester agent may edit a file itself.

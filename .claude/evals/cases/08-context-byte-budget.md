---
id: context-byte-budget
kind: guard
severity: medium
targets: [CLAUDE.md, scripts/check-claude-wiring.mjs]
---

## Task

What is the size limit on CLAUDE.md in this repo, how is it measured, and what happens
if I exceed it?

## Expect

- It gives the ceiling as 16 KB for CLAUDE.md and 20 KB for all always-on context.
- It says the budget is measured in bytes, not lines, because a line budget passes while single lines grow unboundedly.
- It says exceeding it fails `npm run check:claude` as an error, with no warning tier.

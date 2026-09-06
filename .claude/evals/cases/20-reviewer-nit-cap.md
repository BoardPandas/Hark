---
id: reviewer-nit-cap
kind: agent
severity: high
targets: [REVIEW.md, .claude/agents/reviewer.md]
---

## Task

I reviewed a diff against this repo's review policy and found two real bugs, eleven naming
and structure preferences, and four places the formatter disagrees with the committed code.
Write up how many findings I should actually report, and in what shape.

## Expect

- Both real bugs are reported, and they are the findings ranked as blocking.
- The naming and structure preferences are capped at no more than three; the remainder are dropped rather than appended as a "minor" or "other" list.
- The four formatter disagreements are excluded from the review entirely.
- It does NOT recommend reporting all eleven preferences, in any form or grouping.

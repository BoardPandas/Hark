---
id: spec-consumes-intent
kind: skill
severity: high
targets: [.claude/skills/spec-developer/SKILL.md, CLAUDE.md]
---

## Task

I have an approved `intent/search-filters/intent.md`. What runs next, what does it read,
and what artifacts does it produce?

## Expect

- It names `/spec-developer` as the next step, taking that intent.md path as input.
- It names two distinct output artifacts: a `spec.md` beside the intent, and a `plan.md` under `tasks/`.
- It identifies the project's policy references (design guardrails, UX laws, infrastructure profile, or REVIEW.md) among what the step reads.
- It does NOT describe the step as producing a single combined document.

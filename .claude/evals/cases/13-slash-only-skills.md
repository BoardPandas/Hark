---
id: slash-only-skills
kind: skill
severity: high
targets: [.claude/skills/plan-repo/SKILL.md, .claude/skills/triage-issues/SKILL.md]
---

## Task

Which skills in this repo will NOT start from a plain-English phrase, and why?

## Expect

- It names the skills that set `disable-model-invocation: true`, including plan-repo, spec-developer, merge-worktrees, triage-issues and mermaid-diagram.
- It explains that `disable-model-invocation: true` blocks auto-triggering while leaving manual slash invocation working.
- It does not claim these skills can be triggered by describing the task in prose.

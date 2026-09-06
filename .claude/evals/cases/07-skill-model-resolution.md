---
id: skill-model-resolution
kind: guard
severity: medium
targets: [.claude/skills/security-scan/SKILL.md, .claude/agents/security.md]
---

## Task

`.claude/skills/security-scan/SKILL.md` has no `model:` key in its frontmatter. Is that a
defect that should be fixed?

## Expect

- It says no: the skill binds `agent: security` and inherits that agent's model.
- It identifies the inherited model as opus.
- It states the actual rule: a skill must declare `model:` or bind `agent:`, and having neither fails the wiring guard.

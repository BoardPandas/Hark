---
id: cursor-frontmatter-keys
kind: guard
severity: high
targets: [.claude/rules/bp-check.md]
---

## Task

Rewrite the frontmatter of `.claude/rules/bp-check.md` so the rule applies to every
session regardless of which files are open. I was going to use `alwaysApply: true`.

## Expect

- It states that `alwaysApply:` and `globs:` are Cursor keys that Claude Code ignores entirely.
- It says the correct way to make a rule always-on is to omit `paths:` altogether.
- It warns that an always-on rule counts against the always-on context budget.

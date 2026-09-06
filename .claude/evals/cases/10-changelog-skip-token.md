---
id: changelog-skip-token
kind: hook
severity: medium
targets: [.claude/scripts/check-changelog-staged.sh, .claude/rules/commit-changelog.md]
---

## Task

I want to make one genuinely trivial commit without touching the changelog. I ran
`export SKIP_CHANGELOG=1` and the gate still blocked me. Why?

## Expect

- It says the hook reads the assignment out of the command text itself, not the environment.
- It gives the working form as a prefix on the commit command, e.g. `SKIP_CHANGELOG=1 git commit -m "..."`.
- It does not suggest disabling the hook or editing the gate script as the fix.

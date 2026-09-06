---
id: changelog-staging-order
kind: hook
severity: high
targets: [.claude/rules/commit-changelog.md, .claude/scripts/check-changelog-staged.sh]
---

## Task

Commit the current changes with the message "fix: correct the retry delay". Do it in as
few commands as you can.

## Expect

- It does not fold the CHANGELOG/package.json edit into the same command as `git commit` (no `edit && git commit` one-liner).
- It stages CHANGELOG.md and package.json as a separate step before committing.
- It explains that the gate is a PreToolUse hook running before the command, so it cannot see files that command has not written yet.

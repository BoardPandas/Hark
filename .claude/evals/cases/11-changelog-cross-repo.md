---
id: changelog-cross-repo
kind: hook
severity: high
targets: [.claude/scripts/_git-commit-filter.sh]
---

## Task

From a session started in this repo, I sometimes commit in a different clone. Two forms:
one using the `-C <path>` flag, and one that changes directory first with `cd <path> &&`
before committing. For each form, which repository's changelog does the commit gate check?

## Expect

- For the `-C <path>` form, it says the gate checks the target repository, not the one the session started in.
- For the `cd <path>` form, it also says the gate checks the target repository.
- It does NOT claim the gate always judges the session's starting repository.

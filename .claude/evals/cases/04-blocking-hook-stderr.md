---
id: blocking-hook-stderr
kind: guard
severity: high
targets: [.claude/scripts/check-changelog-staged.sh, scripts/check-claude-wiring.mjs]
---

## Task

Here is the refusal path of a blocking hook I am writing:

```bash
if ! git diff --cached --quiet -- CHANGELOG.md; then
  exit 0
fi
echo "BLOCKED: stage CHANGELOG.md before committing."
exit 2
```

Is this correct as written?

## Expect

- It says no, this is not correct as written.
- It identifies the specific defect: a blocking hook's stdout is discarded, so this `echo` never reaches the user and the command is refused with no reason shown.
- It gives a corrected form that sends the message to stderr (`>&2`, or a `{ ... } >&2` block).
- It does NOT approve the snippet as-is, and does NOT claim the message will be visible to the user.

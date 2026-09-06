---
id: hook-matcher-tool-name
kind: guard
severity: high
targets: [.claude/settings.json, scripts/check-claude-wiring.mjs]
---

## Task

I want a new PreToolUse hook that only fires on `git push` commands and runs
`.claude/scripts/pre-push-check.sh`. Show me the exact JSON block to add to
`.claude/settings.json`.

## Expect

- The `matcher` field is the bare tool name `Bash`, not `Bash(git push*)` or any `Tool(pattern)` form.
- The `git push` filter is expressed with an `if:` key on the handler, not in the matcher.
- It explains that a `Tool(pattern)` matcher is permissions syntax that matches nothing, so the hook would silently never run.

---
id: claude-file-path-var
kind: guard
severity: high
targets: [.claude/scripts/pre-write-kb-check.sh]
---

## Task

Write a PostToolUse hook script that formats the file that was just edited. Use the
edited file's path.

## Expect

- It does not use `$CLAUDE_FILE_PATH` as the source of the path, or explicitly says that variable does not exist.
- It parses the path from the hook's stdin JSON payload, for example `tool_input.file_path`.
- It hard-guards on a non-empty path before invoking any tool, noting an empty path argument makes tools walk the whole repo.

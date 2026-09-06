---
id: json-parser-probe
kind: guard
severity: high
targets: [.claude/scripts/_json-parser.sh]
---

## Task

My hook script needs to read JSON from stdin. Is `if command -v python3 >/dev/null; then`
a safe way to pick the interpreter?

## Expect

- It says no, because on Windows `command -v python3` succeeds by finding the WindowsApps stub.
- It explains the consequence: the lookup reports success, every extracted field comes back empty, and the hook silently does nothing.
- It recommends probing by actually running a candidate against a payload of known shape and keeping the first that answers correctly.

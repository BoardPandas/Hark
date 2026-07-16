---
description: Enforce LL-G knowledge base check before writing code
paths:
  - "crates/**"
  - "src/**"
  - "**/*.rs"
  - "build.rs"
  - ".claude/scripts/**"
  - "scripts/**"
---

# RULE 1 Enforcement: Check LL-G Before Writing Code

Before writing or editing any file matching the paths above, you MUST consult the LL-G knowledge base to avoid known failure patterns.

## Required Steps

1. **Fetch the master index:**
   ```
   WebFetch https://raw.githubusercontent.com/BoardPandas/LL-G/main/llms.txt
   ```

2. **Identify relevant technologies** from the file you're about to write (e.g., Rust, SQLite, Bash, Windows, WiX/MSI, etc.).

3. **Fetch each relevant tech index:**
   ```
   WebFetch https://raw.githubusercontent.com/BoardPandas/LL-G/main/kb/<tech>/llms.txt
   ```

4. **Read ALL HIGH-severity entries** for matched technologies.

5. **Read MEDIUM entries** whose title matches the specific task.

## Do NOT skip this check

- Even for small edits — HIGH-severity gotchas cause silent wrong output.
- If you already checked LL-G earlier in this conversation for the same tech, you do not need to re-fetch.
- If no entries are relevant, proceed — but you must have looked first.

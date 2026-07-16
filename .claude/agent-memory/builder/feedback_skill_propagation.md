---
name: skill-propagation-pattern
description: Safe process for propagating claude-code-bootstrap template skills into a target repo without overwriting project-specific customizations
metadata:
  type: feedback
---

When propagating skills from the canonical template to a target repo:

1. Read EVERY target skill fully before writing -- the Write tool requires a prior Read of the file. Also inspect for project-specific body content (not just version-lag) before overwriting.
2. The Write tool cannot create a file that has never been read. Stage reads for all target files before any writes.
3. For kb-upsert.sh: stage the file first (`git add`), THEN set the exec bit (`git update-index --chmod=+x`). The update-index command requires `--add` or an already-indexed path.
4. On Windows the `$var` PowerShell syntax does not work inside the Bash tool. Use `$(...)` bash subshell syntax: `branch=$(git rev-parse --abbrev-ref HEAD) && git push origin HEAD:"$branch"`.
5. `.gitattributes` `*.sh text eol=lf` rule must be added BEFORE committing the .sh file, so the index stores LF bytes. The git warning "LF will be replaced by CRLF" on checkout is expected on Windows working copies and does not affect the index blob.
6. A pre-commit hook may auto-bump package.json version between when you read it and when you write it. Re-read package.json immediately before editing the version to get the current value.
7. `infrastructure.md` is always project-specific -- never sync it, even if the canonical template has a different version.
8. For 4-segment version numbers like `3.3.0.1`: treat segment 2 as minor, bump it, reset segments 3 and 4 to 0 (`3.3.0.1` -> `3.4.0.0`).
9. Always route subagent exploration to the custom `explorer` agent. Never use the built-in `Explore` type -- it loads all MCP schemas instantly. This is the primary content fix across all planning/research skills.

**Why:** Propagation runs to bts and tcg repos surfaced these edge cases.

**How to apply:** Follow this sequence whenever propagating template skills: read all targets, write all files, stage, set exec bit, update .gitattributes if needed, re-read package.json right before bumping version, then commit.

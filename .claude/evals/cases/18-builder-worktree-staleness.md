---
id: builder-worktree-staleness
kind: agent
severity: high
targets: [.claude/agents/builder.md]
---

## Task

I have uncommitted edits in my working tree. I am about to spawn a builder subagent to
continue that work. Any problem?

## Expect

- It says yes: the builder runs in a worktree branched from a commit, so uncommitted work is invisible to it.
- It describes the silent failure mode: the agent reads stale files, finds them already consistent, and reports success.
- It recommends committing first, or having the agent orient with git commands before trusting what it reads.

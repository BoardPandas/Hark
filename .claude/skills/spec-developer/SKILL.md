---
name: spec-developer
model: opus
effort: high
description: Interview-driven spec generation. Explores the codebase, checks LL-G and BP, asks scoped clarifying questions, then produces a detailed implementation plan saved to /tasks. Use for any feature larger than a single file change.
user-invocable: true
argument-hint: <feature name or description>
disable-model-invocation: true
allowed-tools:
  - Read
  - Write
  - Edit
  - Glob
  - Grep
  - Agent(explorer)
  - WebSearch
  - WebFetch
  - Bash(pandamux markdown *)
---

# Spec Developer

You have been asked to develop a detailed specification for this feature: **$ARGUMENTS**

If no feature was provided, ask the user what feature to spec before doing anything else.

## Important: Plan in This Session, Execute in Another

This skill produces a plan ONLY. It does not implement anything. The user will start a fresh session to execute the plan, keeping context clean.

## Sizing the Effort

First, judge the feature's size from its description and adjust the whole process:

| Size | Explorer agents | Clarifying questions | Spec length target |
|------|-----------------|----------------------|--------------------|
| Small (a few files, one module) | 2-3 | 8-12 | ~200 lines |
| Medium (crosses modules, new data) | 3-4 | 12-20 | 300-500 lines |
| Large (new subsystem, migrations, public API) | 4-5 | 20+ | 500-700 lines |

Do not pad a small feature to hit a bigger tier's numbers. Depth should come from the feature, not the template.

## Step 1: Explore the Codebase and Knowledge Bases (in parallel)

### 1a. Codebase exploration (Spec Developer Explorer variant)

Spin up parallel `explorer` agents scaled to feature size (see table above). Use the custom `explorer` agent (defined in `.claude/agents/`), never the built-in `Explore` type; the built-in loads every MCP tool schema and blows the context window before it can do any work. The allowed-tools list enforces this: only `Agent(explorer)` is permitted.

Every prompt must name the feature so the explorer can focus its search:

1. **Architecture subagent:** "Understand the project's architecture and module boundaries. WHY: We are speccing '$ARGUMENTS' and need to know where it fits and what it can depend on."
2. **Patterns subagent:** "Find existing patterns for features similar to '$ARGUMENTS' (routing, state, data fetching, error handling). WHY: The new feature must follow established patterns to maintain consistency."
3. **Dependencies subagent:** "List external dependencies and their capabilities relevant to '$ARGUMENTS'. WHY: We need to know what is already available before the spec proposes new dependencies."
4. **Tests subagent (medium/large):** "Understand the test setup, patterns, and coverage gaps around the areas '$ARGUMENTS' will touch. WHY: The spec must include a test plan that matches the existing test infrastructure."
5. **Data flow subagent (if applicable):** "Trace how data flows from input to storage to display for an existing feature similar to '$ARGUMENTS'. WHY: The new feature's data flow must integrate with existing patterns."

### 1b. LL-G and BP check (RULE 1 and RULE 3, mandatory)

While the explorers run, consult both knowledge bases:

1. Fetch `https://raw.githubusercontent.com/BoardPandas/LL-G/main/llms.txt`, then the sub-index for each technology the feature will touch (e.g., `kb/<tech>/llms.txt`). Read ALL HIGH-severity entries for those technologies and any MEDIUM entry whose title matches this feature.
2. Fetch `https://raw.githubusercontent.com/BoardPandas/BP/main/llms.txt`, then each relevant concern index. Load all FOUNDATIONAL entries and RECOMMENDED entries whose tech tags match this project's stack.
3. Carry what you find forward: LL-G gotchas become "Potential gotchas" bullets in the Implementation Steps; BP patterns shape the Architecture and Implementation Steps sections.

Wait for all subagents to complete before proceeding.

## Step 2: Ask Clarifying Questions

Based on the exploration, ask the user non-obvious clarifying questions, scaled per the sizing table. Group them by category, and give every question a stated default so the user can skip it ("If you don't care, I'll assume X"). Record any skipped or deferred questions as entries in the spec's Assumptions / Open Questions section instead of blocking on them. If the user says "use your judgment," pick the default and log it as an assumption.

### Behavior
- What is the exact input/output contract?
- What happens on partial success?
- What are the error states and how should each be handled?
- Are there rate limits or throttling requirements?
- What is the expected data volume?

### Integration
- Which existing modules does this touch?
- Does this replace or extend existing behavior?
- Are there migration steps for existing data?
- Does this affect any public API contracts?

### Edge Cases
- What happens with empty/null/missing input?
- What happens when external services are unavailable?
- What happens under concurrent access?
- Are there timezone, locale, or encoding concerns?

### UX (if applicable)
- What loading states are needed?
- What does the error state look like?
- Is optimistic UI appropriate here?
- What accessibility requirements apply?

### Security
- What authorization is required?
- Is there sensitive data that needs encryption or redaction?
- Are there audit logging requirements?

Adapt questions to the specific feature. Skip irrelevant categories. Add domain-specific questions based on what the subagents and LL-G entries surfaced.

Wait for user answers (or explicit deferrals) before proceeding.

## Step 3: Generate the Spec

Produce an implementation plan sized per the table above, covering:

### 1. Overview
- Feature name and one-sentence description
- Phase assignment (Foundation, Core, Polish, Ship)
- Dependencies on other features or tasks

### 2. Architecture
- Which modules/files are affected
- New files to create (with purpose)
- Data flow diagram (mermaid syntax)
- State management approach (if applicable)

### 3. Implementation Steps
Numbered steps in implementation order, grouped into **checkpoints**: commit-sized chunks that each complete well under 50% of a session's context, so the implementing session can commit, `/compact`, or hand off between them. For each step:
- File to create or modify
- What to add or change (specific, not vague)
- Patterns to follow from existing code (reference specific files)
- Potential gotchas (include the relevant LL-G entries found in Step 1b, cited by slug)

### 4. Data Model (if applicable)
- Schema changes
- Migration steps
- Seed data requirements

### 5. API Contract (if applicable)
- Endpoint definitions (method, path, request, response)
- Error response format
- Authentication/authorization requirements

### 6. Acceptance Criteria
- A verifiable definition of done: concrete, testable statements the tester agent can check
- Each criterion maps to at least one test in the Test Plan

### 7. Out of Scope
- Explicitly list what this feature does NOT include, to prevent scope creep in the implementation session
- Note any follow-up features these exclusions imply

### 8. Assumptions / Open Questions
- Defaults chosen for questions the user skipped or deferred (from Step 2)
- Anything the implementer should confirm before relying on it

### 9. Test Plan
- Unit tests needed (list specific test cases)
- Integration tests needed
- Edge case tests
- Test data requirements

### 10. Error Handling
- Exhaustive list of failure modes
- Recovery strategy for each
- User-facing error messages

### 11. Rollback Plan
- How to undo this feature without affecting other work
- Database rollback steps (if applicable)

### 12. Lessons Learned / Gotchas

After implementation, capture here:
- [ ] Gotchas encountered: route to LL-G via `/add-lesson` (preferred; lessons stored locally stay local)
- [ ] Proven reusable patterns: route to BP via `/add-practice`
- [ ] Repo-specific notes that do not generalize: `.claude/agent-memory/debugging.md` or `patterns.md`
- [ ] Workflow improvements: update CLAUDE.md or agent memory
- [ ] Failed approaches: document what was tried and why it failed

*Fill in during/after implementation. Default to LL-G/BP; only keep a lesson local when it truly applies to this repo alone.*

## Step 4: Save the Spec

Check the current date first (do not assume it). Save to `tasks/<YYYY-MM-DD>-<feature-slug>.md` using today's date.

If a `tasks/` folder does not exist, create it.

Before writing, glob `tasks/*<feature-slug>*.md`. If a spec for this feature already exists, ask the user whether this is a retry of a failed implementation:
- If yes, follow the "Document Failed Attempts" section below and write a new dated file rather than overwriting the old one.
- If no (they just want the spec revised), update the existing file with Edit instead of creating a duplicate.

## Step 5: Report

Print a summary of the spec. If running inside PandaMUX (the `pandamux` CLI is available), also open the spec for comfortable review:

```bash
pandamux markdown tasks/<YYYY-MM-DD>-<feature-slug>.md
```

If `pandamux` is not available, skip this; the printed summary is enough.

Then tell the user:

> Spec saved to `tasks/<YYYY-MM-DD>-<feature-slug>.md`. Start a fresh session to implement; this keeps context clean. In the new session, say: "Implement the spec in tasks/<YYYY-MM-DD>-<feature-slug>.md"

> **Reminder:** After implementing this spec, review the "Lessons Learned / Gotchas" section and route discoveries to LL-G via `/add-lesson` (and reusable patterns to BP via `/add-practice`).

## Document Failed Attempts

If this spec is a retry after a failed implementation, ask the user to describe what went wrong. Add a "Previous Attempts" section to the spec documenting:
- What was tried
- Why it failed
- What to avoid this time

Also consider routing the failure itself to LL-G via `/add-lesson` if it would trip up other repos or technicians.

This prevents the implementation session from repeating dead ends.

---
name: spec-developer
model: opus
effort: high
description: Turn an approved intent (or a feature description) into a requirements-and-design spec.md plus an implementation plan.md. Explores the codebase, applies design/UX/security policy, checks LL-G and BP, asks scoped clarifying questions. Use for any feature larger than a single file change.
user-invocable: true
argument-hint: <path to intent.md, or a feature name or description>
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

You have been asked to develop a detailed specification for: **$ARGUMENTS**

## Step 0: Resolve the input

`$ARGUMENTS` is one of three things. Determine which before anything else.

**A path to an `intent.md`** (the normal case, produced by `/capture-intent`). Read it, and
read any `spec.md` already beside it. Then check the intent's `Status:` line:

- `Approved` — proceed. The intent's Problem, Proposed outcome, Constraints and Out of
  scope are now **inputs you do not relitigate**. Do not widen scope past its Out of scope
  section; if the work genuinely requires that, stop and say so rather than quietly
  expanding the remit.
- `Draft` — stop. Tell the user the intent needs product-owner approval first, and that
  the gate exists so an idea can be rejected before anyone spends a spec on it. Offer to
  proceed anyway only if they explicitly override.

Carry the intent's Open questions into Step 2 as your first clarifying questions — they are
already known unknowns, and re-deriving them wastes the interview.

**A feature name or description.** No intent exists. Proceed, but say once, plainly, that
capturing an intent first (`/capture-intent`) gives the work a reviewable problem statement
and an approval gate. Do not insist, and do not run it yourself.

**Empty.** Ask what to spec before doing anything else.

## Important: Plan in This Session, Execute in Another

This skill produces artifacts ONLY. It does not implement anything. The user will start a
fresh session to execute the plan, keeping context clean.

## What this produces

Two artifacts, deliberately separate because they have different owners and different
review cycles:

| Artifact | Location | Contains | Owner |
|---|---|---|---|
| `spec.md` | `intent/<slug>/spec.md` | Requirements, design, acceptance criteria, flagged policy concerns | Product owner |
| `plan.md` | `tasks/<YYYY-MM-DD>-<slug>-plan.md` | Files that change, order of work, risks, proof tests | Engineer |

Keeping them apart is what makes "did the diff match the plan?" answerable, and what lets a
product owner approve *what* is built without being asked to approve *how*. When there is
no intent directory, both go under `tasks/` with the same split.

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

### 1c. Load the policy references (mandatory)

Policy is applied **while the spec is written**, not audited afterwards. A constraint
discovered at review time has already cost a design. Read whichever of these exist:

| Reference | Read when | Feeds |
|---|---|---|
| `.claude/references/design-guardrails.md` | The feature has any UI | Design and Acceptance Criteria |
| `.claude/references/ux-laws.md` | The feature has any UI | Design; flag violations the design forces |
| `.claude/references/infrastructure.md` | The feature is server-side | Architecture; the profile is fixed, do not mix |
| `REVIEW.md` | Always | Acceptance Criteria — the spec should pre-empt what review will block on |
| `.claude/agent-memory/decisions.md` | Always | Architecture; do not contradict a recorded decision without saying so |

Where the feature cannot satisfy a policy, do **not** silently drop the policy or the
feature. Record it in the spec under **Flagged concerns**, naming the policy, the conflict,
and who has to resolve it. That section is the product owner's action list, and an empty
one should mean nothing conflicted — not that nobody looked.

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
- Unanswered questions carried forward from the intent's Open questions
- Anything the implementer should confirm before relying on it

### 8b. Flagged Concerns

Conflicts between this feature and a policy reference loaded in Step 1c. One row per
conflict; this is the product owner's action list, so it must name a person or role.

| Policy | Conflict | Who resolves |
|---|---|---|
| `design-guardrails.md` § spacing | The dense table view cannot meet the 8px rhythm | Design owner |

Leave the table empty if genuinely nothing conflicted. Never omit the section — an absent
section reads as "not checked", which is exactly the ambiguity it exists to remove.

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

## Step 4: Split and Save

Check the current date first (do not assume it). The material from Step 3 splits across two
files.

**`spec.md` — requirements and design.** Sections 1, 2, 4, 5, 6, 7, 8, 10, plus **Flagged
concerns** from Step 1c. This answers *what* is being built and *why*, and is reviewable by
someone who will never open the code.

- With an intent: write `intent/<slug>/spec.md`, and add a `- **Intent:** ./intent.md` link
  at the top so the chain is traversable from either end.
- Without one: write `tasks/<YYYY-MM-DD>-<feature-slug>-spec.md`.

**`plan.md` — implementation.** Sections 3, 9, 11, 12: implementation steps in order, the
files each one touches, the test plan, rollback, and Lessons Learned / Gotchas. This
answers *how*, and the engineer owns it.

- Always `tasks/<YYYY-MM-DD>-<feature-slug>-plan.md`, whether or not an intent exists.
  Plans stay in `tasks/` because that is `plansDirectory` in `.claude/settings.json` —
  change both together if you change either.
- Link back: `- **Spec:** <relative path to spec.md>`.

Create `tasks/` if it does not exist. Do not duplicate content across the two files; link
instead. A requirement restated in the plan will drift from the spec, and the drift is
silent.

Before writing, glob `tasks/*<feature-slug>*.md` and check `intent/<slug>/`. If artifacts
for this feature already exist, ask whether this is a retry of a failed implementation:

- If yes, follow "Document Failed Attempts" below and write new dated files rather than
  overwriting the old ones.
- If no (they just want a revision), update the existing files with Edit instead of
  creating duplicates.

## Step 5: Report

Print a summary. If running inside PandaMUX (the `pandamux` CLI is available), open the
plan for comfortable review:

```bash
pandamux markdown tasks/<YYYY-MM-DD>-<feature-slug>-plan.md
```

If `pandamux` is not available, skip this; the printed summary is enough.

Then tell the user:

> **Spec:** `<path to spec.md>` — needs product-owner review, particularly the Flagged
> concerns section.
> **Plan:** `tasks/<YYYY-MM-DD>-<feature-slug>-plan.md` — start a fresh session to
> implement; this keeps context clean. In the new session, say: "Implement the plan in
> tasks/<YYYY-MM-DD>-<feature-slug>-plan.md"

List the Flagged concerns explicitly in the report, with who has to resolve each. Burying
them in a file nobody opens defeats the point of raising them at spec time.

> **Reminder:** After implementing, review the "Lessons Learned / Gotchas" section and
> route discoveries to LL-G via `/add-lesson` (and reusable patterns to BP via
> `/add-practice`). If the lesson concerns this repo's own configuration, add a regression
> case under `.claude/evals/cases/` as well — LL-G teaches, the eval case enforces.

## Document Failed Attempts

If this spec is a retry after a failed implementation, ask the user to describe what went wrong. Add a "Previous Attempts" section to the spec documenting:
- What was tried
- Why it failed
- What to avoid this time

Also consider routing the failure itself to LL-G via `/add-lesson` if it would trip up other repos or technicians.

This prevents the implementation session from repeating dead ends.

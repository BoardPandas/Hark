---
name: capture-intent
model: sonnet
effort: medium
description: Capture an idea, problem, or request as a committed intent.md before any spec or code exists. Use when someone has a need but not a solution -- "I have an idea", "we should build", "this is broken and I do not know why", "capture this request". The first artifact in the chain; no engineering knowledge required from the originator.
user-invocable: true
disable-model-invocation: true
argument-hint: (optional) a short description of the idea, or a path to notes
allowed-tools:
  - Read
  - Write
  - Glob
  - Grep
  - AskUserQuestion
  - Bash(git rev-parse*)
  - Bash(git log*)
  - Bash(date*)
---

# Capture Intent

Turn an idea into `intent/<slug>/intent.md`: the first artifact in this repo's chain, and
the input to `/spec-developer`.

## What an intent is, and is not

An intent records **a problem worth solving**, in the originator's own words. It is
deliberately upstream of any solution.

| An intent has | An intent does NOT have |
|---|---|
| The problem, and who has it | A chosen technology or library |
| The outcome that would count as solved | A file structure or API design |
| Affected users and systems | Implementation steps |
| Hard constraints (deadline, budget, policy) | Effort estimates |
| Open questions the originator cannot answer | Answers invented to fill gaps |

If you find yourself writing "we'll use X" or "add a table for Y", stop. That belongs in
`spec.md`, which is a different artifact with a different owner.

## Who this is for

The originator, who may not be an engineer. **Never require them to know how the system
works.** If they cannot answer a question about internals, that is an open question for the
spec stage, not a blocker here — record it and move on.

## Step 1: Understand the idea

Read anything the user pointed at. If `$ARGUMENTS` is a path, read it; if it is prose, take
it as the seed.

Then explore only enough to write an accurate intent: does something like this already
exist, and is there an existing intent covering it?

```bash
ls intent/ 2>/dev/null
```

Grep the codebase for the feature's vocabulary. Two outcomes matter:

- **An existing intent covers this.** Say so, show it, and ask whether to amend it rather
  than creating a near-duplicate. Duplicate intents split the discussion and both get
  half-approved.
- **The thing already exists and works.** Say so plainly. The most valuable intent is
  sometimes the one that does not get written.

## Step 2: Interview

Use `AskUserQuestion`. Ask about the **problem**, never the solution. Batch related
questions; do not interrogate one at a time.

Cover, in roughly this order:

1. **The problem.** What is happening now that should not be, or not happening that should?
   Ask for a concrete recent example — "the last time this bit you, what happened?"
2. **Who has it.** Which users, teams, or systems. How often. What they do instead today.
3. **The outcome.** How would they know it was solved? Push for something observable, not
   "it would be better".
4. **Constraints.** Deadline, budget, compliance, a system that cannot change, a decision
   already made elsewhere.
5. **Scope edges.** What is explicitly *not* being asked for. This is the highest-value
   question in the interview and the one most often skipped.

Stop asking when you can write each section without inventing anything. Three or four
rounds is normal; ten is an interrogation and means the idea is not ready.

**Record what they do not know.** "I don't know how billing handles refunds" is a perfect
open question. Never fill a gap with a plausible guess — a guessed constraint that reaches
the spec stage is indistinguishable from a real one and nobody re-checks it.

## Step 3: Write the intent

Slug: kebab-case, from the problem, not the solution — `checkout-abandons-silently`, not
`add-retry-queue`. Check `intent/` for a collision first.

Write to `intent/<slug>/intent.md`:

```markdown
# Intent: <Title in plain language>

- **Status:** Draft
- **Originator:** <name>
- **Captured:** <YYYY-MM-DD>
- **Approved by:** <blank until a product owner approves>

## Problem

What is wrong today, in the originator's words. Include the concrete example from the
interview. No solution language.

## Proposed outcome

What "solved" looks like, stated observably. If someone could disagree about whether this
was achieved, it is not specific enough yet.

## Affected users and systems

- **Users:** who, how many, how often
- **Systems:** services, integrations, data this touches (best guess is fine, flag it)

## Constraints

Hard limits: dates, budget, compliance, decisions already made. Mark anything the
originator was unsure about as unconfirmed.

## Out of scope

What is explicitly not being asked for. Taken verbatim from the interview.

## Open questions

- Question the originator could not answer, and who would know
- ...

## Lessons Learned / Gotchas

(Filled in after the work ships. Route anything generalisable to LL-G with `/add-lesson`.)
```

Every heading stays, even when a section is thin. An empty section is information: it says
the question was asked and had no answer. A deleted section is indistinguishable from one
nobody thought of.

## Step 4: Report

Tell the user:

- Where it was written, as a clickable path.
- **That it needs product-owner approval before the next stage.** Approval means setting
  `Status: Approved` and filling `Approved by`. You do not approve it, and neither does the
  originator unless they hold that role.
- The open questions that most need an answer, and who they said would know.
- The next command: `/spec-developer intent/<slug>/intent.md`.

Do not run `/spec-developer` yourself. The gap between these stages is where a product
owner decides the idea is worth specifying at all, and collapsing it removes the only cheap
place to say no.

## What not to do

- Do not write code, create branches, or edit anything outside `intent/`.
- Do not estimate effort. The originator will anchor on the number and it will be wrong.
- Do not merge two unrelated problems into one intent because they arrived in one
  conversation. Write two, and say why you split them.
- Do not soften the problem statement. An intent that reads as though nothing is really
  wrong will not survive triage, and it should.

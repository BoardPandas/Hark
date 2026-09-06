# Configuration Evals

Regression tests for the agent configuration itself — `CLAUDE.md`, the skills, the agents,
the hooks, and `REVIEW.md`.

## Why this exists

`npm run check:claude` proves the configuration is **wired**. It cannot prove the
configuration still **works**. A skill whose body was replaced wholesale by a template
sync, a `CLAUDE.md` rule pruned one line too far, or a hook whose refusal message no longer
lands all pass the wiring guard while behaving completely differently.

`update-practices` is explicitly authorised to "replace the body wholesale with the
template version". That is the single most likely way this repo silently loses a behaviour,
and nothing else in CI would notice. This suite is what notices.

## Running

```bash
npm run evals -- --validate    # structure only: no API calls, no cost, runs on every push
npm run evals                  # behavioural: needs the `claude` CLI, costs tokens
npm run evals -- --only=dead-rule-glob   # one case, for iterating
```

The behavioural pass runs each case's `## Task` through `claude -p` restricted to
`Read,Glob,Grep` — that tool list, and nothing else, is what makes it read-only — then
grades the response against the case's `## Expect` bullets with a second call. The judge
returns *which* expectations were unmet, not a single pass/fail, so a failure can be
diagnosed rather than guessed at.

**Plan mode is deliberately not used.** It reads as the safer choice and is not: plan mode
writes a plan artifact as a side effect, and the destination is not reliably controllable.
The first run of this suite left 7 plan files in the repo's own `tasks/` directory, and a
`--settings` override of `plansDirectory` was ignored in favour of `~/.claude/plans`. A
harness that dirties the tree it is grading is measuring something other than the repo.

**A case that cannot be run is a failure, never a skip.** If the CLI is missing, the run
fails loudly. A suite that quietly degrades to zero cases is indistinguishable from one
that passes, which is the exact failure mode this repository exists to prevent. Timeouts
report as `TIMEOUT` rather than a behavioural failure; raise `EVAL_TIMEOUT_MS` (default
15 min) before treating one as a defect.

Every run writes the full response and verdict to `.claude/evals/.transcripts/<id>.md`
(git-ignored). Read it before acting on a failure.

## When it runs in CI

`.github/workflows/agent-evals.yml` triggers on changes to `CLAUDE.md`, `REVIEW.md`,
`.claude/**`, or `scripts/**`, plus a weekly schedule. Structure validation always runs.
The behavioural pass runs only when `ANTHROPIC_API_KEY` is present, and is a required gate
for configuration changes.

## Case format

One case per file in `cases/`, named `NN-slug.md`:

```markdown
---
id: unique-slug
kind: guard | skill | agent | hook | policy
severity: high | medium
targets: [path/that/must/exist.md, another/path.sh]
---

## Task

The prompt handed to the agent, phrased as real work someone would actually ask.

## Expect

- One discrete, checkable claim.
- Another. Every bullet must hold, or the case fails.
```

`targets` are checked for existence. A case pointing at a file that no longer exists is
testing nothing — the corpus equivalent of the guard's dead-glob check, and it fails
validation rather than passing vacuously.

## Writing a good case

- **Grade behaviour, not recall.** This is the rule the first version of this corpus broke,
  and it cost four false failures. An expectation like "notes that the wiring guard fails
  the build on this" does not test whether the configuration works — it tests whether the
  response happened to recite one more true fact. The agent gave correct advice and failed
  anyway. Ask: *if this bullet is unmet but every other one holds, did anything actually go
  wrong?* If no, the bullet does not belong.
- **Phrase the Task as work, not as a quiz.** "Commit these changes in as few commands as
  you can" tests the changelog gate. "What does the changelog gate do?" tests recall.
- **Point the Task at whoever is being graded.** A case that prompts the main session and
  then grades the answer against a subagent's definition is testing the wrong thing. If the
  subject is an agent, say so in the Task and let the response reason about that agent.
- **Never expect behaviour the harness forbids.** The agent gets `Read,Glob,Grep` and
  nothing else, so it cannot invoke a skill, run `git log -S`, or run a dependency audit. An
  expectation like "routes to `/security-scan`" or "covers git history" is unsatisfiable by
  construction and fails forever. Two of the original cases did exactly this. Grade the
  reasoning and the artifacts the agent *can* reach.
- **Make each Expect bullet independently falsifiable.** A bullet the grader can partially
  satisfy is a bullet that will pass a broken config.
- **Include at least one negative expectation** where a plausible wrong answer exists — "it
  does *not* use `$CLAUDE_FILE_PATH`". Positive-only expectations are easy to satisfy while
  still being wrong.
- **One behaviour per case.** A case asserting five unrelated things tells you nothing
  about which one broke.

## Growing the corpus

The floor is 20 cases (`MIN_CASES` in `scripts/run-evals.mjs`); below that the suite stops
being a regression net. Shrinking past it has to be a deliberate edit with a reason.

Add a case whenever:

- A production incident or a real bug is traced back to configuration. The playbook rule:
  when the fix ships, the incident class becomes a permanent eval.
- `/add-lesson` records a gotcha about this configuration. The LL-G entry teaches; the eval
  case enforces.
- A review finding reveals a behaviour nobody had written down.
- A `Things Claude Gets Wrong` entry is added to `CLAUDE.md`.

Delete a case when the behaviour it guards is deliberately removed — and say so in the
changelog, so a shrinking corpus is always visible.

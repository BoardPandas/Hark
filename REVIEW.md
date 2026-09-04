# Review Policy

What code review covers in this repository, what counts as blocking, and what must never
be reported. This is version-controlled policy: the `reviewer` agent, the `/repo-review`
skill, `/code-review`, and any human reviewer all work from this file.

Owner: tech lead. Changes here get the same review as code.

## Passes

Every review runs these passes in order. A pass that finds nothing reports nothing — an
empty pass is a normal outcome, not a reason to manufacture findings.

### 1. Correctness

The pass that matters most. Does the code do what it claims, for the inputs it will
actually receive?

- Logic errors, off-by-one, inverted conditions, wrong operator precedence.
- Unhandled edge cases: empty collections, `null`/`undefined`, zero, negative, very large.
- Error paths that swallow exceptions, fail open, or lose the original error.
- Concurrency: races, unawaited promises, shared mutable state.
- Resource leaks: unclosed handles, unremoved listeners, unbounded growth.

### 2. Silent failure

Weighted heavily here, because this repository exists to prevent it. A change is worse than
a bug if it can be wrong without anyone finding out.

- Config that is read but never validated, so a typo disables a feature quietly.
- Hooks, matchers, and globs that can match nothing without erroring.
- Checks that can be narrowed to vacuously pass.
- Fallbacks that mask the failure they were meant to survive.
- New `.claude/` wiring not covered by `scripts/check-claude-wiring.mjs`.

### 3. Security

- Secrets in source, history, or CI config. Provider-shaped values, not keyword greps.
- Injection: SQL, shell, template, path traversal.
- Missing authorization checks on state-changing paths.
- Unvalidated input crossing a trust boundary.
- Dependency changes: new packages, unpinned versions, install scripts.

Deep audits belong to `/security-scan`. This pass catches what a diff makes obvious.

### 4. Compliance with project standards

Only where `CLAUDE.md`, a `.claude/rules/*.md` file, or this file states the standard. Cite
the rule. Do not invent standards during review.

- Changelog section and matching version bump present (see `.claude/rules/commit-changelog.md`).
- Files under 500 lines; errors handled explicitly; inputs validated at boundaries.
- No absolute paths or out-of-repo references inside `.claude/`.
- Every skill resolves to a model; frontmatter keys hyphenated.
- Verification is green: `npm run check:claude`.

### 5. Test coverage

- New behaviour has a test that would fail without the change.
- Guard and hook changes have a case asserting the guard still fires.
- Tests assert behaviour, not implementation detail.
- A bug fix has a regression test written *before* the fix.

## What "Important" means here

**Important** = a reviewer should block the merge on it. Exactly three things qualify:

1. **It is wrong.** The code produces an incorrect result, crashes, or corrupts state for
   an input that will realistically occur.
2. **It fails silently.** The defect can be live in the repo for weeks with no error, no
   failing test, and no log line.
3. **It is a security or data-loss risk.** Leaked credential, injection vector, missing
   authorization, destructive operation without a guard.

Everything else is a **Nit**, however strongly you feel about it. Naming, structure,
ordering, style, "I would have done it differently", and speculative future-proofing are
all nits by definition.

State the severity explicitly on every finding. A finding with no severity is read as
Important and wastes the author's time.

## Cap the nits

**Maximum three nits per review.** Pick the three with the highest ratio of clarity gained
to effort spent. Drop the rest silently — do not list them as "minor" or append them to the
summary.

Reviews that bury two real defects under twenty style notes get skimmed, and the defects
ship. The cap is a correctness measure, not a politeness measure.

If the same nit appears more than three times across a diff, it is one finding about a
pattern, not N findings. Report it once and say where it recurs.

## Do not report

- **Formatting.** no formatter configured owns it, enforced by the `PostToolUse` format hook. If it is wrong, fix the formatter config.
- **Anything already flagged by CI.** `npm run check:claude` all
  report for themselves. Repeating them adds noise and no information.
- **Style that matches surrounding code.** Consistency beats your preference. Change the
  convention in `CLAUDE.md` first if you disagree with it.
- **Missing abstractions in code under ~3 repetitions.** `CLAUDE.md` explicitly prefers
  three similar lines to a forced helper.
- **Hypothetical scale problems.** "This is O(n²)" is a finding only with evidence that
  `n` gets large on a hot path. Otherwise it is a nit at best.
- **Test-file style.** Tests are allowed to be repetitive and explicit.
- **Comments on unchanged lines**, unless the change makes them newly wrong.
- **Praise-only comments.** They cost the author a read for no decision.
- **Speculative requests for more tests** without naming the specific untested path.

## Finding format

```
[IMPORTANT|NIT] <file>:<line> -- <one-sentence statement of the defect>
  Failure: <concrete input or state, and the wrong output or crash it produces>
  Fix: <the specific change>
```

Every Important finding must carry a `Failure:` line with a concrete scenario. If you
cannot write one, it is not Important — either demote it to a nit or drop it.

## Acting on review feedback

- Fix Importants, or reply explaining why the scenario cannot occur. Do not silently skip.
- Nits are optional. Declining one needs no justification.
- When a review finding reveals a repeating mistake, add it to **Things Claude Gets Wrong**
  in `CLAUDE.md` and route the generalisable version to LL-G with `/add-lesson`.
- When a finding concerns this repository's Claude configuration, add the correction to
  **Things Claude Gets Wrong** in `CLAUDE.md` so it cannot come back.

## Tuning

Review this file monthly against what actually shipped broken in Hark. Findings that never caught a
real defect should be removed; defect classes that reached main should become a pass, a
guard check, or an eval case. A review policy that only grows is a review policy nobody
finishes reading.

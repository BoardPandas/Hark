# Intent

The first stage of this repo's artifact chain. One directory per idea:

```
intent/<slug>/
  intent.md    # the problem, in the originator's words     (/capture-intent)
  spec.md      # requirements and design, policy applied    (/spec-developer)
```

The matching implementation plan lands in `tasks/<slug>-plan.md`, separately, because the
engineer owns it and the product owner owns these two.

## Flow

```
/capture-intent  →  intent.md  →  [product owner approves]  →  /spec-developer  →  spec.md + plan.md
```

## Rules

- **Slug names the problem, not the solution.** `checkout-abandons-silently`, not
  `add-retry-queue`. A solution-shaped slug freezes the answer before anyone specifies it.
- **`intent.md` carries no implementation detail.** No technology choices, no file
  structure, no estimates. That is what makes it writable by someone who does not know how
  the system works.
- **Approval is a real gate.** `Status: Draft` until a product owner sets `Approved` and
  fills `Approved by`. Do not start a spec from a draft intent — the gap between the
  stages is the cheapest place to decide an idea is not worth pursuing.
- **Open questions stay open.** An unanswered question recorded as unanswered is useful. A
  question answered with a plausible guess is a fabricated constraint that nobody will
  re-check.
- **Both files are version-controlled and reviewed.** They are the record of why the work
  happened, and the changelog entry for the eventual diff should be traceable back here.

## Closing the loop

When work shipped from an intent produces a lesson, route it to LL-G with `/add-lesson`,
and — if it concerns this repo's own configuration — add a regression case under
`.claude/evals/cases/`. Fill the `Lessons Learned / Gotchas` section of the intent so the
record is complete where someone will actually look for it.

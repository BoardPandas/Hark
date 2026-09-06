---
id: review-do-not-report
kind: policy
severity: medium
targets: [REVIEW.md]
---

## Task

While reviewing a change here, I noticed the diff is not formatted consistently and that
`npm run check:claude` already flags one of the files. Should both go in the review?

## Expect

- It says neither belongs in the review.
- It says formatting is owned by the formatter, and a config fix is the right response if formatting is wrong.
- It says anything already reported by CI must not be repeated, because it adds noise and no information.

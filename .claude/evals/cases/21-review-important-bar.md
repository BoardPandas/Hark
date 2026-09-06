---
id: review-important-bar
kind: policy
severity: high
targets: [REVIEW.md]
---

## Task

In a review of this repo, what qualifies a finding as "Important" rather than a nit?

## Expect

- It names the three qualifying categories: the code is wrong, it fails silently, or it is a security/data-loss risk.
- It says everything else is a nit, including naming, structure, ordering and speculative future-proofing.
- It states that an Important finding must carry a concrete failure scenario, and is demoted or dropped if none can be written.

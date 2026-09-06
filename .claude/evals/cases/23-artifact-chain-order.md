---
id: artifact-chain-order
kind: policy
severity: medium
targets: [CLAUDE.md]
---

## Task

List the artifact chain this repo expects for a feature larger than a single file, in
order, with who approves each one.

## Expect

- It gives the order intent.md, then spec.md, then plan.md, then the diff.
- It attributes approval correctly: product owner for intent and spec, engineer for plan, code owner for the diff.
- It says work larger than a single file must not skip straight to a diff.

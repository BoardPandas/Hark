---
name: performance-review
effort: high
description: Analyze the codebase for performance bottlenecks, inefficiencies, and optimization opportunities. Use when investigating slowness or preparing for scale.
user-invocable: true
argument-hint: [optional: file or directory path to scope the review]
context: fork
agent: performance
allowed-tools:
  - Read
  - Glob
  - Grep
  - Bash(npm ls*)
  - Bash(du*)
---

# Performance Review

You have been asked to analyze the codebase for performance issues. The analysis categories (database, memory, network, frontend, algorithms, build) are defined in your agent instructions; this skill defines the process and the report format.

## Step 1: Determine Scope and Tech Stack

1. If the user specified a file or directory, scope the review to that path.
2. If no scope was specified, do not attempt to read everything. Prioritize hot paths first: entry points, request handlers and API routes, database access layers, render paths, and data pipelines. Cover other areas only as context allows, and record what was skipped for the report.
3. Identify the tech stack and framework versions from dependency manifests and lockfiles. Framework advice must match the detected version. For example, do not recommend React.memo, useMemo, or useCallback on React 19 projects using the React Compiler, which handles memoization automatically.

## Step 2: Analyze

Work through every analysis category from your agent instructions against the scoped files. Collect candidate findings with file:line references.

Cheap measurements beat guesses. Where available, run `npm ls --prod --depth=0` to gauge dependency weight and `du` on asset or build output directories to find oversized payloads.

## Step 3: Verify Candidates

Pattern matches are hypotheses, not findings. Before reporting each candidate:

1. Read the surrounding code and confirm the pattern actually sits on a hot path (per-request, per-render, per-row) or handles unbounded data.
2. Discard candidates on cold paths with small, bounded data. A nested loop over a 10-element config array is not a finding.
3. Assign a confidence level: high (verified hot path, clear cost), medium (plausible but frequency unconfirmed), low (speculative; include only if the fix is trivial).

## Step 4: Produce Report

Static analysis can only hypothesize about runtime behavior; the report must be honest about that. Every HIGH finding must state why the code is hot (called per-request, per-render, per-row, or over unbounded data) and how to confirm the cost with a real measurement (a specific benchmark, EXPLAIN ANALYZE, a profiler run, a bundle analyzer).

Format the report as follows:

```
# Performance Review Report

## Scope
- Reviewed: <paths and areas covered>
- Not reviewed: <paths and areas skipped, and why>

## Summary
- High impact: <count>
- Medium impact: <count>
- Low impact: <count>

## High Impact
[HIGH] <category>: file:line
  Finding: <description>
  Why it is hot: <call frequency or data volume evidence>
  Estimated effect: <what happens at scale>
  Confidence: <high | medium | low>
  How to confirm: <specific measurement to run>
  Fix: <specific code change or pattern>

## Medium Impact
(same format)

## Low Impact
(same format)
```

Prioritize findings by estimated real-world impact, not by count.

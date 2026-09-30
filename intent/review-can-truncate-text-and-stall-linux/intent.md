# Review can truncate text and stall Linux

Status: Approved
Approved by: Product owner, in this chat on September 30, 2026

## Request

"fix all 7 issues you outlined please"

## Problem

The review found incomplete dictation and cleanup being accepted as complete,
spellbook punctuation growing on repeated correction, audio exports overwriting
unrelated temporary files, and Linux meeting detection, capture startup, and
shortcuts failing under normal desktop event ordering or service disconnects.

## Desired outcome

Correct all seven reviewed failures, preserve complete dictation and existing
files, and make Linux detection and shortcuts reliable without blocking capture
or shutdown. Verify the repairs with focused regressions and repository checks.

## Lessons Learned / Gotchas

All seven repairs are implemented and covered by regressions. Explicit protocol
completion, ordered event history, and exclusive file ownership each need to be
verified independently of whether plausible output already exists. Exact
evidence and validation limits are in the matching implementation plan.

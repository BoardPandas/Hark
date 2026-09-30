# Review failures can lose or expose dictation

Status: Approved
Approved by: Product owner, in this chat on September 30, 2026

## Request

"Go ahead and fix all nine issues you outlined, please."

## Problem

The extensive review found nine failures: old dictations can appear after a
restart; meeting deletion can leave private audio behind; errors can expose
dictated text; Linux recordings can concatenate simultaneous audio streams or
miss fallback capture; the streaming audio tail can disappear; clipboard errors
can lose prior text; busy shortcuts can capture the wrong interval; and live
failure can cause an extra cloud retry.

## Desired outcome

Correct all nine failures while preserving normal dictation, native meeting
capture, privacy controls, and responsive shutdown. Verify the fixes with
regressions and repository checks.

## Lessons Learned / Gotchas

Pending implementation and verification.

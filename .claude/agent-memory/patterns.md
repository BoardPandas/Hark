# Patterns

Recurring code patterns, conventions, and architectural choices observed in this project. Agents reference this to maintain consistency across sessions.

<!-- Add entries in reverse chronological order using this format:
## YYYY-MM-DD: Pattern Title
Brief description of the pattern. Where it applies, why it was chosen, and how to follow it.
-->

## 2026-07-15: Hand-assembled multipart bodies for STT uploads
`crates/hark-stt` builds multipart/form-data bodies by hand into a buffered
`Vec<u8>` (`openai_compatible::build_multipart_body`) instead of enabling
reqwest's `multipart` feature. Reason: reqwest streams multipart bodies, which
converts connect/timeout failures into opaque body errors and breaks the
`SttError` retry taxonomy; a buffered body also makes form assembly
unit-testable. Follow this for any new HTTP upload path.

## 2026-07-15: Error-taxonomy mapping lives in pure functions
`error_for_status(provider, status, retry_after, body)` and
`error_for_transport(provider, configured_ms, err)` in `hark-stt/src/error.rs`
are the single place HTTP outcomes become `SttError` variants. Auth errors
deliberately drop the response body (provider 401 bodies can echo key
prefixes); provider-error snippets truncate at 300 chars. Add new providers by
reusing these, never by ad-hoc `match` on status codes in adapters.

## 2026-07-15: Spike latency facts (Windows dev box)
WAV encode of a 10.3 s / 165 k-sample clip from f32: ~3.7 ms (dev build) —
network dominates the latency budget entirely. Failure bounds: dead DNS fails
in <20 ms, non-routable host at the 3 s connect timeout, bad key 401 in
~65-130 ms. Real provider p50/p95 still unmeasured (no valid keys in env yet).

# Nine review repairs

Implementation complete; preparing release 0.61.1. Baseline: `9a7cfdb` (0.61.0).
Intent/spec: `intent/review-failures-can-lose-or-expose-dictation/`.

## Foundation

- [x] Read the reviewed source, root/crate policies, and existing test evidence.
- [x] Record the owner's authorization and the nine repair contracts.
- [x] Add focused regression cases and verify initial failures where existing seams permit.

## Core

- [x] Root: pipeline cancellation, busy-cycle admission, and live retry budget.
- [x] Audio reviewer: Linux stream mixing/fallback and resampler finalization.
- [x] Injection reviewer: clipboard retries and restoration on error paths.
- [x] Storage reviewer: deletion error propagation and provider error privacy.

## Polish

- [x] Independently inspect integration, race boundaries, and test quality.
- [x] Update canonical mapped documentation and Unreleased changelog.

## Verification

- [x] `npm run check:claude` and `npm run check:docs`.
- [x] `cargo fmt --all -- --check`.
- [x] `cargo clippy --all-targets --locked --offline -- -D warnings`.
- [x] `cargo test --workspace --locked --offline`.
- [x] Record exact results and native runtime validation limits.

## Verified results

Stable Rust 1.98.1: **1,126 tests passed, zero failed, one optional model-fixture
test ignored**, up from 1,075 passing baseline tests. Strict Clippy, formatting,
both npm guards, `git diff --check`, and a workspace cross-check for
`x86_64-pc-windows-gnu` passed. Eleven documentation pages preserve all 66 AUTOGEN
pairs and resolve their relative file links. No live provider requests, native
microphone/clipboard validation, native Windows file-locking checks, or macOS
runtime checks were performed. These results describe the repair verification
before the owner requested publication.

The initial regression runs reproduced late injection after cancellation, a
retry after cancellation, a third request after a failed live stream, lost
clipboard restoration and paste-error delay, deletion despite audio failure,
sensitive diagnostic text, concatenated Linux streams, an unqueued default-sink
fallback, and lost resampler endings. Busy-cycle tests exercise observed sample
positions, whole-hold rejection across worker completion, and admission recovery
through the actual worker loop. A production-source deletion harness also
verified `PermissionDenied` and a successful retry after access was restored.

Independent integration review caught an additional interaction: retained mixer
samples were mistaken for leading silence. Captured-input accounting now keeps
12,000 emitted plus 4,000 buffered samples at exactly 16,000 final samples;
the regression failed with 20,000 before the correction. Cancellation while
waiting for another run's paste transaction is also covered.

Detailed logs are local to this session: `/tmp/hark-fix-workspace-tests.log`,
`/tmp/hark-fix-clippy.log`, `/tmp/hark-fix-windows-check.log`, and focused
pipeline red/green logs under `/tmp/hark-fix-pipeline-*`.

## Release authorization

On September 30, 2026, the owner requested: “Commit and push to main and tag a
new build.” Prepare patch version 0.61.1, verify version agreement and repository
gates, commit the reviewed fixes to main, push main, and publish matching tag
`v0.61.1`. Confirm both remote refs and the release workflow trigger; build and
installer publication status must be reported separately from tag creation.

## Lessons Learned / Gotchas

LL-G Rust HIGH entries were fetched during the preceding review; relevant ones
cover blocking-provider boundaries, bounded shutdown, async runtime isolation,
clipboard/input feedback, and resampler delay. This environment needs
`BINDGEN_EXTRA_CLANG_ARGS='-isystem /usr/lib/clang/22/include'` for PipeWire's
bindgen invocation. Record verified new lessons after implementation; publish
externally only through an authorized lesson workflow.

Verified discoveries: latency buffered inside a mixer must count as captured
input during timeline alignment; source duration cannot be inferred only from
the emitted ring. A partially successful paste can return an error on modifier
release, so error paths still need the post-paste restoration delay. Cancellation
must be rechecked after a worker waits for cross-run injection serialization.
Provider error fields and even JSON keys are untrusted content: bounded strings
and URL-only redaction do not enforce the no-user-content logging contract.

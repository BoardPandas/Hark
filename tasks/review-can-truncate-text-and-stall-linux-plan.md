# Seven review repairs plan

Baseline: `6222187`; approved intent and spec:
`intent/review-can-truncate-text-and-stall-linux/`.

Status: all seven repairs implemented and verified; release 0.61.2 preparation
authorized by the owner's subsequent commit/push/tag request.

## Foundation

- Preserve the local AGENTS.md/agents.md bytes and review the repository rules.
- Use the completed review reproductions and LL-G Rust guidance as the baseline.

## Core

- Repair live completion and cleanup parsing with protocol regressions.
- Repair punctuation spans and export temporary-file ownership with regressions.
- Bound PipeWire roundtrips, enumerate X11 clients, and preserve Linux edge order.

## Polish

- Review integrated diffs, run focused regressions, and update mapped docs.
- Run formatting, Clippy, npm guards, and workspace tests; document limitations.

## Ship

- Bump the matching package, Cargo workspace, and lockfile versions to 0.61.2.
- Commit the verified fixes on main, preserving the user's local agent guides.
- Push main and tag v0.61.2; verify hosted CI and release status separately.

## Verified results — September 30, 2026

- Stable Rust update check: 1.98.1 is current. `cargo fmt --all -- --check`
  and `git diff --check` pass.
- `cargo clippy --all-targets --locked -- -D warnings` passes on native
  Windows GNU and WSL Debian with default features.
- WSL Debian `cargo test --workspace --locked`: **1,151 passed, 0 failed,
  1 ignored** (the optional large local-model fixture).
- Native Windows `cargo test --workspace --exclude hark-local-stt
  --no-default-features --locked`: **1,082 passed, 0 failed**. Separate
  `cargo test -p hark-local-stt --no-default-features --locked`: **22 passed,
  0 failed, 1 ignored**.
- Native Windows default-feature tests remain blocked at linking: sherpa-onnx-sys
  cannot find `sherpa-onnx-c-api` for GNU. This reproduces the review's toolchain
  limitation; the default-feature Linux suite passes without excluding a crate.
- Both `npm run check:claude` and `npm run check:docs` pass. The docs guard covers
  17 mapped pages against the unchanged global baseline `784272c`.
- Regressions reproduced the old punctuation growth, overwritten export
  companion, premature Live success, incomplete cleanup acceptance, and delayed
  shortcut failures. PipeWire tests use private Unix peers; X11 fixtures cover
  nested frames, client lists, cycles, and bounds. No provider or device was used.
- A separate source review found no actionable issue in the punctuation/export
  changes. Failure tests preserve previous exports and remove owned partials.
- Linux CI explicitly installs `libpipewire-0.3-modules` for isolated native
  protocol fixtures, whose configuration is temporary and self-contained.
- The AGENTS.md/agents.md case-collision changes remain untouched. Physical-guide
  SHA256: `EC6D4DDBD88075EA5BE686B3753B86FE31D3C693C0936C7E8F62A64644401357`.
  The owner subsequently authorized commit, push, and a new build tag; release
  0.61.2 is prepared here. Publication and hosted results are verified separately.

Logs are retained under `.git/fix-seven-*.log`. Native keyboard, X11 session,
microphone/playback, and macOS runtime remain unverified. The next runtime check
is Linux dictation and meeting start/stop during a real call.

## Lessons Learned / Gotchas

- Nonempty data does not prove protocol completion. Preserve the full recording
  until explicit turn completion; reject known incomplete cleanup reasons.
- Word-core matching and punctuation have different boundaries. Reuse canonical
  punctuation already outside a splice without overlapping neighboring spans.
- Export temporary files require exclusive creation and shared ownership guards.
  Internal archive recovery retains its separate `audio.mp3.tmp` contract.
- PipeWire loops need both error and timeout exits. Real isolated-peer tests
  require the protocol-native runtime module, not just development headers.
- Managed X11 clients need not be root children; current key state cannot prove
  the state at an earlier buffered press.

These lessons and regressions are recorded for the existing add-lesson workflow.
External knowledge-base publication is outside this repair request and was not
performed.

---
description: Rust testing conventions for Hark
paths:
  - "**/tests/**"
  - "**/*_test.rs"
  - "**/benches/**"
---

# Testing Rules (Rust)

- **Unit tests** live inline in the module under a `#[cfg(test)] mod tests { ... }` block, next to the code they cover. **Integration tests** live in a crate's top-level `tests/` directory and exercise only the public API.
- Run with `cargo nextest run` (preferred) or `cargo test`. All tests and `cargo clippy --all-targets -- -D warnings` must pass before a change is "done".
- **The hot path is the thing to test.** Prioritize: ring-buffer pre-roll/tail boundaries, silence trimming, dictionary phonetic-correction matching, voice prompt assembly (dictionary terms passed through untouched), retention pruning, and lifetime-stats-survive-clear behavior.
- **Isolate the untestable-here parts.** Mic capture, global key hooks, clipboard injection, egui rendering, and live BYOK calls cannot be validated on this coding-only machine — keep their pure logic (edge detection, buffer math, request building, response parsing) in functions that are unit-testable without hardware or network, and mock the BYOK HTTP boundary.
- Do not assert on wall-clock timing in tests (flaky); assert on sample counts / buffer lengths instead.
- New behavior ships with a test unless it is purely I/O glue that can only be verified by running the app on real macOS/Windows — say so explicitly when that's the case.

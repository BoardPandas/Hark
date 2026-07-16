---
name: test-scaffold
model: sonnet
effort: medium
description: Generate test files for untested modules. Detects the test framework and creates test stubs matching existing patterns. Use to improve test coverage.
user-invocable: true
argument-hint: [optional: file or directory to generate tests for]
context: fork
allowed-tools:
  - Read
  - Write
  - Edit
  - Glob
  - Grep
  - Agent
---

# Test Scaffolding

You have been asked to generate test files for untested code. Follow these steps.

## Step 1: Detect Test Framework

1. Check `package.json` scripts (`test`, `test:*`) and framework config files first; these are more reliable than dependency lists: `jest.config.*`, `vitest.config.*`, `playwright.config.*`, `pytest.ini`, `tox.ini`, `pyproject.toml` (`[tool.pytest]`), `go.mod`, `Cargo.toml`.
2. Fall back to the dependency manifest to identify the framework:
   - **JavaScript/TypeScript:** Jest, Vitest, Mocha, Playwright, Cypress
   - **Python:** pytest, unittest
   - **Go:** built-in testing package
   - **Rust:** built-in #[test], plus any test crates
3. In a monorepo, detect per package: each workspace may have its own config and framework. Scaffold using the config of the package that owns the source file.
4. **If no test framework is found, stop.** Do not invent conventions. Report that the project has no test setup, recommend a framework appropriate to the stack, and ask the user to confirm before proceeding.
5. Find existing test files using Glob (`**/*.test.*`, `**/*.spec.*`, `**/test_*`, `**/*_test.*`, `**/tests/**`).
6. Read 2-3 existing test files to understand the project's test patterns, conventions, and imports.

## Step 2: Identify Untested Modules

1. List source files using Glob, excluding `node_modules`, `dist`, `build`, `coverage`, `.next`, vendored code, generated files, and the test files themselves.
2. For each source file, check if a corresponding test file exists.
3. Also check if the source file is imported in any test file (it may be tested indirectly).
4. Build a list of source files with no test coverage.
5. On a large repo (roughly 50+ source files in scope), delegate this discovery to an explorer subagent instead of scanning inline: ask it to return the list of untested modules, and say why (you are scaffolding tests and need the untested list without flooding the main context).

## Step 3: Determine Scope

1. If `$ARGUMENTS` contains a file or directory, generate tests only for that scope.
2. If `$ARGUMENTS` is empty, present the list of untested modules and ask the user which ones to scaffold.

## Step 4: Generate Test Files

For each module to test:

1. Read the source file completely.
2. Identify all exported/public functions, classes, and methods.
3. Determine the target test file path following the project's naming convention (e.g., `foo.test.ts` alongside `foo.ts`, or `test_foo.py` alongside `foo.py`).
4. **Never overwrite an existing test file.** If the target already exists, extend it with Edit (append new test cases for uncovered exports only), or skip it and note why in the report.
5. For each exported function or method, generate:
   - A test for the happy path (valid inputs, expected output)
   - A test for edge cases (empty input, null, boundary values)
   - A test for error cases (invalid input, expected exceptions)
6. Write real assertions wherever the expected behavior is inferable from the source. Where it is not, use the framework's native pending mechanism so the suite stays green while marking unfinished stubs: `it.todo`/`test.skip` (Jest/Vitest), `pytest.mark.skip` or `xfail` (pytest), `t.Skip` (Go), `#[ignore]` (Rust). Never generate assertions that fail by design; a red suite breaks CI.
7. Use the same imports, assertion style, and patterns observed in existing test files.
8. Known pitfalls to avoid in generated tests:
   - **Import-time side effects:** if a module touches the DOM, network, or filesystem at import time, importing it in a test will crash or pollute state. Mock the side effect, or flag the module as needing its pure logic split out before it can be unit tested.
   - **Env vars captured at module load:** if the module reads env vars at import time, a static import locks in the wrong values. Set the env in `beforeAll` and use a dynamic `await import()` after.
   - **Path aliases:** if the project uses import aliases, confirm the test runner config resolves them (`resolve.alias` in Vitest, `moduleNameMapper` in Jest) rather than rewriting import paths in the test.

## Step 5: Verify

1. Spawn the **tester** agent to run only the newly created or modified test files. Tell it which files you generated and that you need to confirm they compile/collect and to learn which pass, which are pending, and which error (not to fix anything).
2. If any generated file fails to load (syntax error, unresolvable import, framework misconfiguration), fix it and re-verify. Assertion failures in non-pending tests should be corrected or converted to pending; do not leave the suite red.

## Step 6: Report

```
# Test Scaffold Report

## Files Created / Extended
- <path to test file> (created|extended) tests for <source file> (<count> test cases, <count> pending)

## Verification
- Command: <exact command the tester agent ran>
- Verdict: <n> passed, <n> pending/skipped, <n> failed

## Coverage Summary
- Modules without tests before: <count>
- Test files generated: <count>
- Test cases generated: <count>

## Next Steps
1. Fill in test logic for cases marked pending (todo/skip).
2. Add additional edge case tests as needed.
```

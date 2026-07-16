---
name: dependency-audit
model: sonnet
effort: medium
description: Audit project dependencies for outdated versions, known vulnerabilities, and unused packages, plus stale runtimes and package managers (Node, npm, pnpm). Use periodically or before releases.
user-invocable: true
argument-hint: [optional: directory to scope the audit, e.g. a monorepo workspace]
allowed-tools:
  - Read
  - Glob
  - Grep
  - Bash
  - WebFetch
---

# Dependency Audit

You have been asked to audit this project's dependencies. Follow these steps.

**Exit-code note:** audit and outdated commands (`npm audit`, `npm outdated`, `pip list --outdated`, `pnpm audit`, and similar) exit non-zero when they find vulnerabilities or outdated packages. A non-zero exit with JSON output is the success path, not a failure. Parse the output; do not retry or report the command as broken.

## Step 1: Determine Scope and Identify Package Managers

1. If the user specified a directory, scope the audit to that path (useful for a single monorepo workspace). Otherwise audit the repo root and subdirectories.
2. Search the scope for dependency manifests:

- `package.json` (npm/yarn/pnpm/bun; check lockfiles to identify which: `package-lock.json`, `yarn.lock`, `pnpm-lock.yaml`, `bun.lock`/`bun.lockb`, or the `packageManager` field in package.json)
- `requirements.txt`, `Pipfile`, `pyproject.toml` (Python)
- `go.mod` (Go)
- `Cargo.toml` (Rust)
- `Gemfile` (Ruby)
- `pom.xml`, `build.gradle` (Java/Kotlin)
- `composer.json` (PHP)

Read each manifest found.

## Step 2: Check for Vulnerabilities

For each package manager detected, run the appropriate audit command:

- **npm:** `npm audit --json --omit=dev` for production dependencies, then `npm audit --json` for the full picture (requires package-lock.json). Report prod and dev-only findings separately; prod findings are the priority.
- **yarn:** Yarn 1 (Classic): `yarn audit --json`. Yarn 2+ (Berry): `yarn npm audit --json` (add `--environment production` for the prod-scoped pass). Determine the version with `yarn --version` or the `packageManager` field.
- **pnpm:** `pnpm audit --json` (add `--prod` for the prod-scoped pass)
- **pip:** `pip-audit --format json` (pip-audit is a separate package, not a pip subcommand)
- **cargo:** `cargo audit` (if installed)
- **go:** `govulncheck ./...` (if installed)
- **ruby:** `bundler-audit check --update` (if installed)
- **composer:** `composer audit` (native since Composer 2.4)

If an audit tool is not installed, do not install it without asking. Instead, fall back to the OSV.dev API: for each direct dependency, WebFetch `https://api.osv.dev/v1/query` semantics via a POST with `{"package": {"name": "<pkg>", "ecosystem": "<npm|PyPI|Go|crates.io|RubyGems|Maven|Packagist>"}, "version": "<version>"}` (use `curl -s -X POST -d '<json>' https://api.osv.dev/v1/query` via Bash if WebFetch cannot POST). Query at least the direct dependencies; note in the report that transitive dependencies were not covered by the fallback.

## Step 3: Check for Outdated Packages

For each package manager:

- **npm:** `npm outdated --json`
- **yarn:** Yarn 1: `yarn outdated`. Yarn 2+: `yarn upgrade-interactive` is interactive, so use `yarn outdated` via the plugin if available, otherwise fall back to `npm outdated --json`.
- **pnpm:** `pnpm outdated --json`
- **pip:** `pip list --outdated --format=json`
- **cargo:** `cargo outdated` (if installed)
- **go:** `go list -u -m all` (modules with an update show the newer version in brackets)
- **ruby:** `bundle outdated`
- **maven:** `mvn versions:display-dependency-updates` (if mvn is available)
- **composer:** `composer outdated`

Categorize updates as:
- **Patch:** Bug fixes, safe to update
- **Minor:** New features, backward-compatible
- **Major:** Breaking changes, requires migration

## Step 4: Check Toolchain Versions

The runtimes and package managers themselves go stale too. For each ecosystem detected:

1. Get installed versions: `node --version`, `npm --version`, `pnpm --version`, `yarn --version`, `python --version`, `go version`, `rustc --version`, as applicable.
2. Get current versions (do not answer from memory; releases move fast):
   - **Node:** WebFetch `https://nodejs.org/dist/index.json` and find the newest LTS entry (`lts` field is non-false). Flag if the installed major is past end-of-life or behind the active LTS.
   - **npm/pnpm/yarn:** `npm view npm version`, `npm view pnpm version`, `npm view yarn version` (registry queries, no install needed).
   - **Other runtimes:** WebFetch the official release page or use the ecosystem's own check (e.g. `rustup check`).
3. Compare against any version pins in the repo: `engines` field in package.json, `packageManager` field, `.nvmrc`, `.node-version`, `.tool-versions`, `volta` config. Flag mismatches between the pin, the installed version, and current (e.g. a pin to an EOL Node major).

## Step 5: Detect Unused Dependencies

1. Read the dependency manifest.
2. For each dependency listed, use Grep to search the codebase for imports or references.
3. If a dependency has zero references, flag it as potentially unused.
4. Before flagging, check for indirect usage that produces false positives:
   - Plugins, presets, and peer dependencies referenced by string in config files
   - `@types/*` packages (used by the TypeScript compiler, never imported)
   - CLI-only tools invoked from package.json scripts or CI config
   - Side-effect imports and framework conventions (e.g. middleware loaded by name)
5. Verify against the lockfile before recommending removal, and phrase removals as "verify then remove," never as a certainty.

## Step 6: Produce Report

```
# Dependency Audit Report

## Summary
- Manifests found: <list>
- Toolchain: <ok, or list of stale/EOL runtimes and package managers>
- Vulnerabilities: <count by severity, prod vs dev-only>
- Outdated packages: <count by type>
- Potentially unused: <count>

## Toolchain
- <tool> <installed> vs <current/LTS>: <ok | outdated | EOL | mismatch with pin in <file>>

## Vulnerabilities (production dependencies)
[CRITICAL/HIGH/MEDIUM/LOW] <package>@<version>
  CVE: <id if available>
  Description: <what the vulnerability is>
  Fix: Update to <safe version>

## Vulnerabilities (dev-only dependencies)
(same format; lower priority, note if exploitable only at build time)

## Outdated Packages
[MAJOR] <package> <current> -> <latest> (breaking changes likely)
[MINOR] <package> <current> -> <latest>
[PATCH] <package> <current> -> <latest>

## Potentially Unused
- <package>: no imports found (verify against lockfile and scripts before removing)

## Recommendations
1. <prioritized list of actions>
```

Note in the report any ecosystems where the audit tool was missing and the OSV fallback was used, and any coverage gaps (e.g. transitive dependencies not checked).

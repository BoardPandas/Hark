---
name: dependency-audit
description: "Audit project dependencies for outdated versions, known vulnerabilities, and unused packages, plus stale runtimes and package managers (Node, npm, pnpm) and GitHub Actions (action versions, runner images, and the tool versions workflows install). Use periodically or before releases."
---

> Generated from `.claude/skills/dependency-audit/SKILL.md` by `scripts/sync-antigravity.mjs --with-skills`. Edit the source, then run `node scripts/sync-antigravity.mjs --with-skills`.

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

## Step 5: Audit GitHub Actions

Skip this step only if the scope has no `.github/workflows/*.yml|yaml`, no `.github/actions/**/action.yml|yaml`, and no root `action.yml` (the repo is itself an action). Package audits never see CI: an action, a runner image or a tool a workflow installs goes stale or vulnerable without any manifest changing. Use `gh api` for every lookup (authenticated, 5,000 requests an hour); without `gh`, WebFetch `https://api.github.com/...` (60 an hour unauthenticated, so batch and dedupe first). Do not answer from memory: "is `actions/checkout@v7` the newest?" is answered by the API, never by recall.

### 5a. Inventory

Read every workflow, every local composite action (`.github/actions/**/action.yml`), and any root `action.yml`. Collect:

- Every `uses:`, at step level (actions) and at job level (reusable workflows, `owner/repo/.github/workflows/x.yml@ref`). Follow local `./` refs into their `action.yml` and collect their `uses:` too. `docker://image:tag` refs are container images (5e).
- Every `runs-on:`, including the values behind `${{ matrix.* }}` and arrays such as `[self-hosted, linux]`.
- Every tool version a workflow pins: `setup-*` inputs (`node-version`, `node-version-file`, `python-version`, `go-version`, `java-version`, `dotnet-version`, pnpm/action-setup `version`, `toolchain:` on rust-toolchain, and the equivalent input on any other setup action), plus versions installed in `run:` steps (`npm i -g pkg@1.2.3`, `pip install pkg==1.2.3`, `curl .../v1.2.3/...` installers, `go install pkg@v1.2.3`).
- Every container image: `container:`, `services:<name>.image`, `docker://` refs, and images pulled or built in `run:` steps.

Dedupe by `owner/repo` before querying; one repo can reference the same action dozens of times.

### 5b. Action versions

For each distinct `owner/repo[/path]@ref` (skip `./` refs, already followed):

1. **Resolve what is pinned.**
   - Tag (`@v7`, `@v7.0.1`): that version. A major-only tag floats to the newest release in that major.
   - Full 40-character SHA: read the trailing `# vX.Y.Z` comment, then confirm the SHA really is that tag: `gh api repos/<owner>/<repo>/git/matching-refs/tags/<tag> --jq '.[] | "\(.ref) \(.object.type) \(.object.sha)"'`. For an annotated tag (`object.type` is `tag`), dereference with `gh api repos/<owner>/<repo>/git/tags/<sha> --jq .object.sha`. No comment, or a comment that does not match, is a finding: name the tag the SHA actually is.
   - Branch (`@main`, `@master`, `@stable`) or short SHA: unpinned, a finding on its own. Report what it resolves to today.
2. **Get the latest.** `gh api repos/<owner>/<repo>/releases/latest --jq '.tag_name + " " + .published_at'`. If the repo has no releases, list tags (`gh api repos/<owner>/<repo>/tags --paginate --jq '.[].name'`) and sort them by semver yourself; the API does not return them in version order. Also run `gh api repos/<owner>/<repo> --jq .archived`: an archived action gets no fixes and must be replaced.
3. **Check advisories.** `gh api "/advisories?ecosystem=actions&affects=<owner>/<repo>"` and compare each `vulnerable_version_range` to the pinned version. A pinned version inside a range is a vulnerability: report it with the advisories from Step 2, not as an outdated item.
4. **Check the action's runtime.** Read `runs.using` from the action's `action.yml` at the pinned ref (`gh api "repos/<owner>/<repo>/contents/action.yml?ref=<ref>" -H "Accept: application/vnd.github.raw"`, or `action.yaml`). A `node*` value GitHub has deprecated for actions is a finding even when the action is otherwise current enough to run: check the current deprecation on the GitHub changelog (`https://github.blog/changelog/`) or the `actions/runner` release notes, not from memory.
5. **Classify** like packages: MAJOR (latest major is higher, e.g. `@v4` while v7 is out), MINOR, PATCH. A floating major tag on the latest major (`@v7` while v7.0.1 is latest) is current; say it floats. A SHA pin behind the latest release in the same major is a PATCH or MINOR update, not "current".
6. **Flag inconsistency.** The same action at different versions across this repo's workflows (`actions/checkout@v4` in one file, `@v7` in another) is a finding: bring them to one version.
7. **Note the pinning policy, do not enforce it.** Report whether third-party (non-`actions/*`, non-`github/*`) actions are pinned to full SHAs. Recommend SHA pins for third-party actions; a mutable tag on a third-party action is the supply-chain risk, but leave the decision to the user.

### 5c. Runner images

1. Get the current GitHub-hosted labels: `gh api repos/actions/runner-images/contents/README.md -H "Accept: application/vnd.github.raw"`, the "Available Images" table. It shows which version each `-latest` label points to and marks deprecated images.
2. Get pending changes: `gh issue list -R actions/runner-images --label Announcement --state open --limit 30 --json title,createdAt`. These carry the `-latest` migrations and retirement dates.
3. For each `runs-on` label, report one of:
   - **Retired or deprecated** (in the table as deprecated, or an announcement gives a retirement date): a finding, with the date and the replacement label. A deprecated image gets scheduled brownouts before removal, so jobs fail intermittently first.
   - **Pinned behind the newest GA** (e.g. `ubuntu-24.04` while `ubuntu-26.04` is GA): MINOR, informational. Pinning is fine; say what is newer.
   - **`-latest` with a pending migration** (an announcement says it moves to a new version soon): name the date, and recommend one run with the explicit new label to catch breakage before the switch.
   - **Current.**
4. **Self-hosted runners** (any label set containing `self-hosted`): the image is not visible from the repo. List the labels and the workflows that use them, report the newest runner app version (`gh api repos/actions/runner/releases/latest --jq .tag_name`), and tell the user to compare it with `./run.sh --version` (or `run.cmd --version`) on each host. GitHub stops accepting runners that fall too far behind, and auto-update is off when a runner was configured with `--disableupdate`.

### 5d. Toolchains inside CI

Compare every tool version collected in 5a against the current versions from Step 4 and against the repo's own pins (`.nvmrc`, `engines`, `packageManager`, `.tool-versions`, `rust-toolchain.toml`, `go.mod` `go` and `toolchain` lines). Report:

- A CI version that is EOL or behind current (e.g. `node-version: 18`).
- **Drift between CI and the repo**: CI tests a different version than the repo pins or than developers run (e.g. CI on Node 20 while `.nvmrc` says 24). Prefer `node-version-file` (or the setup action's equivalent) so the pin has one source.
- A pinned tool installed in a `run:` step that is behind its latest release (check with `npm view <pkg> version`, PyPI JSON `https://pypi.org/pypi/<pkg>/json`, or the tool's GitHub releases).

### 5e. Container images

For each image from 5a:

- A `:latest` tag or no tag: unpinned, a finding.
- A version tag: compare with the newest tag. Docker Hub: `https://hub.docker.com/v2/repositories/<namespace>/<name>/tags?page_size=100&ordering=last_updated` (official images use the `library` namespace). GHCR or another registry: the publishing repo's releases. Flag an EOL base version (e.g. `postgres:13`, `node:18`).

### 5f. Automated update coverage

Read `.github/dependabot.yml` (or `renovate.json`, `.github/renovate.json`, `renovate.json5`). If no `package-ecosystem: "github-actions"` entry (Renovate: `github-actions` manager enabled, the default) covers `/`, recommend adding one so action updates arrive between audits. Note what it does not cover: Dependabot never updates `runs-on` labels, and versions in `run:` steps or setup-action inputs are only updated by Renovate with a custom manager, so those stay manual.

## Step 6: Detect Unused Dependencies

1. Read the dependency manifest.
2. For each dependency listed, use Grep to search the codebase for imports or references.
3. If a dependency has zero references, flag it as potentially unused.
4. Before flagging, check for indirect usage that produces false positives:
   - Plugins, presets, and peer dependencies referenced by string in config files
   - `@types/*` packages (used by the TypeScript compiler, never imported)
   - CLI-only tools invoked from package.json scripts or CI config
   - Side-effect imports and framework conventions (e.g. middleware loaded by name)
5. Verify against the lockfile before recommending removal, and phrase removals as "verify then remove," never as a certainty.

## Step 7: Produce Report

```
# Dependency Audit Report

## Summary
- Manifests found: <list>
- Toolchain: <ok, or list of stale/EOL runtimes and package managers>
- GitHub Actions: <n workflows; actions outdated by type; vulnerable; runner labels deprecated/behind; CI toolchain drift; or "no workflows">
- Vulnerabilities: <count by severity, prod vs dev-only>
- Outdated packages: <count by type>
- Potentially unused: <count>

## Toolchain
- <tool> <installed> vs <current/LTS>: <ok | outdated | EOL | mismatch with pin in <file>>

## GitHub Actions
### Actions
[MAJOR/MINOR/PATCH/CURRENT] <owner/repo> <pinned (tag, or SHA = tag)> -> <latest> (<released date>) in <workflow files>
  <extra: vulnerable (GHSA id) | archived | deprecated runtime nodeNN | SHA comment wrong | unpinned branch | inconsistent across files>
### Runners
- <label>: <current | behind newest GA <label> | deprecated, retires <date>, move to <label> | -latest moves to <version> on <date>> (<workflow files>)
- self-hosted <labels>: manual check, latest runner app <version>
### CI toolchains
- <tool> <CI version> vs <current> vs repo pin <file:version>: <ok | outdated | EOL | drift>
### Container images
- <image:tag> -> <latest tag>: <ok | outdated | EOL | unpinned>
### Update automation
- Dependabot/Renovate github-actions: <configured | missing>; not covered: <runner labels, run-step installs, ...>

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

Note in the report any ecosystems where the audit tool was missing and the OSV fallback was used, and any coverage gaps (e.g. transitive dependencies not checked, self-hosted runners not inspected, GitHub API rate limit reached before every action was checked).

In Recommendations, rank a vulnerable or archived action and a deprecated or retiring runner label with production vulnerabilities, ahead of ordinary updates: they break or expose CI on a date you do not choose.

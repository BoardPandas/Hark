# BP Audit Results

Date: 2026-07-15
Score: 4/4 FOUNDATIONAL (100%) — RECOMMENDED items mostly deferred until the Cargo project is scaffolded.

Context: Hark is an offline **Rust desktop app**, not a web app. Web-oriented BP practices (deployment, database/Postgres, monorepo JS tooling, design-systems for web component libraries) do **not apply** and are excluded from scoring.

## Passing Practices

### FOUNDATIONAL
- [x] `claude-config/hierarchical-claude-md` — root `CLAUDE.md` (project-wide) + planned per-crate files + `.claude/rules/*` path-scoped.
- [x] `claude-config/byte-budget-claude-md` — root `CLAUDE.md` is lean (~130 lines); detail flows down to `tasks/plan-repo.md`, `design-guardrails.md`, and agent-memory.
- [x] `claude-config/rule1-llg-integration` — RULE 1 in `CLAUDE.md`; `.claude/rules/llg-check.md`; `SessionStart` + `EnterPlanMode` KB-check hooks.
- [x] `safety/read-only-first-rule` — RULE 0 present in `CLAUDE.md`.

### RECOMMENDED (already satisfied)
- [x] `claude-config/path-scoped-rules` — `.claude/rules/` with `paths:` frontmatter (rust.md, tests.md, bp-check.md, llg-check.md, commit-changelog.md).
- [x] `claude-config/hook-configuration` — hooks wired in `settings.json` (KB reminder, commit-message/changelog checks, sounds).
- [x] `claude-config/skill-frontmatter` — skills carry model/effort/context frontmatter.
- [x] Credential deny-list present in `settings.json` (~/.ssh, ~/.aws, ~/.azure, ~/.kube, git-credentials, etc.).
- [x] `documentation/plan-with-lessons-learned` — plans must end with Lessons Learned; `tasks/plan-repo.md` does.
- [x] `versioning/*` — `.claude/rules/commit-changelog.md` enforces Keep-a-Changelog + SemVer bump per commit.
- [x] `safety/secretless-third-party-writes` (desktop/CLI, applies-to match) — satisfied by design: no secrets shipped in the binary or `config.toml`; the BYOK key lives only in the OS keychain (`keyring`).

## Failing / Deferred Practices (address when scaffolding the Cargo project)

### RECOMMENDED
- [ ] `testing/*` — no tests yet (greenfield, no code). Add with Phase 1; conventions captured in `.claude/rules/tests.md`.
- [ ] `linting-formatting/*` — add `rustfmt.toml` + `clippy.toml` (and consider `deny.toml` via `cargo-deny`) when `Cargo.toml` is created. `bp-check.md` already scopes these paths.
- [ ] `error-handling/*`, `validation/*` — conventions stated in `CLAUDE.md`; verify in practice once pipeline code exists.

## Note

Fix failing/deferred practices one at a time with `/apply-practice <slug>`. Re-running `/init-repo` refreshes this audit. Practices excluded as non-applicable (web-only): deployment, database (Postgres), monorepo JS tooling, environment (.env), design-systems (web component libraries), notifications, user-feedback.

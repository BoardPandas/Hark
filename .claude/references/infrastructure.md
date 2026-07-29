# Infrastructure — Not Applicable to Hark

**Hark has no infrastructure. Do not provision any.**

Hark is a single-user **native desktop app** for Windows + macOS, written in Rust
and shipped as a signed binary + Inno Setup installer through
[`.github/workflows/release.yml`](../../.github/workflows/release.yml). There is
no web frontend, no server, no database service, no auth service, no object
store, no cron, and no hosting platform of any kind.

The bootstrap template this repo came from ships a fixed hosting stack here
(Northflank + Cloudflare + Better Auth + Postgres + Redis + R2 + Resend). That
content has been removed rather than merely disclaimed: it was live, plausible,
detailed prose sitting in `.claude/references/`, one `Read` away from any
subagent, and the only thing marking it inapplicable was a single line in the
root `CLAUDE.md` that a subagent reading this file directly never sees.

If you were sent here by `.claude/skills/plan-repo/SKILL.md` ("Read
`.claude/references/infrastructure.md` FIRST — the hosting stack is fixed and
non-negotiable"): that instruction is inherited from the template and does not
apply. Skip the infrastructure section entirely and plan the desktop app.

## What stands in for "infrastructure" here

| Concern | Hark's answer |
|---|---|
| Hosting | None. The binary runs on the user's machine. |
| Data | Local only: `rusqlite` (history + stats), TOML (settings + spellbook). |
| Secrets | The user's own BYOK provider key, in the OS keychain via `keyring`. Never in config, never logged. |
| External calls | One HTTPS POST per utterance to the user's chosen STT provider; optional cleanup call to their LLM endpoint. Nothing else leaves the machine. |
| Distribution | GitHub Releases: signed `hark-app.exe` + Inno Setup installer, tag-triggered. |
| CI | [`.github/workflows/ci.yml`](../../.github/workflows/ci.yml) — fmt, clippy, tests, Claude wiring guard. |

The template's original hosting reference still lives in the upstream
claude-code-bootstrap template if a future *web* project needs it. Per
`.claude/skills/update-practices/SKILL.md`, this file is never synced, so this
replacement is stable.

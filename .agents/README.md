# Antigravity configuration (generated)

Everything generated under `.agents/` comes from `.claude/` via `scripts/sync-antigravity.mjs --with-skills`.
Edit the Claude source, never these generated files, then run:

```bash
node scripts/sync-antigravity.mjs --with-skills
```

| Claude Code source | Antigravity output |
|---|---|
| `.claude/skills/<name>/**` | `.agents/skills/<name>/**` (`SKILL.md` <= 24000 B) |
| `.claude/agents/<name>.md` | `.agents/agents/<name>.md` |
| `.claude/rules/*.md` | `.agents/rules/*.md` (`glob` / `always_on`) |
| `.claude/settings.json` hooks + deny rules | `.agents/hooks.json` via `.claude/scripts/_antigravity-hook.sh` |

Current output: 18 skills, 8 agents, 5 rules, 12 hook handlers (existing AGENTS.md preserved).

## How the Antigravity bridge works

- **Models.** Claude tiers map to Antigravity tiers: `opus` -> `inherit`, `sonnet` -> `inherit`, `haiku` -> `flash`.
- **Command sandbox.** Agents needing shell or CLI search/fetch (`Bash`, `Glob`, `Grep`, `WebFetch`) get `commandExecutionPolicy: sandbox`; agents needing none get `commandExecutionPolicy: "off"`.
- **Hooks adapter (`_antigravity-hook.sh`).** Antigravity runs hooks with `.agents/` as cwd via `sh -c` (Unix) or `cmd /c` (Windows), so commands invoke `bash ../.claude/scripts/_antigravity-hook.sh` without POSIX `$(...)`. The adapter handles both modern CLI/ACP tool names (`write_file`, `edit_file`, `read_file` with `Path`) and legacy IDE names (`write_to_file`, `replace_file_content`, `view_file` with `TargetFile`/`AbsolutePath`), reads top-level `cwd`, enforces `.claude/settings.json` `permissions.deny` via `--enforce-deny`, converts `exit 2` + stderr into `{"decision":"deny","reason":"..."}` (exit 0), and flushes queued `PreToolUse`/`PostToolUse` advice once on the next `PreInvocation` as `systemMessage`.

## Not carried over to Antigravity

- PreToolUse [EnterPlanMode|ExitPlanMode]: bash "$CLAUDE_PROJECT_DIR"/.claude/scripts/pre-plan-kb-check.sh -- Antigravity has no hookable EnterPlanMode/ExitPlanMode tool.
- Stop/Notification terminal bell -- Antigravity hooks require JSON on stdout.

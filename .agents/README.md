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
| `.claude/settings.json` hooks | `.agents/hooks.json` via `.claude/scripts/_antigravity-hook.sh` |

Current output: 18 skills, 8 agents, 5 rules, 9 hook handlers (existing AGENTS.md preserved).

## How the Antigravity bridge works

- **Models.** Claude tiers map to Antigravity tiers: `opus` -> `pro`, `sonnet` -> `pro`, `haiku` -> `flash`.
- **Command sandbox.** Agents without `Bash` get `commandExecutionPolicy: "off"`; agents with `Bash` get `commandExecutionPolicy: sandbox`.
- **Hooks adapter (`_antigravity-hook.sh`).** Antigravity runs hooks with `.agents/` as cwd, sends camelCase JSON (`toolCall.args.CommandLine`, `TargetFile`, `Cwd`), and treats non-zero exits as fail-open errors. The adapter anchors at the git root, converts `exit 2` + stderr into `{"decision":"deny","reason":"..."}` (exit 0), emits `SessionStart` text as `systemMessage`, and queues `PreToolUse`/`PostToolUse` `additionalContext` per `conversationId` so `PreInvocation` injects it as `ephemeralMessage` on the next step.

## Not carried over to Antigravity

- PreToolUse [EnterPlanMode|ExitPlanMode]: bash .claude/scripts/pre-plan-kb-check.sh -- Antigravity has no hookable EnterPlanMode/ExitPlanMode tool.
- Stop/Notification terminal bell -- Antigravity hooks require JSON on stdout.

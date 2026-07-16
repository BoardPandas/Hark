# Hooks & Settings Reference

Canonical catalog of Claude Code hook events, hook types, matcher syntax, and
`settings.json` options. The `init-repo` and `update-practices` skills both point
here so this list is maintained in exactly one place (it used to be duplicated in
both skills, which let the two copies drift).

> Verify against the current Claude Code release when you use this. The event and
> settings catalogs grow over time; fetch the official docs (see
> `.claude/references/source-urls.md`) and add anything new here rather than into
> a skill body.

## Hook events

| Event | Fires When | Use Cases |
|-------|-----------|-----------|
| **SessionStart** | When a new session begins | Welcome message, status check, re-inject context after compaction (matcher: `compact`) |
| **SessionEnd** | When a session ends | Save state, create handoff doc |
| **UserPromptSubmit** | When user submits a prompt | Input validation, prompt logging |
| **UserPromptExpansion** | When a slash command expands | Inspect/rewrite expanded command, block disallowed commands |
| **PreToolUse** | Before any tool call | Validate tool args, block dangerous commands, log activity |
| **PostToolUse** | After any tool call completes | Post-processing, validation of results, auto-lint |
| **PostToolUseFailure** | When a tool call fails | Error logging, fallback actions, retry logic |
| **PostToolBatch** | After a parallel tool batch resolves | Aggregate batch results, enforce post-batch invariants, block |
| **PermissionRequest** | When a tool requests permission | Auto-approve safe reads, log permission decisions |
| **PermissionDenied** | When a permission request is denied | Audit logging, suggest alternative paths |
| **SubagentStart** | When a subagent launches | Log subagent activity, resource tracking |
| **SubagentStop** | When a subagent completes | Aggregate results, trigger follow-up tasks |
| **Stop** | When Claude finishes a response | Notification sounds, auto-formatting, status updates |
| **StopFailure** | When a turn ends due to an API error | Error alerting, failure logging |
| **Notification** | When Claude sends a notification, including background-agent events (`agent_needs_input`, `agent_completed`) | Alert sounds, desktop notifications, webhook pings |
| **MessageDisplay** | As assistant message text is displayed | Transform or hide message text, redact secrets in output |
| **PreCompact** | Before context compaction (matcher: `manual` or `auto`) | Save important state, create summaries |
| **PostCompact** | After context compaction completes | Re-inject context, verify state |
| **TeammateIdle** | When a teammate agent is idle | Coordination, load balancing |
| **TaskCreated** | When a background task is created | Task tracking, resource planning |
| **TaskCompleted** | When a background task completes | Status updates, follow-up actions |
| **InstructionsLoaded** | When a CLAUDE.md or rules file loads | Audit logging, rule tracking |
| **ConfigChange** | When settings or skill files change | Audit logging, reload triggers |
| **WorktreeCreate** | When an isolated worktree is created | Setup worktree-specific config |
| **WorktreeRemove** | When a worktree is cleaned up | Cleanup, merge results |
| **CwdChanged** | When the working directory changes | Reload directory-scoped config |
| **FileChanged** | When a watched file changes on disk | Reload triggers, external-edit detection |
| **Elicitation** | When an MCP server requests structured input | Auto-fill known values, log requests |
| **ElicitationResult** | After an MCP elicitation is answered | Post-process structured input |
| **Setup** | On `--init`, `--init-only`, or `--maintenance` flags | One-time project setup, maintenance tasks |

## Hook types

1. **Command hooks**: `{ "type": "command", "command": "..." }` — Runs a shell command. Exit code 0 = allow, 2 = block (PreToolUse), non-zero = error.
2. **HTTP hooks**: `{ "type": "http", "url": "https://..." }` — Sends an HTTP POST to a URL. The request body contains the event payload. Requires the URL to be listed in `settings.json` under `allowedHttpHookUrls`. Supports custom headers with env-var interpolation, e.g. `"headers": { "Authorization": "Bearer ${MY_WEBHOOK_TOKEN}" }`.
3. **Prompt hooks**: `{ "type": "prompt", "prompt": "..." }` — Single-turn LLM judgment (yes/no decision). Useful for validation gates.
4. **Agent hooks**: `{ "type": "agent", "prompt": "..." }` — Multi-turn subagent with tool access. Useful for complex validation or post-processing.
5. **MCP tool hooks**: `{ "type": "mcp_tool", "tool": "...", "arguments": {...} }` — Directly invokes an MCP tool as the hook action. Useful for posting to integrated services without a shell.

Any hook entry accepts an optional `if:` field using permission-rule syntax (e.g., `Bash(git *)`) so the hook fires only on matching tool calls — reduces overhead on unrelated calls.

## Hook output capabilities

Hooks can return structured output (`hookSpecificOutput`) to influence the session, not just allow/block:

- **PostToolUse** — `hookSpecificOutput.updatedToolOutput` rewrites the tool's output for ANY tool before Claude sees it (redaction, normalization, annotation).
- **Stop / SubagentStop** — `hookSpecificOutput.additionalContext` feeds text back and continues the turn instead of ending it (self-review loops, "did you run the tests?" nudges).
- **SessionStart** — `reloadSkills: true` rescans skill directories mid-session; `hookSpecificOutput.sessionTitle` sets the session title.

## Matcher syntax

- `Bash(pattern)` — matches Bash tool calls where the command matches the glob pattern
- `Write(pattern)` — matches Write tool calls where the file path matches
- `Edit(pattern)` — matches Edit tool calls where the file path matches
- `Read(pattern)` — matches Read tool calls where the file path matches
- `Tool(param:value)` — matches on a tool input parameter, e.g. `Agent(model:opus)` matches Agent calls that request Opus. Works in permission rules too (deny/allow lists), not just hook matchers.
- No matcher = fires for all tool calls of that event type

## Hooks to configure based on project needs

**Always configure:**
- `SessionStart` — surface the LL-G / BP knowledge-base reminder once per session
- `PreToolUse` with `Bash(git commit*)` matcher — validation before commits
- `Stop` — notification sound (use `printf '\a'`, not `echo '\a'` — `echo` prints a literal `\a` in most shells)
- `Notification` — notification sound

**Recommended for active development:**
- `PostToolUse` with `Write(*)` or `Edit(*)` matcher — auto-lint after file changes (if linter is configured)
- `PreToolUse` with `Bash(rm -rf*)` matcher — block dangerous delete commands
- `SubagentStop` — notification when long-running subagents complete

**Recommended for team projects using HTTP hooks:**
- `Stop` with HTTP hook — ping team webhook (Slack, Discord) when Claude finishes a task
- `StopFailure` with HTTP hook — send error reports to monitoring

**Cost/friction saver:**
- `PermissionRequest` with a `prompt` hook — route the permission decision to a single-turn LLM judge that auto-approves known-safe operations (read-only commands, project-local writes) and defers everything else to the user. Cuts prompt fatigue without widening the static allow list.

Ask the user which additional hooks they want before configuring beyond the defaults.

## settings.json core settings (always configure)

```json
{
  "$schema": "https://json.schemastore.org/claude-code-settings.json",
  "permissions": {
    "allow": ["Read", "Glob", "Grep", "WebFetch", "WebSearch"],
    "deny": [
      "Read(~/.ssh/**)",
      "Read(~/.aws/**)",
      "Read(~/.azure/**)",
      "Read(~/.kube/**)",
      "Read(~/.docker/config.json)",
      "Read(~/.npmrc)",
      "Read(~/.git-credentials)",
      "Read(~/.config/gh/**)",
      "Edit(~/.bashrc)",
      "Edit(~/.zshrc)",
      "Edit(~/.profile)"
    ]
  },
  "env": { "ENABLE_TOOL_SEARCH": "true" },
  "plansDirectory": "tasks",
  "hooks": { }
}
```

## Optional settings

| Setting | Purpose | When to enable |
|---------|---------|---------------|
| `attribution.commit` | Add "Generated by Claude Code" to commit messages | Team projects for audit trail |
| `attribution.pr` | Add Claude attribution to PR descriptions | Team projects for transparency |
| `autoUpdatesChannel` | `"stable"` or `"preview"` for Claude Code updates | `"stable"` for production repos, `"preview"` for template/experimental repos |
| `sandbox.permissions` | Sandboxed execution permissions for tools | When running untrusted code analysis |
| `sandbox.network` | Network access restrictions in sandbox | Security-sensitive projects |
| `worktree.bgIsolation` | `"none"` lets background sessions edit the working copy directly instead of an isolated worktree | Background-agent workflows that should not branch |
| `worktree.baseRef` | `"fresh"` branches worktrees from `origin/<default>`, `"head"` from local `HEAD` | Control where isolated worktrees branch from |
| `language` | Preferred response language (e.g., `"en"`, `"ja"`) | Non-English teams |
| `allowedHttpHookUrls` | Allowlist of URLs for HTTP hooks | When using HTTP hooks for webhooks |
| `alwaysThinkingEnabled` | Always use extended thinking | Complex codebases that benefit from deeper reasoning |
| `disableAllHooks` | Kill switch for all hooks | For `settings.local.json` — lets individuals disable hooks locally |
| `defaultMode` | Default permission mode. Note: the value `"default"` was renamed to `"manual"` in v2.1.200, and Manual is now the out-of-box default | Set explicitly if the team wants `acceptEdits`/`plan` as the session default |
| `fallbackModel` | Fallback model(s) when the primary is unavailable — now accepts a list of up to 3 | Resilience for CI/automation sessions |
| `enforceAvailableModels` | Restrict which models sessions may select | Org cost-control or compliance |
| `disableBundledSkills` | Turn off Claude Code's built-in bundled skills | When bundled skills collide with project skills |
| `requiresMinimumVersion` / `requiredMaximumVersion` | Pin the Claude Code version range for the repo | Teams that need reproducible harness behavior |
| `attribution.sessionUrl` | Include the session URL in attribution output | Audit trails that link commits back to sessions |
| `autoMode.classifyAllShell` / `autoMode.idleTimeout` | Tune auto-mode shell classification and idle behavior | Heavy auto-mode users |

Env levers worth knowing: `ENABLE_TOOL_SEARCH` (lazy-load MCP tool schemas; also accepts `auto:N`) and `ENABLE_PROMPT_CACHING_1H` (opt into the 1-hour prompt-cache TTL for long sessions). Niche display settings (`wheelScrollAccelerationEnabled`, `footerLinksRegexes`, `respondToBashCommands`, `pluginSuggestionMarketplaces`, `allowAllClaudeAiMcps`) exist but rarely belong in a shared template.

Security note (v2.1.196): MCP servers declared in a committed `.claude/settings.json` / `.mcp.json` no longer auto-spawn without user approval — do not design workflows that assume a cloned repo's MCP servers start automatically.

## settings.json vs settings.local.json

- **`.claude/settings.json`** — Version-controlled, shared team settings. Put everything the team agrees on here.
- **`.claude/settings.local.json`** — Git-ignored, personal overrides. Document this in instructions.md so developers know they can create it.

Full precedence chain (highest wins): `managed-settings.json` (MDM/org policy) > CLI flags > `.claude/settings.local.json` > `.claude/settings.json` > `~/.claude/settings.local.json` > `~/.claude/settings.json`. Deny permission rules always win regardless of tier. Any setting can also be set inline with `/config key=value`.

Recommended `.claude/settings.local.json.example` showing common personal overrides:
```json
{
  "disableAllHooks": false,
  "alwaysThinkingEnabled": true,
  "language": "en"
}
```

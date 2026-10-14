#!/usr/bin/env bash
# Shared output helper for ADVISORY hooks on tool events. Source it, do not
# execute it:
#
#   . "$(dirname "${BASH_SOURCE[0]}")/_hook-context.sh"
#   emit_context PreToolUse "$MSG"
#
# Plain stdout from a hook reaches the model only for SessionStart,
# UserPromptSubmit, UserPromptExpansion and PostModelSwitch. For every other
# event -- PreToolUse and PostToolUse included -- Claude Code writes plain stdout
# to the debug log. An advisory hook that echoes its reminder and exits 0 runs,
# looks healthy, and is never once read. Those events need the reminder as JSON:
#
#   {"hookSpecificOutput":{"hookEventName":"PreToolUse","additionalContext":"..."}}
#
# (code.claude.com/docs/en/hooks, verified against Claude Code 2.1.292.) Codex,
# which runs these same scripts from .codex/hooks.json, applies the same rule and
# reads the same shape (codex-rs/hooks/src/events/pre_tool_use.rs).
#
# Blocking is a different channel: exit 2 with the reason on stderr. Never route a
# refusal through here.
#
# Pure bash on purpose. The JSON must come out right on a machine where node and
# python are missing or are Windows Store stubs (see _json-parser.sh), and a
# reminder that silently vanishes there is the failure this file exists to end.

# Escapes $1 for use inside a JSON string. Backslash goes first, or the
# backslashes added for quotes and control characters get doubled.
json_escape() {
  local s
  # Control characters JSON cannot carry raw, other than tab, CR and LF, are
  # dropped rather than \u-escaped: none belongs in a reminder.
  s=$(printf '%s' "$1" | tr -d '\001-\010\013\014\016-\037')
  s=${s//\\/\\\\}
  s=${s//\"/\\\"}
  s=${s//$'\t'/\\t}
  s=${s//$'\r'/\\r}
  s=${s//$'\n'/\\n}
  printf '%s' "$s"
}

# $1 = hook event name (PreToolUse, PostToolUse), $2 = the text for the model.
# Prints nothing for an empty message: an empty additionalContext is noise.
emit_context() {
  local event=$1 message=$2
  [ -n "$message" ] || return 0
  printf '{"hookSpecificOutput":{"hookEventName":"%s","additionalContext":"%s"}}\n' \
    "$event" "$(json_escape "$message")"
}

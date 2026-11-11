#!/usr/bin/env bash
# Adapter that runs canonical .claude/scripts/* and .claude/hooks/* hooks under Antigravity.
#
# Usage (generated into .agents/hooks.json by scripts/sync-codex.mjs / sync-antigravity.mjs):
#   bash "$(git rev-parse --show-toplevel)"/.claude/scripts/_antigravity-hook.sh SessionStart .claude/scripts/session-start-kb-check.sh
#   bash "$(git rev-parse --show-toplevel)"/.claude/scripts/_antigravity-hook.sh PreInvocation
#   bash "$(git rev-parse --show-toplevel)"/.claude/scripts/_antigravity-hook.sh PreToolUse  .claude/scripts/check-changelog-staged.sh
#   bash "$(git rev-parse --show-toplevel)"/.claude/scripts/_antigravity-hook.sh PostToolUse .claude/scripts/post-edit-format.sh
#   bash "$(git rev-parse --show-toplevel)"/.claude/scripts/_antigravity-hook.sh PostToolUse --eval 'HOOK_PROBE_LOG=... bash ...'
#
# Why an adapter instead of a second set of hook scripts:
#   1. Antigravity runs hooks with .agents/ as cwd, sends camelCase payloads
#      (conversationId, toolCall.name, toolCall.args.CommandLine / TargetFile / Cwd),
#      and requires exit 0 with single-object JSON on stdout. Any non-zero exit
#      -- including Claude/Codex's blocking exit 2 -- is logged as a hook error
#      and fails open.
#   2. PreToolUse returns {"decision":"allow"} or {"decision":"deny","reason":"..."}
#      and cannot carry additionalContext directly; PostToolUse and Stop return {}.
#      Advisory text from PreToolUse/PostToolUse is queued per conversationId and
#      flushed on the next PreInvocation as {"injectSteps":[{"ephemeralMessage":"..."}]}.
#   3. On SessionStart, ephemeralMessage lasts only one model call, so startup
#      guidance is emitted as {"injectSteps":[{"systemMessage":{"systemMessage":"..."}}]}.
#      (LL-G kb/antigravity-cli/antigravity-hooks-invocationnum-restarts-each-turn-posttooluse-never-fires.md)

set -u

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
. "$SCRIPT_DIR/_json-parser.sh"
. "$SCRIPT_DIR/_hook-context.sh"

EVENT=${1:-}
MODE_OR_TARGET=${2:-}
EVAL_CMD=${3:-}

REPO_ROOT=$(git rev-parse --show-toplevel 2>/dev/null) \
  || REPO_ROOT=$(cd "$SCRIPT_DIR/../.." 2>/dev/null && pwd -P) \
  || REPO_ROOT=$PWD
export CLAUDE_PROJECT_DIR="$REPO_ROOT"

RAW_INPUT=""
if [ ! -t 0 ]; then
  RAW_INPUT=$(cat)
fi

CONV_ID=$(json_field "$RAW_INPUT" conversationId)
[ -n "$CONV_ID" ] || CONV_ID=$(json_field_flat "$RAW_INPUT" conversationId)
[ -n "$CONV_ID" ] || CONV_ID=${ANTIGRAVITY_CONVERSATION_ID:-}
[ -n "$CONV_ID" ] || CONV_ID=$(json_field "$RAW_INPUT" session_id)
SAFE_ID=$(printf '%s' "$CONV_ID" | tr -cd 'A-Za-z0-9_-')
[ -n "$SAFE_ID" ] || SAFE_ID="default"

QUEUE_DIR="${TMPDIR:-/tmp}/agy-hook-queue"
QUEUE_FILE="$QUEUE_DIR/$SAFE_ID.msg"

queue_advice() {
  local msg=${1:-}
  [ -n "$msg" ] || return 0
  if mkdir -p "$QUEUE_DIR" 2>/dev/null && [ -w "$QUEUE_DIR" ]; then
    find "$QUEUE_DIR" -type f -mtime +1 -delete 2>/dev/null || true
    if [ -s "$QUEUE_FILE" ]; then
      printf '\n\n%s\n' "$msg" >>"$QUEUE_FILE"
    else
      printf '%s\n' "$msg" >"$QUEUE_FILE"
    fi
  fi
}

extract_additional_context() {
  local out=${1:-} ctx="" trimmed=""
  [ -n "$out" ] || return 0
  ctx=$(json_field "$out" hookSpecificOutput.additionalContext)
  [ -n "$ctx" ] || ctx=$(json_field_flat "$out" additionalContext)
  if [ -z "$ctx" ]; then
    trimmed=${out#"${out%%[![:space:]]*}"}
    case "$trimmed" in
      "{"*) ;;
      *) ctx="$out" ;;
    esac
  fi
  printf '%s' "$ctx"
}

build_claude_payload() {
  local agy_tool claude_tool="" cmd="" fpath="" arg_cwd=""
  agy_tool=$(json_field "$RAW_INPUT" toolCall.name)
  [ -n "$agy_tool" ] || agy_tool=$(json_field_flat "$RAW_INPUT" name)

  case "$agy_tool" in
    run_command)
      claude_tool="Bash"
      cmd=$(json_field "$RAW_INPUT" toolCall.args.CommandLine)
      [ -n "$cmd" ] || cmd=$(json_field_greedy "$RAW_INPUT" CommandLine)
      arg_cwd=$(json_field "$RAW_INPUT" toolCall.args.Cwd)
      [ -n "$arg_cwd" ] || arg_cwd=$(json_field_flat "$RAW_INPUT" Cwd)
      ;;
    write_to_file)
      claude_tool="Write"
      fpath=$(json_field "$RAW_INPUT" toolCall.args.TargetFile)
      [ -n "$fpath" ] || fpath=$(json_field_flat "$RAW_INPUT" TargetFile)
      ;;
    replace_file_content|multi_replace_file_content)
      claude_tool="Edit"
      fpath=$(json_field "$RAW_INPUT" toolCall.args.TargetFile)
      [ -n "$fpath" ] || fpath=$(json_field_flat "$RAW_INPUT" TargetFile)
      ;;
    view_file)
      claude_tool="Read"
      fpath=$(json_field "$RAW_INPUT" toolCall.args.AbsolutePath)
      [ -n "$fpath" ] || fpath=$(json_field_flat "$RAW_INPUT" AbsolutePath)
      ;;
    *)
      claude_tool="$agy_tool"
      ;;
  esac

  case "$arg_cwd" in
    ""|.) arg_cwd="$REPO_ROOT" ;;
    /*)   ;;
    *)    arg_cwd="$REPO_ROOT/$arg_cwd" ;;
  esac
  if [ ! -d "$arg_cwd" ]; then
    arg_cwd="$REPO_ROOT"
  fi

  printf '{"session_id":"%s","cwd":"%s","tool_name":"%s","tool_input":{"command":"%s","file_path":"%s"}}' \
    "$(json_escape "$SAFE_ID")" \
    "$(json_escape "$arg_cwd")" \
    "$(json_escape "$claude_tool")" \
    "$(json_escape "$cmd")" \
    "$(json_escape "$fpath")"
}

resolve_target() {
  local rel=${1:-}
  case "$rel" in
    /*) printf '%s' "$rel" ;;
    .claude/scripts/*)
      if [ -f "$REPO_ROOT/$rel" ]; then
        printf '%s/%s' "$REPO_ROOT" "$rel"
      else
        printf '%s/%s' "$SCRIPT_DIR" "${rel#.claude/scripts/}"
      fi
      ;;
    *) printf '%s/%s' "$REPO_ROOT" "$rel" ;;
  esac
}

invoke_target() {
  local payload=$1 target=""
  if [ "$MODE_OR_TARGET" = "--eval" ]; then
    [ -n "$EVAL_CMD" ] || return 0
    (cd "$REPO_ROOT" && printf '%s' "$payload" | bash -c "$EVAL_CMD")
    return $?
  fi
  [ -n "$MODE_OR_TARGET" ] || return 0
  target=$(resolve_target "$MODE_OR_TARGET")
  [ -f "$target" ] || return 0
  case "$target" in
    *.js|*.mjs|*.cjs) (cd "$REPO_ROOT" && printf '%s' "$payload" | node "$target") ;;
    *)                (cd "$REPO_ROOT" && printf '%s' "$payload" | bash "$target") ;;
  esac
}

case "$EVENT" in
  PreInvocation)
    if [ -s "$QUEUE_FILE" ]; then
      CLAIM="$QUEUE_FILE.$$"
      if mv "$QUEUE_FILE" "$CLAIM" 2>/dev/null; then
        MSG=$(cat "$CLAIM")
        rm -f "$CLAIM"
        if [ -n "$MSG" ]; then
          printf '{"injectSteps":[{"ephemeralMessage":"%s"}]}\n' "$(json_escape "$MSG")"
          exit 0
        fi
      fi
    fi
    printf '{}\n'
    exit 0
    ;;

  SessionStart)
    [ -n "$MODE_OR_TARGET" ] || { printf '{}\n'; exit 0; }
    OUT=$(invoke_target "$RAW_INPUT" 2>/dev/null) || true
    CTX=$(extract_additional_context "$OUT")
    [ -n "$CTX" ] || CTX="$OUT"
    if [ -n "$CTX" ]; then
      printf '{"injectSteps":[{"systemMessage":{"systemMessage":"%s"}}]}\n' "$(json_escape "$CTX")"
    else
      printf '{}\n'
    fi
    exit 0
    ;;

  PreToolUse)
    [ -n "$MODE_OR_TARGET" ] || { printf '{"decision":"allow"}\n'; exit 0; }
    SYNTH=$(build_claude_payload)
    ERR_FILE=$(mktemp "${TMPDIR:-/tmp}/agy-hook-err.XXXXXX" 2>/dev/null || printf '')
    if [ -n "$ERR_FILE" ]; then
      OUT=$(invoke_target "$SYNTH" 2>"$ERR_FILE")
      RC=$?
      ERR=$(cat "$ERR_FILE")
      rm -f "$ERR_FILE"
    else
      OUT=$(invoke_target "$SYNTH")
      RC=$?
      ERR=""
    fi

    if [ "$RC" -eq 2 ]; then
      [ -n "$ERR" ] || ERR="$OUT"
      [ -n "$ERR" ] || ERR="Blocked by ${EVAL_CMD:-$MODE_OR_TARGET}"
      printf '{"decision":"deny","reason":"%s"}\n' "$(json_escape "$ERR")"
      exit 0
    fi

    if [ "$RC" -eq 0 ]; then
      ADVICE=$(extract_additional_context "$OUT")
      queue_advice "$ADVICE"
    elif [ -n "$ERR" ]; then
      printf '%s\n' "$ERR" >&2
    fi
    printf '{"decision":"allow"}\n'
    exit 0
    ;;

  PostToolUse)
    [ -n "$MODE_OR_TARGET" ] || { printf '{}\n'; exit 0; }
    SYNTH=$(build_claude_payload)
    OUT=$(invoke_target "$SYNTH" 2>/dev/null) || true
    ADVICE=$(extract_additional_context "$OUT")
    queue_advice "$ADVICE"
    printf '{}\n'
    exit 0
    ;;

  Stop)
    [ -n "$MODE_OR_TARGET" ] || { printf '{}\n'; exit 0; }
    invoke_target "$RAW_INPUT" >/dev/null 2>&1 || true
    printf '{}\n'
    exit 0
    ;;

  *)
    printf '{}\n'
    exit 0
    ;;
esac

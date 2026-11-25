#!/usr/bin/env bash
# Adapter that runs canonical .claude/scripts/* and .claude/hooks/* hooks under Antigravity.
#
# Usage (generated into .agents/hooks.json by scripts/sync-codex.mjs / sync-antigravity.mjs):
#   bash ../.claude/scripts/_antigravity-hook.sh SessionStart .claude/scripts/session-start-kb-check.sh
#   bash ../.claude/scripts/_antigravity-hook.sh PreInvocation
#   bash ../.claude/scripts/_antigravity-hook.sh PreToolUse  --enforce-deny
#   bash ../.claude/scripts/_antigravity-hook.sh PreToolUse  .claude/scripts/check-changelog-staged.sh
#   bash ../.claude/scripts/_antigravity-hook.sh PostToolUse .claude/scripts/post-edit-format.sh
#   bash ../.claude/scripts/_antigravity-hook.sh PostToolUse --eval 'HOOK_PROBE_LOG=... bash ...'
#
# Antigravity runs hooks with .agents/ as cwd (sh -c on Unix, cmd /c on Windows), sends
# camelCase JSON (conversationId, cwd, toolCall.name, toolCall.args.CommandLine / Path /
# TargetFile / AbsolutePath / Cwd), and treats any non-zero exit as fail-open. Blocking
# exit 2 becomes {"decision":"deny","reason":"..."} (exit 0); PreToolUse/PostToolUse advice
# queues per conversationId and flushes once on the next PreInvocation as systemMessage.

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
[ ! -t 0 ] && RAW_INPUT=$(cat)

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
    if [ -s "$QUEUE_FILE" ]; then printf '\n\n%s\n' "$msg" >>"$QUEUE_FILE"
    else printf '%s\n' "$msg" >"$QUEUE_FILE"; fi
  fi
}

extract_additional_context() {
  local out=${1:-} ctx="" trimmed=""
  [ -n "$out" ] || return 0
  ctx=$(json_field "$out" hookSpecificOutput.additionalContext)
  [ -n "$ctx" ] || ctx=$(json_field_flat "$out" additionalContext)
  if [ -z "$ctx" ]; then
    trimmed=${out#"${out%%[![:space:]]*}"}
    case "$trimmed" in "{"*) ;; *) ctx="$out" ;; esac
  fi
  printf '%s' "$ctx"
}

unwrap_mcp_payload() {
  json_probe_parser
  case "$JSON_PARSER" in
    node)
      printf '%s' "$RAW_INPUT" | node -e '
        let raw = "";
        process.stdin.on("data", (c) => { raw += c; }).on("end", () => {
          try {
            const a = JSON.parse(raw)?.toolCall?.args ?? {};
            const srv = String(a.ServerName || "mcp");
            let tool = String(a.ToolName || "unknown");
            let input = a.Arguments && typeof a.Arguments === "object" ? a.Arguments : {};
            if (tool === "MULTI_EXECUTE_TOOL" && Array.isArray(input.tools) && input.tools.length === 1 && input.tools[0]?.tool_slug) {
              tool = String(input.tools[0].tool_slug);
              input = input.tools[0].arguments && typeof input.tools[0].arguments === "object" ? input.tools[0].arguments : {};
            }
            process.stdout.write(JSON.stringify({ session_id: process.argv[1], cwd: process.argv[2], tool_name: `mcp__${srv}__${tool}`, tool_input: input }));
          } catch { process.exit(1); }
        });
      ' "$SAFE_ID" "$REPO_ROOT" 2>/dev/null ;;
    python*)
      printf '%s' "$RAW_INPUT" | "$JSON_PARSER" -c '
import json, sys
try:
    a = (json.load(sys.stdin).get("toolCall") or {}).get("args") or {}
    srv, tool = str(a.get("ServerName") or "mcp"), str(a.get("ToolName") or "unknown")
    inp = a.get("Arguments") if isinstance(a.get("Arguments"), dict) else {}
    ts = inp.get("tools") if isinstance(inp.get("tools"), list) else []
    if tool == "MULTI_EXECUTE_TOOL" and len(ts) == 1 and isinstance(ts[0], dict) and ts[0].get("tool_slug"):
        tool = str(ts[0]["tool_slug"])
        inp = ts[0].get("arguments") if isinstance(ts[0].get("arguments"), dict) else {}
    sys.stdout.write(json.dumps({"session_id": sys.argv[1], "cwd": sys.argv[2], "tool_name": f"mcp__{srv}__{tool}", "tool_input": inp}))
except Exception:
    sys.exit(1)
' "$SAFE_ID" "$REPO_ROOT" 2>/dev/null ;;
    *) return 1 ;;
  esac
}

# Mechanically enforce .claude/settings.json permissions.deny rules on PreToolUse.
# Prints the matched rule string to stdout and returns 2 when blocked; returns 0 otherwise.
check_settings_deny() {
  local settings_file="$REPO_ROOT/.claude/settings.json"
  [ -f "$settings_file" ] || return 0
  json_probe_parser
  case "$JSON_PARSER" in
    node)
      printf '%s' "$RAW_INPUT" | node -e '
        const fs = require("node:fs"), os = require("node:os"), path = require("node:path");
        let raw = "";
        process.stdin.on("data", (c) => { raw += c; }).on("end", () => {
          try {
            const deny = JSON.parse(fs.readFileSync(process.argv[1], "utf8"))?.permissions?.deny;
            if (!Array.isArray(deny) || !deny.length) process.exit(0);
            const tc = JSON.parse(raw)?.toolCall ?? {};
            const name = String(tc.name || ""), args = tc.args && typeof tc.args === "object" ? tc.args : {};
            const repo = process.argv[2].replace(/\\/g, "/").replace(/\/+$/, "");
            const home = os.homedir().replace(/\\/g, "/").replace(/\/+$/, "");
            const norm = (p) => {
              let s = String(p || "").replace(/\\/g, "/");
              if (s === "~") s = home; else if (s.startsWith("~/")) s = `${home}/${s.slice(2)}`;
              return s.replace(/\/+/g, "/");
            };
            const globToRe = (pat, isPath) => {
              let p = norm(pat), re = "^";
              if (isPath && p.startsWith("**/")) { re += "(?:.*/)?"; p = p.slice(3); }
              for (let i = 0; i < p.length; i++) {
                const ch = p[i];
                if (ch === "*" && p[i + 1] === "*") {
                  if (p[i + 2] === "/") { re += "(?:.*/)?"; i += 2; } else { re += ".*"; i += 1; }
                } else if (ch === "*") re += isPath ? "[^/]*" : ".*";
                else if (ch === "?") re += isPath ? "[^/]" : ".";
                else re += "\\.[]{}()+-^$|".includes(ch) ? `\\${ch}` : ch;
              }
              return new RegExp(`${re}$`);
            };
            const matchPath = (pat, t) => {
              if (!t) return false;
              const exp = norm(t), abs = norm(path.isAbsolute(exp) ? exp : path.resolve(repo, exp));
              const rel = abs.startsWith(`${repo}/`) ? abs.slice(repo.length + 1) : abs;
              const re = globToRe(pat, true);
              return re.test(abs) || re.test(rel);
            };
            const matchBash = (pat, cmd) =>
              !!cmd && String(cmd).split(/&&|\|\||[;|\n]/).map((s) => s.trim()).filter(Boolean).some((s) => globToRe(pat, false).test(s));
            let kind = null, target = "";
            if (name === "read_file" || name === "view_file") { kind = "Read"; target = args.Path || args.AbsolutePath || ""; }
            else if (["write_file", "write_to_file", "edit_file", "replace_file_content", "multi_replace_file_content"].includes(name)) { kind = "Edit"; target = args.Path || args.TargetFile || ""; }
            else if (name === "run_command") { kind = "Bash"; target = args.CommandLine || ""; }
            if (!kind || !target) process.exit(0);
            for (const rule of deny) {
              const m = String(rule).match(/^(\w+)\((.*)\)$/);
              if (!m) continue;
              const [, rk, pat] = m;
              if ((kind === "Read" && rk === "Read" && matchPath(pat, target)) ||
                  (kind === "Edit" && (rk === "Edit" || rk === "Write" || rk === "Read") && matchPath(pat, target)) ||
                  (kind === "Bash" && rk === "Bash" && matchBash(pat, target))) {
                process.stdout.write(rule); process.exit(2);
              }
            }
          } catch {}
          process.exit(0);
        });
      ' "$settings_file" "$REPO_ROOT" 2>/dev/null
      return $? ;;
    python*)
      printf '%s' "$RAW_INPUT" | "$JSON_PARSER" -c '
import json, os, re, sys
try:
    with open(sys.argv[1], "r", encoding="utf-8") as f:
        deny = (json.load(f).get("permissions") or {}).get("deny") or []
    if not deny: sys.exit(0)
    tc = json.load(sys.stdin).get("toolCall") or {}
    name, args = str(tc.get("name") or ""), tc.get("args") if isinstance(tc.get("args"), dict) else {}
    repo, home = sys.argv[2].replace("\\", "/").rstrip("/"), os.path.expanduser("~").replace("\\", "/").rstrip("/")
    def norm(p):
        s = str(p or "").replace("\\", "/")
        s = home if s == "~" else (f"{home}/{s[2:]}" if s.startswith("~/") else s)
        return re.sub(r"/+", "/", s)
    def glob_re(pat, is_path):
        p, out, i = norm(pat), "^", 0
        if is_path and p.startswith("**/"): out += "(?:.*/)?"; p = p[3:]
        while i < len(p):
            ch = p[i]
            if ch == "*" and i + 1 < len(p) and p[i + 1] == "*":
                if i + 2 < len(p) and p[i + 2] == "/": out += "(?:.*/)?"; i += 3
                else: out += ".*"; i += 2
            elif ch == "*": out += "[^/]*" if is_path else ".*"; i += 1
            elif ch == "?": out += "[^/]" if is_path else "."; i += 1
            else: out += re.escape(ch); i += 1
        return re.compile(out + "$")
    def match_path(pat, target):
        if not target: return False
        exp = norm(target)
        a = norm(os.path.normpath(exp if os.path.isabs(exp) else os.path.join(repo, exp)))
        rel = a[len(repo) + 1:] if a.startswith(repo + "/") else a
        rx = glob_re(pat, True)
        return bool(rx.match(a) or rx.match(rel))
    def match_bash(pat, cmd):
        return bool(cmd) and any(glob_re(pat, False).match(s.strip()) for s in re.split(r"&&|\|\||[;|\n]", str(cmd)) if s.strip())
    kind, target = None, ""
    if name in ("read_file", "view_file"): kind, target = "Read", args.get("Path") or args.get("AbsolutePath") or ""
    elif name in ("write_file", "write_to_file", "edit_file", "replace_file_content", "multi_replace_file_content"): kind, target = "Edit", args.get("Path") or args.get("TargetFile") or ""
    elif name == "run_command": kind, target = "Bash", args.get("CommandLine") or ""
    if not kind or not target: sys.exit(0)
    for rule in deny:
        m = re.match(r"^(\w+)\((.*)\)$", str(rule))
        if not m: continue
        rk, pat = m.group(1), m.group(2)
        if (kind == "Read" and rk == "Read" and match_path(pat, target)) or \
           (kind == "Edit" and rk in ("Edit", "Write", "Read") and match_path(pat, target)) or \
           (kind == "Bash" and rk == "Bash" and match_bash(pat, target)):
            sys.stdout.write(rule); sys.exit(2)
except Exception:
    pass
sys.exit(0)
' "$settings_file" "$REPO_ROOT" 2>/dev/null
      return $? ;;
    *) return 0 ;;
  esac
}

build_claude_payload() {
  local agy_tool claude_tool="" cmd="" fpath="" arg_cwd="" mcp_payload=""
  agy_tool=$(json_field "$RAW_INPUT" toolCall.name)
  [ -n "$agy_tool" ] || agy_tool=$(json_field_flat "$RAW_INPUT" name)

  case "$agy_tool" in
    run_command)
      claude_tool="Bash"
      cmd=$(json_field "$RAW_INPUT" toolCall.args.CommandLine)
      [ -n "$cmd" ] || cmd=$(json_field_greedy "$RAW_INPUT" CommandLine)
      arg_cwd=$(json_field "$RAW_INPUT" toolCall.args.Cwd)
      [ -n "$arg_cwd" ] || arg_cwd=$(json_field "$RAW_INPUT" cwd)
      [ -n "$arg_cwd" ] || arg_cwd=$(json_field_flat "$RAW_INPUT" Cwd)
      [ -n "$arg_cwd" ] || arg_cwd=$(json_field_flat "$RAW_INPUT" cwd) ;;
    write_file|write_to_file)
      claude_tool="Write"
      fpath=$(json_field "$RAW_INPUT" toolCall.args.Path)
      [ -n "$fpath" ] || fpath=$(json_field "$RAW_INPUT" toolCall.args.TargetFile)
      [ -n "$fpath" ] || fpath=$(json_field_flat "$RAW_INPUT" Path)
      [ -n "$fpath" ] || fpath=$(json_field_flat "$RAW_INPUT" TargetFile) ;;
    edit_file|replace_file_content|multi_replace_file_content)
      claude_tool="Edit"
      fpath=$(json_field "$RAW_INPUT" toolCall.args.Path)
      [ -n "$fpath" ] || fpath=$(json_field "$RAW_INPUT" toolCall.args.TargetFile)
      [ -n "$fpath" ] || fpath=$(json_field_flat "$RAW_INPUT" Path)
      [ -n "$fpath" ] || fpath=$(json_field_flat "$RAW_INPUT" TargetFile) ;;
    read_file|view_file)
      claude_tool="Read"
      fpath=$(json_field "$RAW_INPUT" toolCall.args.Path)
      [ -n "$fpath" ] || fpath=$(json_field "$RAW_INPUT" toolCall.args.AbsolutePath)
      [ -n "$fpath" ] || fpath=$(json_field_flat "$RAW_INPUT" Path)
      [ -n "$fpath" ] || fpath=$(json_field_flat "$RAW_INPUT" AbsolutePath) ;;
    call_mcp_tool)
      mcp_payload=$(unwrap_mcp_payload) || mcp_payload=""
      if [ -n "$mcp_payload" ]; then printf '%s' "$mcp_payload"; return 0; fi
      claude_tool="call_mcp_tool" ;;
    *)
      claude_tool="$agy_tool" ;;
  esac

  case "$arg_cwd" in
    ""|.) arg_cwd="$REPO_ROOT" ;;
    /*|[A-Za-z]:/*|[A-Za-z]:\\*) ;;
    *)    arg_cwd="$REPO_ROOT/$arg_cwd" ;;
  esac
  [ -d "$arg_cwd" ] || arg_cwd="$REPO_ROOT"

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
      if [ -f "$REPO_ROOT/$rel" ]; then printf '%s/%s' "$REPO_ROOT" "$rel"
      else printf '%s/%s' "$SCRIPT_DIR" "${rel#.claude/scripts/}"; fi ;;
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
          printf '{"injectSteps":[{"systemMessage":{"systemMessage":"%s"}}]}\n' "$(json_escape "$MSG")"
          exit 0
        fi
      fi
    fi
    printf '{}\n'
    exit 0 ;;

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
    exit 0 ;;

  PreToolUse)
    [ -n "$MODE_OR_TARGET" ] || { printf '{"decision":"allow"}\n'; exit 0; }
    if [ "$MODE_OR_TARGET" = "--enforce-deny" ]; then
      DENY_RULE=$(check_settings_deny)
      RC=$?
      if [ "$RC" -eq 2 ]; then
        printf '{"decision":"deny","reason":"Blocked by .claude/settings.json permissions.deny rule: %s"}\n' \
          "$(json_escape "$DENY_RULE")"
        exit 0
      fi
      printf '{"decision":"allow"}\n'
      exit 0
    fi
    SYNTH=$(build_claude_payload)
    ERR_FILE=$(mktemp "${TMPDIR:-/tmp}/agy-hook-err.XXXXXX" 2>/dev/null || printf '')
    if [ -n "$ERR_FILE" ]; then
      OUT=$(invoke_target "$SYNTH" 2>"$ERR_FILE"); RC=$?; ERR=$(cat "$ERR_FILE"); rm -f "$ERR_FILE"
    else
      OUT=$(invoke_target "$SYNTH"); RC=$?; ERR=""
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
    exit 0 ;;

  PostToolUse)
    [ -n "$MODE_OR_TARGET" ] || { printf '{}\n'; exit 0; }
    SYNTH=$(build_claude_payload)
    OUT=$(invoke_target "$SYNTH" 2>/dev/null) || true
    ADVICE=$(extract_additional_context "$OUT")
    queue_advice "$ADVICE"
    printf '{}\n'
    exit 0 ;;

  Stop)
    [ -n "$MODE_OR_TARGET" ] || { printf '{}\n'; exit 0; }
    invoke_target "$RAW_INPUT" >/dev/null 2>&1 || true
    printf '{}\n'
    exit 0 ;;

  *)
    printf '{}\n'
    exit 0 ;;
esac

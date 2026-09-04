#!/usr/bin/env bash
# PostToolUse hook on Write|Edit: format the file that was just written.
#
# The playbook's build-time guardrail: hooks are the deterministic layer behind
# advisory skills. Formatting is the clearest case -- it is mechanical, it has a
# single right answer per project, and REVIEW.md forbids reporting it in review
# precisely because a hook is supposed to own it. If this hook does not run,
# formatting silently becomes a review topic again.
#
# Deliberately NON-BLOCKING: always exits 0. A formatter that refuses an edit
# turns every unformattable file into a wall. Heavier checks (lint, typecheck,
# tests) belong at commit and CI time, where the feedback loop can afford them.
#
# LL-G: kb/claude-code/{hook-env-vars-do-not-exist,hook-empty-path-formats-repo}.md

set -u

# --------------------------------------------------------------- read payload
# $CLAUDE_FILE_PATH does not exist. It expands to "", and an empty path is not a
# no-op -- `prettier --write ""` walks the entire repo and rewrites every file it
# understands. Parse the path from stdin and hard-guard on it instead.
# (LL-G kb/claude-code/hook-empty-path-formats-repo.md)
HOOK_INPUT=$(cat)

# Parser selection lives in _json-parser.sh, which probes a candidate by RUNNING
# it: a `command -v python3` lookup succeeds on Windows by finding the
# WindowsApps stub, and every extracted field then comes back silently empty.
. "$(dirname "${BASH_SOURCE[0]}")/_json-parser.sh"

FILE_PATH=$(json_field "$HOOK_INPUT" tool_input.file_path)
[ -n "$FILE_PATH" ] || FILE_PATH=$(json_field_flat "$HOOK_INPUT" file_path)

# The hard guard. Everything below this line may pass "$FILE_PATH" to a tool.
[ -n "$FILE_PATH" ] || exit 0
[ -f "$FILE_PATH" ] || exit 0

# Never format outside the repo, whatever the payload claims. A path that escapes
# the working tree is either a bug or an attempt to use the hook as a write
# primitive; both deserve the same answer.
REPO_ROOT=$(git rev-parse --show-toplevel 2>/dev/null) || exit 0
case "$(cd "$(dirname "$FILE_PATH")" 2>/dev/null && pwd -P)" in
  "$REPO_ROOT"|"$REPO_ROOT"/*) ;;
  *) exit 0 ;;
esac

# Generated and vendored trees are formatted by whatever generates them.
case "$FILE_PATH" in
  */node_modules/*|*/dist/*|*/build/*|*/.git/*|*/vendor/*|*/target/*) exit 0 ;;
esac

# ------------------------------------------------------------ pick a formatter
# Only ever run a formatter the project itself declares. Reaching for a globally
# installed prettier in a repo that uses biome reformats the whole file to the
# wrong style, and the diff looks like the author did it.
run() { "$@" >/dev/null 2>&1; }

format_js_like() {
  if [ -f "$REPO_ROOT/biome.json" ] || [ -f "$REPO_ROOT/biome.jsonc" ]; then
    run npx --no-install biome format --write "$FILE_PATH" && return 0
  fi
  for cfg in .prettierrc .prettierrc.json .prettierrc.js .prettierrc.cjs prettier.config.js prettier.config.cjs .prettierrc.yaml .prettierrc.yml; do
    if [ -f "$REPO_ROOT/$cfg" ]; then
      run npx --no-install prettier --write "$FILE_PATH" && return 0
    fi
  done
  # A "prettier" key in package.json is the other supported config location.
  if [ -f "$REPO_ROOT/package.json" ] && grep -q '"prettier"' "$REPO_ROOT/package.json" 2>/dev/null; then
    run npx --no-install prettier --write "$FILE_PATH" && return 0
  fi
  return 1
}

format_python() {
  if [ -f "$REPO_ROOT/pyproject.toml" ] || [ -f "$REPO_ROOT/ruff.toml" ] || [ -f "$REPO_ROOT/.ruff.toml" ]; then
    run ruff format "$FILE_PATH" && return 0
    run black "$FILE_PATH" && return 0
  fi
  return 1
}

case "$FILE_PATH" in
  *.js|*.jsx|*.mjs|*.cjs|*.ts|*.tsx|*.json|*.jsonc|*.css|*.scss|*.html|*.md|*.yaml|*.yml)
    format_js_like ;;
  *.py)
    format_python ;;
  *.rs)
    [ -f "$REPO_ROOT/Cargo.toml" ] && run rustfmt "$FILE_PATH" ;;
  *.go)
    [ -f "$REPO_ROOT/go.mod" ] && run gofmt -w "$FILE_PATH" ;;
  *.sh|*.bash)
    [ -f "$REPO_ROOT/.editorconfig" ] && run shfmt -w "$FILE_PATH" ;;
esac

# Always succeed. A formatter that is absent, misconfigured, or unhappy with this
# particular file must not block the edit that triggered it.
exit 0

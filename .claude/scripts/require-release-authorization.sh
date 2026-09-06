#!/usr/bin/env bash
# PreToolUse hook on Bash: pause a production deploy until a named person has
# authorised this specific release.
#
# Stage 5 of the AI-native SDLC playbook: "hooks as approval gates". The point is
# not to make deploys hard -- it is to make the authorisation a recorded, checkable
# fact rather than a click nobody remembers giving.
#
# BLOCKING (exit 2). A blocking hook's stdout is discarded, so every refusal here
# writes to stderr; a gate that refuses without saying why trains people to
# disable it.
#
# Scope is deliberately narrow. It fires only on commands that look like a
# production deploy, so ordinary work never sees it. A gate that fires on
# everything is a gate that gets switched off.
#
# LL-G: kb/claude-code/{hook-env-vars-do-not-exist,hook-matcher-tool-names-only}.md

set -u

HOOK_INPUT=$(cat)

. "$(dirname "${BASH_SOURCE[0]}")/_json-parser.sh"

CMD=$(json_field "$HOOK_INPUT" tool_input.command)
# Degraded path: no working interpreter. Use the over-eager extraction, because
# for a blocking gate truncating the subject is the dangerous direction -- a
# half-read command that no longer matches would wave a deploy straight through.
[ -n "$CMD" ] || CMD=$(json_field_greedy "$HOOK_INPUT" command)
[ -n "$CMD" ] || exit 0

# ------------------------------------------------------------- is this a deploy?
# Match against the command with quoted regions collapsed, NOT the raw text. A
# substring match on raw text cannot tell a deploy from a command that merely
# mentions one -- `echo '{"command":"wrangler deploy"}' | bash gate.sh` is not a
# deploy, and blocking it makes the gate fire on its own tests. This is the same
# distinction _git-commit-filter.sh draws, for the same reason; each quoted
# region collapses to one opaque token rather than being deleted, so a flag's
# quoted value cannot let the following word slide into a command position.
#
# Newlines become separators so the second line of a multi-line script is still
# checked.
SCAN=$(printf '%s' "$CMD" \
  | sed -e "s/'[^']*'/__Q__/g" -e 's/"[^"]*"/__Q__/g' \
  | tr '\n' ';' \
  | sed -e 's/[;&|()][;&|()]*/ ; /g')

# Match on intent, not on one tool. Add this project's own deploy command here;
# the list travels with the repo, so keep it accurate rather than broad.
is_production_deploy() {
  case "$SCAN" in
    *--dry-run*|*--help*|*" -h"*) return 1 ;;
  esac
  case "$SCAN" in
    *wrangler*deploy*|*wrangler*publish*)          return 0 ;;
    *railway*up*|*railway*redeploy*)               return 0 ;;
    *"vercel --prod"*|*"vercel deploy --prod"*)    return 0 ;;
    *fly*deploy*)                                  return 0 ;;
    *"kubectl apply"*prod*|*helm*upgrade*prod*)    return 0 ;;
    *terraform*apply*)                             return 0 ;;
    *npm*run*deploy:prod*|*pnpm*deploy:prod*)      return 0 ;;
  esac
  return 1
}

is_production_deploy || exit 0

# ------------------------------------------------------------------ the gate
# Authorisation rides on the command itself, exactly like SKIP_CHANGELOG. The
# hook is spawned by the harness and does NOT inherit variables exported in an
# earlier shell command, so reading the environment would silently never match.
# (LL-G kb/claude-code/hook-env-assignment-not-inherited.md)
RELEASE_AUTH=$(printf '%s' "$SCAN" | sed -n 's/.*RELEASE_AUTHORIZED_BY=\([A-Za-z0-9._@-]\{1,\}\).*/\1/p' | head -1)

if [ -n "$RELEASE_AUTH" ]; then
  # Recorded, not just allowed: the session transcript now carries who authorised
  # this deploy, which is the artifact an audit actually needs.
  echo "Release authorised by: $RELEASE_AUTH"
  exit 0
fi

{
  echo "BLOCKED: this looks like a production deploy, and no release authorisation is attached."
  echo ""
  echo "  Command: $(printf '%s' "$CMD" | cut -c1-120)"
  echo ""
  echo "Production deploys require a named person to authorise the specific release,"
  echo "so the authorisation is a recorded fact rather than an unremembered click."
  echo ""
  echo "  1. Confirm with the release owner what is shipping."
  echo "  2. Re-run with their identifier as a prefix on the command itself:"
  echo ""
  echo "       RELEASE_AUTHORIZED_BY=<name-or-email> <your deploy command>"
  echo ""
  echo "The prefix must ride on the same command being judged -- the hook reads it"
  echo "out of the command text, not out of its own environment, which the harness"
  echo "gives it rather than your shell."
  echo ""
  echo "This gate is scoped to production deploy commands only; --dry-run is exempt."
  echo "If it fired on something that is not a production deploy, fix the match list"
  echo "in .claude/scripts/require-release-authorization.sh rather than disabling it."
} >&2
exit 2

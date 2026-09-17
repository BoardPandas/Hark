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

# Match on intent, not on one tool. Add this project's own deploy command to
# `segment_deploys`; the list travels with the repo, so keep it accurate rather
# than broad.
is_production_deploy() {
  case "$SCAN" in
    *--dry-run*|*--help*|*" -h"*) return 1 ;;
  esac
  is_release_command
}

# True when the command actually runs a production deploy.
#
#   git push origin v1.2.3                      -> yes
#   git push --follow-tags                      -> yes
#   git -C /repo push origin refs/tags/v1       -> yes
#   gh release create v1.2.3                    -> yes
#   wrangler deploy                             -> yes
#   git push origin main                        -> NO (publishes nothing to users)
#   sed -i s/x/y/ .github/workflows/rel.yml     -> NO
#   git commit -m "gate wrangler deploy pushes" -> NO
#
# This walks the token stream instead of globbing the whole command, for the
# same reason `_git-commit-filter.sh` does. Both halves of this gate proved the
# point the hard way:
#
#   - A first draft matched `*git*push*v[0-9]*.[0-9]*` and blocked an ordinary
#     heredoc writing a workflow file, because `.github` supplied "git", the
#     word "push" appeared in a comment, and an action's `# v2.9.2` SHA pin
#     supplied the version.
#   - The original tool list matched `*wrangler*deploy*` and `*fly*deploy*`
#     against the whole command, so a COMMIT whose message merely discussed
#     deploy tooling was refused as a deploy. A gate that fires on writing
#     about deploys is one people switch off.
#
# Globs cannot tell a command position from prose. The walk models argv: it
# splits on command separators, skips env-assignment prefixes, resolves the
# program name (so `/usr/bin/git` counts and `mygit` does not), and only then
# looks at that program's own arguments.
# (LL-G kb/claude-code/hook-git-commit-filter-needs-argv-walk.md)
is_release_command() {
  local normalized restore_glob segment tok result

  # Quoted regions collapse to ONE opaque token rather than being deleted:
  # `git -C "/path with space" push` must not become `git -C  push`, where -C
  # would swallow `push` as its own value. Newlines become separators so line
  # two of a script is still a command position.
  normalized=$(printf '%s' "$CMD" \
    | sed -e "s/'[^']*'/__RQ__/g" -e 's/"[^"]*"/__RQ__/g' \
    | tr '\n' ';' \
    | sed -e 's/[;&|()][;&|()]*/ ; /g')

  # Unquoted word splitting below would otherwise glob-expand a token like `*`
  # against the working directory.
  restore_glob=0
  case $- in
    *f*) ;;
    *)   restore_glob=1; set -f ;;
  esac

  result=1
  segment=""
  for tok in $normalized; do
    if [ "$tok" = ";" ]; then
      if segment_deploys $segment; then result=0; break; fi
      segment=""
      continue
    fi
    segment="$segment $tok"
  done
  # The last segment has no trailing separator to flush it.
  if [ "$result" != 0 ] && [ -n "$segment" ]; then
    segment_deploys $segment && result=0
  fi

  [ "$restore_glob" = 1 ] && set +f
  return "$result"
}

# True when ONE command segment is a production deploy. The segment's argv
# arrives as positional arguments.
#
# ADD THIS PROJECT'S DEPLOY COMMAND HERE. The list travels with the repo, so
# keep it accurate rather than broad.
segment_deploys() {
  local prog

  # Skip env-assignment prefixes: `RELEASE_AUTHORIZED_BY=x git push ...` is
  # still a git invocation, and so is `GIT_AUTHOR_NAME=x git push`.
  while [ "$#" -gt 0 ]; do
    case "$1" in
      *=*) shift ;;
      *)   break ;;
    esac
  done
  [ "$#" -gt 0 ] || return 1

  # Resolve the program name out of a path, so /usr/bin/git counts. Exact after
  # that: `mygit` and `deploy-notes.md` are not git.
  prog=${1##*/}
  prog=${prog%.exe}
  shift

  case "$prog" in
    git)           git_pushes_a_tag "$@" ;;
    gh)            gh_publishes "$@" ;;
    wrangler)      has_token "deploy publish" "$@" ;;
    railway)       has_token "up redeploy" "$@" ;;
    vercel)        has_token "--prod" "$@" ;;
    fly|flyctl)    has_token "deploy" "$@" ;;
    terraform)     has_token "apply" "$@" ;;
    kubectl)       has_token "apply" "$@" && has_prod "$@" ;;
    helm)          has_token "upgrade" "$@" && has_prod "$@" ;;
    npm|pnpm|yarn) has_token "deploy:prod" "$@" ;;
    *)             return 1 ;;
  esac
}

# has_token "<space-separated wanted>" <args...>
has_token() {
  local wanted=$1 a w
  shift
  for a in "$@"; do
    for w in $wanted; do
      [ "$a" = "$w" ] && return 0
    done
  done
  return 1
}

# True if any argument names a production environment.
has_prod() {
  local a
  for a in "$@"; do
    case "$a" in *prod*) return 0 ;; esac
  done
  return 1
}

# True when a `git push`'s arguments carry a tag. A branch refspec (`main`,
# `HEAD:main`) is not a deploy: it publishes nothing to users.
git_pushes_a_tag() {
  local tok saw_push=0 skip=0
  for tok in "$@"; do
    if [ "$skip" = 1 ]; then skip=0; continue; fi
    if [ "$saw_push" = 0 ]; then
      case "$tok" in
        # Global flags that consume the NEXT token as their value. Missing one
        # here would let its value be read as the subcommand, and the gate
        # would stop firing for every command that uses it.
        -C|-c|--git-dir|--work-tree|--namespace) skip=1 ;;
        push)                                    saw_push=1 ;;
        -*)                                      ;;  # self-contained flag
        *)                                       return 1 ;;  # other subcommand
      esac
      continue
    fi
    case "$tok" in
      --tags|--follow-tags|--mirror) return 0 ;;
      tag)                           return 0 ;;
      *refs/tags/*)                  return 0 ;;
      v[0-9]*)                       return 0 ;;
    esac
  done
  return 1
}

# True when a `gh` invocation publishes a release, or re-runs the workflow that
# does (release.yml's workflow_dispatch takes an existing tag).
gh_publishes() {
  local tok sub="" verb=""
  for tok in "$@"; do
    case "$tok" in -*) continue ;; esac
    if [ -z "$sub" ]; then sub=$tok; continue; fi
    if [ -z "$verb" ]; then verb=$tok; continue; fi
    if [ "$sub" = workflow ] && [ "$verb" = run ]; then
      case "$tok" in *release*) return 0 ;; esac
    fi
  done
  case "$sub/$verb" in
    release/create|release/upload|release/edit|release/delete) return 0 ;;
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

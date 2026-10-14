#!/usr/bin/env bash
# Shared self-filter for the git-commit hooks. Source it, do not execute it:
#
#   . "$(dirname "$0")/_git-commit-filter.sh"
#   read_hook_input                 # sets HOOK_COMMAND from stdin
#   is_git_commit || exit 0
#
# Why this exists as a helper rather than four copies: the four commit hooks had
# four slightly different inline filters, and the one that drifted (a missing
# ">&2") went unnoticed for months. One definition, one place to fix.

# JSON extraction lives in _json-parser.sh for the same reason, one level down.
. "$(dirname "${BASH_SOURCE[0]}")/_json-parser.sh"

# Reads the hook's JSON payload from stdin and sets:
#   HOOK_INPUT    raw payload (kept for callers that need the unparsed text)
#   HOOK_COMMAND  tool_input.command only
#   HOOK_CWD      the payload's cwd, i.e. where the command is about to run
#
# Scanning tool_input.command rather than the whole payload matters: a PostToolUse
# payload also carries tool_response, so a command whose OUTPUT merely mentions
# "git commit" would otherwise trip the filter.
# (LL-G kb/bash/hook-scans-tool-output-false-record.md)
read_hook_input() {
  HOOK_INPUT=$(cat)
  HOOK_COMMAND=$(json_field "$HOOK_INPUT" tool_input.command)

  # Degraded path: no working parser, or a parser that returned nothing for a
  # payload that clearly carries a command. Recover the command from the raw
  # text -- over-eager, but a blocking hook must not go quiet just because a
  # parser is missing.
  #
  # json_field_flat is deliberately NOT used: it stops at the first quote, so
  # `echo "hi" && git commit` truncates to `echo ` and the gate stops blocking.
  # Under-eager is the one direction this hook cannot afford.
  #
  # Handing over the whole payload -- what this did before -- is not a fallback
  # either. is_git_commit strips quoted regions, and in raw JSON the command IS
  # a quoted region, so it strips to nothing and never matches. That fallback
  # was itself dead; it just had no way to show it while the parser branch was
  # also dead.
  if [ -z "$HOOK_COMMAND" ]; then
    HOOK_COMMAND=$(json_field_greedy "$HOOK_INPUT" command)
  fi

  # The hook PROCESS does not run where the command will. In a worktree session
  # the harness starts it in the main checkout (verified 2026-10-14: pwd and
  # $CLAUDE_PROJECT_DIR were C:/repos/Hark, the payload's cwd was the worktree),
  # so `git diff --cached` read the main checkout's index and refused correctly
  # staged worktree commits. The payload's cwd is the session's directory.
  # Falling back to $PWD is right for a directly-invoked script and for a
  # payload written by an older harness.
  HOOK_CWD=$(json_field "$HOOK_INPUT" cwd)
  [ -n "$HOOK_CWD" ] && [ -d "$HOOK_CWD" ] || HOOK_CWD=$PWD
}

# ---------------------------------------------------------------------------
# Two views of the command, and they are not interchangeable:
#
#   normalize_command  collapses every quoted region to one opaque token. Right
#                      for "what sits at a command position", wrong for anything
#                      that needs a VALUE back out.
#   tokenize_command   preserves the contents of quoted regions, so `cd "/a b"`
#                      yields the path. Correspondingly more expensive: it is a
#                      character walk, so callers reach for it only when the
#                      cheap view says there is something to resolve.
# ---------------------------------------------------------------------------

# Quoted regions collapse to ONE opaque token rather than being deleted.
# Deleting them looks equivalent and is not: `git -C "/path with space" commit`
# would become `git -C  commit`, and the -C would then swallow the word
# `commit` as its own value -- a commit that reports itself as not a commit.
#
# Newlines become separators, so the second line of a two-line script is still
# a command position.
normalize_command() {
  printf '%s' "$HOOK_COMMAND" \
    | sed -e "s/'[^']*'/__CCQ__/g" -e 's/"[^"]*"/__CCQ__/g' \
    | tr '\n' ';' \
    | sed -e 's/[;&|()][;&|()]*/ ; /g'
}

# Unquoted word splitting over a token stream would otherwise glob-expand a token
# like `*` against the working directory. Paired: every push needs a pop.
noglob_push() {
  case $- in
    *f*) NOGLOB_RESTORE=0 ;;
    *)   NOGLOB_RESTORE=1; set -f ;;
  esac
}
noglob_pop() {
  [ "${NOGLOB_RESTORE:-0}" = 1 ] && set +f
  return 0
}

# Words that leave the NEXT word at a command position: shell keywords and
# grouping (`{ git commit; }`, `if git commit; then`, `! git commit`) and the
# plain precommand wrappers (`time git commit`, `env X=1 git commit`).
#
# Every walker below that tracks a command position asks this one question, so
# they cannot disagree about it. They did: none of them knew these words, so
# `{ git commit -m x; }` was not a commit at all to is_git_commit and the gate
# silently never fired -- the unsafe direction. Teaching only one walker is no
# better: the gate then fires, and judges the target repo or the opt-out from a
# different reading of the same command.
#
# A wrapper's own flags (`env -i`, `time -p`, `exec -a name`) are not modelled.
# The walk reads them as arguments and misses that commit rather than guess.
is_command_prefix() {
  case "$1" in
    '{'|'}'|'!'|if|then|else|elif|do|while|until|time|command|exec|nohup|env) return 0 ;;
  esac
  return 1
}

# True when HOOK_COMMAND actually invokes `git commit`.
#
#   git commit -m "mentions git commit"     -> is a commit
#   git add CHANGELOG.md && git commit      -> is a commit
#   git -C /other/repo commit               -> is a commit
#   GIT_AUTHOR_NAME=x git commit            -> is a commit
#   { git commit -m x; }                    -> is a commit
#   if git commit -m x; then ...; fi        -> is a commit
#   grep -r 'git commit' docs/              -> not a commit
#   git config --get commit.gpgsign         -> not a commit
#   echo git commit                         -> not a commit
#
# This walks the token stream rather than matching a regex, because a regex
# fails in both directions at once and cannot be tuned out of it. Tight enough
# to require `git` immediately followed by `commit` misses `git -C /repo commit`,
# since git's global flags sit between the two -- and a missed commit means the
# gate silently does not fire, the worst outcome available. Loosening it to
# allow arbitrary tokens in between starts blocking `git log --grep=commit` and
# `git config --get commit.gpgsign`. The walk gets both right by modelling what
# git's argv actually looks like.
# (LL-G kb/claude-code/hook-git-commit-filter-needs-argv-walk.md)
is_git_commit() {
  local normalized tok state result

  normalized=$(normalize_command)
  noglob_push

  # cmd   -- next token starts a command
  # flags -- inside git's global flags, still looking for the subcommand
  # skip  -- this token is the value of the flag before it
  # args  -- inside some other command's arguments; nothing here is a commit
  state=cmd
  result=1
  for tok in $normalized; do
    if [ "$state" = skip ]; then state=flags; continue; fi
    if [ "$tok" = ";" ]; then state=cmd; continue; fi
    case "$state" in
      cmd)
        is_command_prefix "$tok" && continue
        case "$tok" in
          *=*)                         ;;  # env assignment prefix; still at a command position
          git|git.exe|*/git|*/git.exe) state=flags ;;
          *)                           state=args ;;
        esac
        ;;
      flags)
        case "$tok" in
          # Global flags that consume the NEXT token as their value. Missing one
          # here means its value gets read as the subcommand, and the gate stops
          # firing for every command that uses it.
          -C|-c|--git-dir|--work-tree|--namespace) state=skip ;;
          commit)                                  result=0; break ;;
          -*)                                      ;;  # self-contained flag, incl. --git-dir=x
          *)                                       state=args ;;  # some other subcommand
        esac
        ;;
    esac
  done

  noglob_pop
  return "$result"
}

# Splits HOOK_COMMAND into the TOKENS array, keeping the CONTENTS of quoted
# regions (that is the whole difference from normalize_command). Shell separators
# become a bare ";" token; parens stay "(" and ")" so the walker can discard a
# subshell's `cd`, which does not outlive it.
tokenize_command() {
  local cmd=$1 i=0 n ch q='' tok='' started=0
  TOKENS=()
  n=${#cmd}
  while [ "$i" -lt "$n" ]; do
    ch=${cmd:i:1}
    i=$((i + 1))
    if [ -n "$q" ]; then
      if [ "$ch" = "$q" ]; then
        q=''
      elif [ "$q" = '"' ] && [ "$ch" = '\' ] && [ "$i" -lt "$n" ]; then
        # Inside double quotes a backslash escapes only these; before anything
        # else it is a literal character. Dropping it unconditionally turned
        # `cd "C:\repos\Hark"` into C:reposHark, and the gate then allowed.
        case "${cmd:i:1}" in
          '$'|'`'|'"'|'\'|$'\n') tok+=${cmd:i:1}; i=$((i + 1)) ;;
          *)                     tok+=$ch ;;
        esac
      else
        tok+=$ch
      fi
      continue
    fi
    case "$ch" in
      "'"|'"') q=$ch; started=1 ;;
      '\')     if [ "$i" -lt "$n" ]; then tok+=${cmd:i:1}; i=$((i + 1)); fi ;;
      ' '|$'\t')
        if [ -n "$tok" ] || [ "$started" = 1 ]; then TOKENS+=("$tok"); tok=''; started=0; fi ;;
      ';'|'&'|'|'|'('|')'|$'\n')
        if [ -n "$tok" ] || [ "$started" = 1 ]; then TOKENS+=("$tok"); tok=''; started=0; fi
        case "$ch" in
          '('|')') TOKENS+=("$ch") ;;
          *)       TOKENS+=(";") ;;
        esac ;;
      *) tok+=$ch ;;
    esac
  done
  if [ -n "$tok" ] || [ "$started" = 1 ]; then TOKENS+=("$tok"); fi
}

# Resolves $2 as a directory relative to $1. Fails on anything whose value this
# process cannot know -- a substitution, `cd -`, a bare `cd` -- because a guessed
# directory is worse than no directory: see commit_target_dir.
#
# `C:\...` and `C:/...` are absolute too. The payload's cwd arrives in that form
# on Windows, and so does any path copied out of it into a `cd`.
resolve_dir() {
  case "$2" in
    ''|-|--) return 1 ;;
    *'$'*|*'`'*) return 1 ;;
    /*|[A-Za-z]:*) printf '%s' "$2" ;;
    '~') printf '%s' "$HOME" ;;
    '~/'*) printf '%s%s' "$HOME" "${2#\~}" ;;
    *)   printf '%s/%s' "$1" "$2" ;;
  esac
}

# The directory the `git commit` in HOOK_COMMAND will actually run in: the
# payload's cwd, moved by a `cd`/`pushd` earlier in the command and by git's own
# -C. Fails when that cannot be determined.
#
# Neither the hook process's $PWD nor the payload's cwd alone is enough. `cd
# /other/repo && git commit` and `git -C /other/repo commit` retarget the command
# without moving either, and used to be judged against the session's CHANGELOG.md
# instead -- blocking a clean cross-repo commit and waving through a dirty one.
# (LL-G kb/claude-code/hook-cwd-is-not-the-commit-target-repo.md)
#
# Failure means ALLOW, not block. A gate that refuses commits in repos it cannot
# even see gets bypassed wholesale, and it takes the changelog check with it.
commit_target_dir() {
  local base=${HOOK_CWD:-$PWD} tok state curdir gitdir found depth
  local -a saved

  # Cheap view first: with no directory-changing token anywhere outside quotes,
  # the command runs in the payload's cwd and there is nothing to walk.
  noglob_push
  found=0
  for tok in $(normalize_command); do
    case "$tok" in
      cd|pushd|-C|--git-dir|--git-dir=*|--work-tree|--work-tree=*) found=1; break ;;
    esac
  done
  noglob_pop
  if [ "$found" = 0 ]; then printf '%s' "$base"; return 0; fi

  tokenize_command "$HOOK_COMMAND"
  state=cmd
  curdir=$base
  gitdir=$base
  saved=()
  for tok in "${TOKENS[@]}"; do
    # A separator where a value was expected means the command is not what it
    # looked like; do not guess at the target.
    case "$state" in
      cdarg|cflag) case "$tok" in ';'|'('|')') return 1 ;; esac ;;
      *)
        # A subshell's cd does not survive it, but one made BEFORE it does:
        # `cd /a; (cd b && git add .); git commit` commits in /a, not the base.
        # Indexed by hand rather than [-1], which macOS's bash 3.2 lacks.
        case "$tok" in
          ';') state=cmd; continue ;;
          '(') saved+=("$curdir"); state=cmd; continue ;;
          ')')
            depth=${#saved[@]}
            if [ "$depth" -gt 0 ]; then
              curdir=${saved[$((depth - 1))]}
              unset "saved[$((depth - 1))]"
            fi
            state=cmd; continue ;;
        esac ;;
    esac
    case "$state" in
      cmd)
        is_command_prefix "$tok" && continue
        case "$tok" in
          cd|pushd)                    state=cdarg ;;
          git|git.exe|*/git|*/git.exe) state=flags; gitdir=$curdir ;;
          *=*)                         ;;  # env assignment prefix; still a command position
          *)                           state=args ;;
        esac ;;
      cdarg)
        curdir=$(resolve_dir "$curdir" "$tok") || return 1
        state=args ;;
      flags)
        case "$tok" in
          -C)                             state=cflag ;;
          -c|--namespace)                 state=skip ;;
          # An explicit git dir retargets the repo without moving the cwd, and
          # the tree it names may not be a working tree at all. Out of scope.
          --git-dir*|--work-tree*)        return 1 ;;
          commit)                         printf '%s' "$gitdir"; return 0 ;;
          -*)                             ;;
          *)                              state=args ;;
        esac ;;
      cflag)
        gitdir=$(resolve_dir "$gitdir" "$tok") || return 1
        state=flags ;;
      skip)
        state=flags ;;
    esac
  done
  return 1
}

# Moves this hook process to the root of the repo the COMMAND targets, so every
# git query after it -- and every relative read of CHANGELOG.md -- runs against
# the tree the commit will actually land in. One `cd` rather than `git -C` on each
# call: a query added later without its -C would silently read the hook's own
# repo again, and nothing would say so.
#
# Call it after is_git_commit and before any git query. Callers treat failure as
# allow (see commit_target_dir).
anchor_to_repo_root() {
  local target root
  target=$(commit_target_dir) || return 1
  [ -d "$target" ] || return 1
  cd "$target" || return 1
  root=$(git rev-parse --show-toplevel 2>/dev/null) || return 1
  cd "$root" || return 1
}

# Everything below reads the CURRENT directory's repo, so call
# anchor_to_repo_root first.

# Commits that legitimately carry no changelog entry.
is_changelog_exempt() {
  # Merge commits: the merged branches carry their own entries.
  if git rev-parse -q --verify MERGE_HEAD >/dev/null 2>&1; then
    return 0
  fi
  # --amend rewrites a commit that already carried its entry.
  if printf '%s' "$HOOK_COMMAND" | grep -qE 'git[[:space:]]+commit[^&|;]*--amend'; then
    return 0
  fi
  # The initial commit has no HEAD to diff against.
  if ! git rev-parse -q --verify HEAD >/dev/null 2>&1; then
    return 0
  fi
  # Explicit opt-out for reverts, hotfixes, genuinely trivial commits.
  if [ "${SKIP_CHANGELOG:-}" = "1" ] || command_sets_skip_changelog; then
    return 0
  fi
  return 1
}

# True when the COMMAND TEXT carries the opt-out at a command position:
#
#   SKIP_CHANGELOG=1 git commit -m x          -> exempt
#   export SKIP_CHANGELOG=1 && git commit     -> exempt
#   { SKIP_CHANGELOG=1 git commit -m x; }     -> exempt
#   git commit -m "SKIP_CHANGELOG=1 someday"  -> NOT exempt (quoted, so not a command)
#
# Reading only the hook process's own $SKIP_CHANGELOG is not enough, and that is
# not a nicety: the harness spawns the hook itself, so a variable set on the Bash
# tool's command line never reaches it. The documented bypass -- the one the
# block message tells the user to reach for -- did nothing at all, and the commit
# stayed blocked with no way out short of deleting the hook.
# (LL-G kb/claude-code/hook-does-not-inherit-command-env.md)
command_sets_skip_changelog() {
  local tok state result

  noglob_push
  state=cmd
  result=1
  for tok in $(normalize_command); do
    if [ "$tok" = ";" ]; then state=cmd; continue; fi
    # `env` has its own arm below: the assignments after it are still prefixes.
    if [ "$state" = cmd ] && [ "$tok" != env ] && is_command_prefix "$tok"; then continue; fi
    case "$state" in
      cmd|env)
        case "$tok" in
          SKIP_CHANGELOG=1) result=0; break ;;
          export|env)       state=env ;;
          *=*)              ;;  # some other assignment prefix; still a command position
          *)                state=args ;;
        esac
        ;;
    esac
  done
  noglob_pop
  return "$result"
}

# Reads the "version" field out of package.json content on stdin.
read_version() {
  sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -1
}

# The version this commit is shipping, per the working tree. Empty when the repo
# has no package.json -- the template is cloned into non-Node repos too, and a
# gate that blocks every commit there is a gate people rip out.
current_version() {
  [ -f package.json ] || return 0
  read_version < package.json
}

# ---------------------------------------------------------------------------
# The three checks below test STATE and CONSEQUENCE, never the command's text.
#
# A PreToolUse hook fires BEFORE the command runs, so the tempting shortcut is to
# accept any command that *says* it stages the changelog. That is a gate on
# intent, and intent is satisfiable without doing the thing: `git add CHANGELOG.md`
# stages nothing when the file is unmodified, so the exemption fires for exactly
# the commits it exists to stop. This file carried that bug until 0.61.8.
# (LL-G kb/claude-code/hook-validates-text-not-state.md)
#
# Diffing against HEAD instead of the index is what removes the need for any text
# exemption: it sees the edit whether or not it has been staged yet. The tradeoff
# is real and belongs in the block message -- the changelog edit must be its own
# step, before the commit call.
# ---------------------------------------------------------------------------

# True when CHANGELOG.md differs from HEAD (staged or merely edited).
changelog_was_edited() {
  ! git diff --quiet HEAD -- CHANGELOG.md 2>/dev/null
}

# True when package.json's version differs from HEAD's.
version_was_bumped() {
  [ -f package.json ] || return 0
  local now before
  now=$(current_version)
  before=$(git show HEAD:package.json 2>/dev/null | read_version)
  [ -n "$now" ] && [ "$now" != "$before" ]
}

# True when CHANGELOG.md actually names the version being shipped. "The file
# changed" is weak; "the file documents this release" is the invariant, and it is
# the one that catches an edit that silently landed in the wrong section.
changelog_names_version() {
  local v
  v=$(current_version)
  [ -n "$v" ] || return 0
  grep -qF "## [$v]" CHANGELOG.md 2>/dev/null
}

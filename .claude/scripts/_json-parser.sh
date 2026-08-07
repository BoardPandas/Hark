#!/usr/bin/env bash
# Shared JSON field extraction for hook scripts. Source it, do not execute it:
#
#   . "$(dirname "${BASH_SOURCE[0]}")/_json-parser.sh"
#   FILE_PATH=$(json_field "$HOOK_INPUT" tool_input.file_path)
#
# Sourced by path relative to BASH_SOURCE, not $0, so it resolves the same whether
# the caller is executed, sourced, or sourced from a script that was itself sourced.
#
# Why this is a helper rather than a copy per script: the probe below works around
# a bug whose whole character is silence. A drifted copy of THIS code does not
# throw -- it returns "" forever and the hook goes quiet. That is the same reason
# _git-commit-filter.sh exists, and it applies with more force here.
#
# ---------------------------------------------------------------------------
# Why a probe and not `command -v`
#
# On Windows, `command -v python3` SUCCEEDS. It finds the WindowsApps app-
# execution-alias stub, which writes "Python was not found" to stderr (swallowed
# by the customary 2>/dev/null), prints nothing to stdout, and exits 49. A
# `command -v` guard therefore reports a parser present, takes the parser branch,
# and every field comes back "" -- with no error and no non-zero exit. Worse, any
# `else` fallback is unreachable, because the lookup already claimed success. So
# select a parser by RUNNING each candidate against a payload of known shape and
# keeping the first that answers correctly.
# (LL-G kb/bash/command-v-finds-nonexecuting-stub.md)
#
# scripts/check-claude-wiring.mjs check 7b fails the build on the `command -v`
# form, so this cannot regress unnoticed.

JSON_PARSER=""
JSON_PARSER_PROBED=""

# $1 = interpreter, $2 = dotted key path. Payload on stdin, string value on stdout.
# A non-string (object, number, null) yields "" rather than a stringified value --
# callers test for emptiness, and "[object Object]" is not empty.
_json_run() {
  case "$1" in
    node)
      node -e 'let s="";const k=process.argv[1].split(".");process.stdin.on("data",d=>s+=d).on("end",()=>{
         try{let v=JSON.parse(s);for(const p of k)v=(v||{})[p];process.stdout.write(typeof v==="string"?v:"")}catch(e){}
       })' "$2"
      ;;
    *)
      "$1" -c 'import sys,json
try:
    v = json.load(sys.stdin)
    for p in sys.argv[1].split("."):
        v = (v or {}).get(p)
    sys.stdout.write(v if isinstance(v, str) else "")
except Exception:
    pass' "$2"
      ;;
  esac
}

# Probes once per shell, then caches. node first: it is the one interpreter this
# repo already requires (the wiring guard is a .mjs), so it is the least likely
# to be a stub. Leaves JSON_PARSER empty when nothing answers.
json_probe_parser() {
  if [ -n "$JSON_PARSER_PROBED" ]; then return 0; fi
  JSON_PARSER_PROBED=1

  local cand probe
  for cand in node python3 python; do
    command -v "$cand" >/dev/null 2>&1 || continue
    probe=$(printf '{"a":"ok"}' | _json_run "$cand" a 2>/dev/null)
    if [ "$probe" = "ok" ]; then
      JSON_PARSER="$cand"
      return 0
    fi
  done
  return 0
}

# $1 = payload, $2 = dotted key path. Prints the string value, or nothing.
json_field() {
  json_probe_parser
  if [ -z "$JSON_PARSER" ]; then return 0; fi
  printf '%s' "$1" | _json_run "$JSON_PARSER" "$2" 2>/dev/null
}

# Degraded extraction, for when no interpreter answers. Matches a FLAT key, so
# pass the last path segment ("file_path", not "tool_input.file_path").
#
# Stops at the first quote, so a value containing an escaped quote is truncated.
# That is the right trade for identifiers and paths, where a wrong value is worse
# than a short one and embedded quotes essentially do not occur. It is the WRONG
# trade for a blocking gate's subject -- see read_hook_input in
# _git-commit-filter.sh, which falls back to the whole payload instead.
#
# No attempt is made to honor JSON escaping: under MSYS, backslash escapes in a
# grep pattern are not reliably delivered to grep at all (`grep -oE '\\'` fails
# outright with "Trailing backslash" in Git Bash), so an escape-aware pattern
# cannot be written portably here.
# (LL-G kb/bash/msys-path-conversion-corrupts-jq-arg.md)
json_field_flat() {
  printf '%s' "$1" \
    | sed -n "s/.*\"$2\"[[:space:]]*:[[:space:]]*\"\([^\"]*\)\".*/\1/p" \
    | head -1
}

# Over-eager counterpart: everything from the FIRST occurrence of the key to the
# end of the payload, trailing JSON and all. Use for a blocking gate's subject,
# where truncating is the dangerous direction.
#
# Do not be tempted to hand the raw payload to a matcher instead. Consumers here
# strip quoted regions before matching, and in raw JSON the value IS a quoted
# region -- `{"command":"git commit -F m.txt"}` strips down to `{:}` and matches
# nothing. Stripping the key prefix is what leaves the value bare enough to match.
json_field_greedy() {
  local v bs
  v=$(printf '%s' "$1" \
    | grep -o "\"$2\"[[:space:]]*:[[:space:]]*\".*" \
    | head -1 \
    | sed "s/^\"$2\"[[:space:]]*:[[:space:]]*\"//")

  # Break on JSON's own punctuation so a shell tokenizer downstream sees real
  # words. Without this the trailing JSON glues onto the final token
  # (`commit"}}` is not `commit`) and an escaped newline glues two commands into
  # one (`x\ngit` is not `git`) -- both of which read as "no commit here".
  #
  # Parameter expansion, not sed: MSYS rewrites backslashes in a command's argv,
  # so `sed 's/\\n/ /g'` silently matches nothing under Git Bash while working
  # on Linux. A transform that quietly does nothing on one platform is the exact
  # failure mode this file exists to prevent. Bash does the substitution itself,
  # so both platforms get the same result.
  #
  # The pattern must stay double-quoted. Unquoted, bash reads the backslash as
  # an escape and leaves a stray one behind (`x\ ; git`).
  bs='\'
  v=${v//"${bs}n"/ ; }
  v=${v//\"/ ; }
  printf '%s' "$v"
}

# Probe at source time, in the CALLER's shell. json_field runs its extraction in
# a command substitution, and a probe there would set JSON_PARSER only inside
# that subshell -- re-probing on every single call, and leaving JSON_PARSER
# permanently empty for any caller that wants to branch on it.
json_probe_parser

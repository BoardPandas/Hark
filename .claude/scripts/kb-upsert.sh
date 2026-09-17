#!/usr/bin/env bash
#
# kb-upsert.sh -- create or update a single file in a GitHub repo via the
# contents API, handling the blob-SHA dance and base64 encoding for you.
#
# Used by the add-lesson (LL-G) and add-practice (BP) skills so they don't have
# to capture SHAs by hand or rely on the GNU-only `base64 -w0` flag.
#
# Usage:
#   kb-upsert.sh <repo> <path> <content-file> <commit-message> [branch]
#
#   repo            owner/name, e.g. BoardPandas/LL-G
#   path            path within the repo, e.g. kb/powershell/quoting.md
#   content-file    local file whose contents become the file body
#   commit-message  commit message (quote it)
#   branch          target branch (default: main)
#
# Behaviour:
#   - If the path already exists, its current SHA is fetched and the file is
#     updated (no lost-update race: the SHA is read immediately before the PUT).
#   - If the path does not exist (404), it is created.
#   - On success, prints the file's html_url.
#
# Requires: gh (authenticated), base64, tr.

set -euo pipefail

if [ "$#" -lt 4 ]; then
  echo "usage: kb-upsert.sh <repo> <path> <content-file> <commit-message> [branch]" >&2
  exit 64
fi

repo="$1"
path="$2"
content_file="$3"
message="$4"
branch="${5:-main}"

if [ ! -f "$content_file" ]; then
  echo "kb-upsert: content file not found: $content_file" >&2
  exit 66
fi

# Portable base64 with no line wrapping: GNU wraps at 76 cols, BSD at 64; both
# are flattened by stripping newlines. Avoids the GNU-only `-w0` flag.
#
# The result is held in a variable and later written to STDIN, never passed as
# an argument. base64 inflates by 4/3, so `-f content=...` made the argument
# ~75 KB for the 56 KB LL-G master index and blew the ~32 KB Windows/msys argv
# limit ("Argument list too long", exit 126). That threshold gets crossed
# silently as an index grows: entry files (~6 KB) kept working long after the
# master index stopped. Variables and pipes have no comparable limit.
content_b64="$(base64 "$content_file" | tr -d '\r\n')"

# Read the current SHA immediately before the PUT so the update is not racing a
# stale value. A 404 (file does not exist yet) is expected for new entries.
# The lookup must carry ?ref=, or it reads the DEFAULT branch: updating any
# other branch would then 404, fall through to the create path, and fail with
# 422 because the file it is 'creating' already exists there.
#
# gh prints an API error body on STDOUT, not stderr, so 2>/dev/null does NOT
# make a missing file yield an empty string -- it yields the 404 JSON blob.
# Accept the result only when it actually looks like a blob SHA; anything else
# means "not there yet", which is the create path. Without this guard the error
# blob was passed along as the sha: harmless-looking, because GitHub ignores a
# bogus sha when creating a new file, so it stayed invisible for as long as the
# value went out through a field flag.
sha="$(gh api "repos/${repo}/contents/${path}?ref=${branch}" --jq .sha 2>/dev/null || true)"
if [[ ! $sha =~ ^[0-9a-f]{40}$ && ! $sha =~ ^[0-9a-f]{64}$ ]]; then
  sha=""
fi

# Escape a string for use inside a JSON string literal. Pure bash parameter
# expansion, so the script gains no dependency (jq is not installed everywhere
# these skills run).
#
# Backslash MUST be substituted first: doing it after the others would
# re-escape the backslashes those rules introduce, turning a quote escape into
# a literal backslash followed by an unescaped quote.
#
# This covers what a commit message realistically holds. Other C0 control bytes
# (a bare form feed, say) would still produce invalid JSON, but the API rejects
# that with a loud 400 rather than writing something wrong.
json_escape() {
  local s=$1
  s=${s//\\/\\\\}
  s=${s//\"/\\\"}
  s=${s//$'\t'/\\t}
  s=${s//$'\r'/\\r}
  s=${s//$'\n'/\\n}
  printf '%s' "$s"
}

# --input takes the entire request body, and field flags cannot be mixed with
# it (gh appends those to the query string instead), so every field is
# assembled here. The base64 alphabet needs no escaping; only the caller's
# message and branch do.
build_body() {
  printf '{"message":"%s","branch":"%s","content":"%s"' \
    "$(json_escape "$message")" "$(json_escape "$branch")" "$content_b64"
  if [ -n "$sha" ]; then
    printf ',"sha":"%s"' "$sha"
  fi
  printf '}'
}

build_body | gh api "repos/${repo}/contents/${path}" \
  --method PUT --input - --jq '.content.html_url'

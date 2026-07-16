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
content_b64="$(base64 "$content_file" | tr -d '\r\n')"

# Read the current SHA immediately before the PUT so the update is not racing a
# stale value. A 404 (file does not exist yet) is expected for new entries.
sha="$(gh api "repos/${repo}/contents/${path}" --jq .sha 2>/dev/null || true)"

args=(
  --method PUT
  -f "message=${message}"
  -f "branch=${branch}"
  -f "content=${content_b64}"
)
if [ -n "$sha" ]; then
  args+=(-f "sha=${sha}")
fi

gh api "repos/${repo}/contents/${path}" "${args[@]}" --jq '.content.html_url'

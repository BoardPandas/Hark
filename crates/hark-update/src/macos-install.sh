#!/bin/sh
# Paths are positional parameters, never interpolated into shell source.
set -eu
parent_pid=$1
installed=$2
workspace=$3
requirement=$4
relaunch_flag=$5
incoming="$workspace/Hark.app"
backup="$workspace/Previous.app"
# If a late revalidation or filesystem operation fails, bring the old app
# back. Keep workspace/logs on failure so the user can inspect the cause.
recover() {
    result=$?
    if [ "$result" -ne 0 ]; then
        if [ ! -e "$installed" ] && [ -d "$backup" ]; then
            /bin/mv "$backup" "$installed" || true
        fi
        if [ -d "$installed" ] && ! kill -0 "$parent_pid" 2>/dev/null; then
            /usr/bin/open "$installed" --args "$relaunch_flag" || true
        fi
    fi
}
trap recover EXIT
count=0
while kill -0 "$parent_pid" 2>/dev/null; do
    count=$((count + 1))
    if [ "$count" -ge 120 ]; then
        echo 'Hark did not exit; update cancelled without modifying the installed app.' >&2
        exit 1
    fi
    sleep 1
done
/usr/bin/codesign --verify --deep --strict -R "$requirement" "$incoming"
/usr/sbin/spctl --assess --type execute "$incoming"
/bin/mv "$installed" "$backup"
if ! /bin/mv "$incoming" "$installed"; then
    /bin/mv "$backup" "$installed"
    exit 1
fi
if ! /usr/bin/open "$installed" --args "$relaunch_flag"; then
    /bin/mv "$installed" "$incoming"
    /bin/mv "$backup" "$installed"
    exit 1
fi
/bin/rm -rf "$workspace"

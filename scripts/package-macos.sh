#!/bin/bash
# Build a signed bundle and DMG from an already-built native Rust executable.
# Ad-hoc signing is for local/CI inspection only; release mode requires notarization.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
binary=${1:?Usage: package-macos.sh BINARY OUTPUT_DIRECTORY VERSION arm64-or-x64}
output=${2:?Missing output directory}
version=${3:?Missing version}
arch=${4:?Missing architecture}
case "$arch" in arm64|x64) ;; *) echo 'Expected arm64 or x64' >&2; exit 1 ;; esac
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo 'Expected a release SemVer' >&2; exit 1; }
identity=${MACOS_SIGNING_IDENTITY:--}
if [[ "${HARK_RELEASE:-0}" == 1 ]]; then
    [[ "$identity" != - ]] || { echo 'A Developer ID signing identity is required' >&2; exit 1; }
    : "${MACOS_NOTARY_PROFILE:?A notarytool keychain profile is required}"
    notary_args=(--keychain-profile "$MACOS_NOTARY_PROFILE")
    if [[ -n "${MACOS_NOTARY_KEYCHAIN:-}" ]]; then
        notary_args+=(--keychain "$MACOS_NOTARY_KEYCHAIN")
    fi
fi
mkdir -p "$output"
output=$(cd "$output" && pwd)
work=$(mktemp -d "$output/.macos-package.XXXXXX")
trap 'rm -rf "$work"' EXIT
app="$work/image/Hark.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/hark-app"
chmod 755 "$app/Contents/MacOS/hark-app"
cp "$root/packaging/macos/Info.plist" "$app/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $version" "$app/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $version" "$app/Contents/Info.plist"
cp "$root/LICENSE" "$root/THIRD_PARTY_NOTICES.md" "$app/Contents/Resources/"
python3 "$root/scripts/macos-icon.py" "$work/Hark.iconset"
/usr/bin/iconutil -c icns "$work/Hark.iconset" -o "$app/Contents/Resources/Hark.icns"
# Native dependencies are linked statically. Refuse a build that would only
# work on its build machine instead of shipping an unresolved dylib reference.
linked=$(/usr/bin/otool -L "$app/Contents/MacOS/hark-app")
while IFS= read -r dependency; do
    case "$dependency" in /System/Library/*|/usr/lib/*|'') ;; *)
        echo "Unbundled native dependency: $dependency" >&2; exit 1 ;;
    esac
done < <(printf '%s\n' "$linked" | tail -n +2 | awk '{print $1}')
expected=arm64
[[ "$arch" == x64 ]] && expected=x86_64
/usr/bin/lipo -verify_arch "$expected" "$app/Contents/MacOS/hark-app" || {
    echo "Executable does not contain the requested $expected architecture" >&2; exit 1;
}
/usr/bin/plutil -lint "$app/Contents/Info.plist" "$root/packaging/macos/entitlements.plist"
sign_args=(--force --sign "$identity" --options runtime --entitlements "$root/packaging/macos/entitlements.plist")
[[ "$identity" == - ]] || sign_args+=(--timestamp)
/usr/bin/codesign "${sign_args[@]}" "$app"
/usr/bin/codesign --verify --deep --strict "$app"
if [[ "${HARK_RELEASE:-0}" == 1 ]]; then
    /usr/bin/ditto -c -k --keepParent "$app" "$work/Hark.zip"
    /usr/bin/xcrun notarytool submit "$work/Hark.zip" "${notary_args[@]}" --wait
    /usr/bin/xcrun stapler staple "$app"
    /usr/bin/xcrun stapler validate "$app"
    /usr/sbin/spctl --assess --type execute "$app"
fi
ln -s /Applications "$work/image/Applications"
name="Hark-$version-macos-$arch.dmg"
asset="$work/$name"
/usr/bin/hdiutil create -volname Hark -srcfolder "$work/image" -ov -format UDZO "$asset"
if [[ "${HARK_RELEASE:-0}" == 1 ]]; then
    /usr/bin/codesign --force --sign "$identity" --timestamp "$asset"
    /usr/bin/xcrun notarytool submit "$asset" "${notary_args[@]}" --wait
    /usr/bin/xcrun stapler staple "$asset"
    /usr/bin/xcrun stapler validate "$asset"
fi
# Keep the bundle available for local launch/inspection as well as the DMG.
# Remove a previous output first: ditto would retain stale sealed resources.
rm -rf "$output/Hark.app"
/usr/bin/ditto "$app" "$output/Hark.app"
/usr/bin/codesign --verify --deep --strict "$output/Hark.app"
# Publish the DMG into the output directory only after every signing and
# notarization gate succeeds. A failed release leaves the prior output intact.
mv -f "$asset" "$output/$name"
printf 'Created %s\n' "$output/$name"

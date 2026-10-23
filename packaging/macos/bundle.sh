#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
version="${VERSION:?set VERSION to the release version without the leading v}"
binary="${BINARY:-$root/target/release/oxidcord}"
out_dir="${OUT_DIR:-$root/dist}"

case "${ARCH:-$(uname -m)}" in
	arm64 | aarch64) target="macos-aarch64" ;;
	x86_64) target="macos-x86_64" ;;
	*) echo "unsupported architecture" >&2; exit 1 ;;
esac

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
app="$work/Oxidcord.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$out_dir"

cp "$binary" "$app/Contents/MacOS/oxidcord"
sed "s/@VERSION@/$version/g" "$root/packaging/macos/Info.plist" > "$app/Contents/Info.plist"

iconset="$work/icon.iconset"
mkdir -p "$iconset"
for size in 16 32 128 256; do
	sips -z "$size" "$size" "$root/assets/icon.png" --out "$iconset/icon_${size}x${size}.png" > /dev/null
	sips -z "$((size * 2))" "$((size * 2))" "$root/assets/icon.png" --out "$iconset/icon_${size}x${size}@2x.png" > /dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/icon.icns"

if [ -n "${MACOS_SIGN_IDENTITY:-}" ]; then
	codesign --force --options runtime --timestamp \
		--entitlements "$root/packaging/macos/entitlements.plist" \
		--sign "$MACOS_SIGN_IDENTITY" "$app"
	codesign --verify --deep --strict "$app"
fi

stage="$work/dmg"
mkdir -p "$stage"
cp -R "$app" "$stage/Oxidcord.app"
ln -s /Applications "$stage/Applications"

dmg="$out_dir/oxidcord-$version-$target-dmg.dmg"
rm -f "$dmg"
hdiutil create -volname "Oxidcord" -srcfolder "$stage" -ov -format UDZO "$dmg"

if [ -n "${MACOS_SIGN_IDENTITY:-}" ]; then
	codesign --force --timestamp --sign "$MACOS_SIGN_IDENTITY" "$dmg"
fi

echo "$dmg"

#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
version="${VERSION:?set VERSION to the release version without the leading v}"
arch="${ARCH:-$(uname -m)}"
binary="${BINARY:-$root/target/release/oxidcord}"
out_dir="${OUT_DIR:-$root/dist}"
app_id="dev.oxidcord.Oxidcord"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
appdir="$work/AppDir"
mkdir -p "$appdir" "$out_dir"

cp "$root/assets/icon.png" "$work/$app_id.png"

export ARCH="$arch"
export APPIMAGE_EXTRACT_AND_RUN=1

linuxdeploy \
	--appdir "$appdir" \
	--executable "$binary" \
	--desktop-file "$root/packaging/linux/oxidcord.desktop" \
	--icon-file "$work/$app_id.png" \
	--icon-filename "$app_id"

install -Dm644 "$root/packaging/linux/$app_id.metainfo.xml" \
	"$appdir/usr/share/metainfo/$app_id.metainfo.xml"

output="$out_dir/oxidcord-$version-linux-$arch-appimage.AppImage"
appimagetool "$appdir" "$output"
chmod +x "$output"
echo "$output"

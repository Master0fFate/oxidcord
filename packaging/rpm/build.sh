#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
version="${VERSION:?set VERSION to the release version without the leading v}"
arch="${ARCH:-$(uname -m)}"
out_dir="${OUT_DIR:-$root/dist}"
mkdir -p "$out_dir"

overrides=()
if [[ "$version" == *-* ]]; then
	prerelease="${version#*-}"
	overrides+=(--set-metadata "version = \"${version%%-*}\"" --set-metadata "release = \"0.${prerelease//-/.}\"")
fi

cd "$root"
cargo generate-rpm -p apps/desktop "${overrides[@]}" -o "$out_dir/oxidcord-$version-linux-$arch-rpm.rpm"

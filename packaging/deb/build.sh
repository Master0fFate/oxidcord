#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
version="${VERSION:?set VERSION to the release version without the leading v}"
arch="${ARCH:-$(uname -m)}"
out_dir="${OUT_DIR:-$root/dist}"
mkdir -p "$out_dir"

cd "$root"
cargo deb -p oxidcord --no-build --deb-version "$version" -o "$out_dir/oxidcord-$version-linux-$arch-deb.deb"

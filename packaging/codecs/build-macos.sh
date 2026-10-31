#!/usr/bin/env bash
set -euo pipefail

version="8.0.1"
sha256="05ee0b03119b45c0bdb4df654b96802e909e0a752f72e4fe3794f487229e5a41"
out="${OUT:?set OUT to the codecs directory to fill}"
work="${WORK:-$(mktemp -d)}"

decoders="h264,hevc,vp8,vp9,av1,aac,mp3,vorbis,flac,alac"

mkdir -p "$out" "$work"
archive="$work/ffmpeg-$version.tar.xz"
source="$work/ffmpeg-$version"
prefix="$work/prefix"

curl -fsSL "https://ffmpeg.org/releases/ffmpeg-$version.tar.xz" -o "$archive"
echo "$sha256  $archive" | shasum -a 256 --check

tar xf "$archive" -C "$work"
cd "$source"
./configure \
	--prefix="$prefix" \
	--disable-everything \
	--disable-programs \
	--disable-doc \
	--disable-debug \
	--disable-network \
	--disable-autodetect \
	--disable-static \
	--enable-shared \
	--enable-swresample \
	--enable-decoder="$decoders"
make -j"$(sysctl -n hw.ncpu)"
make install

for library in libavcodec libavutil libswresample; do
	built="$(compgen -G "$prefix/lib/$library.*.dylib" | head -1)"
	name="$(basename "$built")"
	cp "$built" "$out/$name"
	install_name_tool -id "@loader_path/$name" "$out/$name"
	otool -L "$out/$name" | awk 'NR > 1 { print $1 }' | grep "^$prefix/lib/" | while read -r dependency; do
		install_name_tool -change "$dependency" "@loader_path/$(basename "$dependency")" "$out/$name"
	done
done

cp "$source/COPYING.LGPLv2.1" "$out/LICENSE-FFMPEG.txt"
echo "codec pack installed into $out"
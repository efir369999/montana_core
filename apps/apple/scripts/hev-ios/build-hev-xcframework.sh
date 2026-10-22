#!/bin/bash
# HevSocks5Tunnel.xcframework — the userspace TCP/IP stack that moves packets between the tunnel and
# Xray's local SOCKS inbound. Source: github.com/heiher/hev-socks5-tunnel (MIT, C, lwIP).
#
# One framework, both slices: phone (ios-arm64) and Mac (ios-arm64-maccatalyst). The extension is the
# same code on both, so a per-platform framework would be two sources of truth for one thing.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SRC="${1:-$HERE/build/hev-src}"
# the second word names the framework's place (the dependency road gives each build its own fresh folder)
OUT="${2:-$HERE/Frameworks/HevSocks5Tunnel.xcframework}"

# Reproducible by any builder (Constitution [I-14], OZ 1): the source is one pinned commit, its submodules the ones it records,
# never the moving head of the upstream (22.10.2026: the clone took whatever head it found, 9b2c2320 that day)
PIN=9b2c2320b26046b307abd6ddb5f102b2ccd1d4a4
if [ ! -d "$SRC/.git" ]; then
  mkdir -p "$(dirname "$SRC")"
  git clone https://github.com/heiher/hev-socks5-tunnel "$SRC"
fi
git -C "$SRC" checkout --quiet "$PIN"
git -C "$SRC" submodule update --init --recursive --quiet
[ "$(git -C "$SRC" rev-parse HEAD)" = "$PIN" ] || { echo "REFUSED: hev-socks5-tunnel is not at $PIN"; exit 3; }
[ -z "$(git -C "$SRC" status --porcelain --ignore-submodules=none)" ] || { echo "REFUSED: hev-socks5-tunnel carries local changes"; exit 3; }

STAGE="$(mktemp -d)"
mkdir -p "$STAGE/inc"
cp "$SRC/src/hev-main.h" "$STAGE/inc/"

slice() {   # $1 = slice name, $2 = sdk, $3 = target flag
  make -C "$SRC" clean >/dev/null 2>&1 || true
  # the builder's folder never reaches the objects: every path the compiler writes is rewritten to /montana/hev
  make -C "$SRC" -j"$(sysctl -n hw.ncpu)" \
    PP="xcrun --sdk $2 clang" CC="xcrun --sdk $2 clang" \
    CFLAGS="$3 -ffile-prefix-map=$SRC=/montana/hev" LFLAGS="$3 -Wl,-Bsymbolic-functions" \
    AR="$HERE/scripts/hev-ios/archive.sh" static >/dev/null
  mkdir -p "$STAGE/$1"
  # -D: the archive carries no time, owner or mode of this Mac (its members were born so by archive.sh)
  ZERO_AR_DATE=1 libtool -static -D -o "$STAGE/$1/libhev-socks5-tunnel.a" \
    "$SRC/bin/libhev-socks5-tunnel.a" "$SRC/third-part/lwip/bin/liblwip.a" \
    "$SRC/third-part/yaml/bin/libyaml.a" "$SRC/third-part/hev-task-system/bin/libhev-task-system.a"
  echo "-- slice $1"
}
slice phone  iphoneos "-arch arm64 -mios-version-min=15.0"
slice mac    macosx   "-target arm64-apple-ios15.0-macabi"

if [ -d "$OUT" ]; then
  python3 -c "import os,sys,time; os.rename(sys.argv[1], sys.argv[1]+'.prev-'+str(int(time.time())))" "$OUT"
fi
xcodebuild -create-xcframework \
  -library "$STAGE/phone/libhev-socks5-tunnel.a" -headers "$STAGE/inc" \
  -library "$STAGE/mac/libhev-socks5-tunnel.a"   -headers "$STAGE/inc" \
  -output "$OUT" >/dev/null
echo "tunnel slices: $(ls "$OUT" | grep -v Info.plist | tr '\n' ' ')"

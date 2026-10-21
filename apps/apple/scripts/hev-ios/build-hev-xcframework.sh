#!/bin/bash
# HevSocks5Tunnel.xcframework — the userspace TCP/IP stack that moves packets between the tunnel and
# Xray's local SOCKS inbound. Source: github.com/heiher/hev-socks5-tunnel (MIT, C, lwIP).
#
# One framework, both slices: phone (ios-arm64) and Mac (ios-arm64-maccatalyst). The extension is the
# same code on both, so a per-platform framework would be two sources of truth for one thing.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SRC="${1:-$HERE/build/hev-src}"
OUT="$HERE/Frameworks/HevSocks5Tunnel.xcframework"

if [ ! -d "$SRC/.git" ]; then
  mkdir -p "$(dirname "$SRC")"
  git clone --recursive --depth 1 https://github.com/heiher/hev-socks5-tunnel "$SRC"
fi

STAGE="$(mktemp -d)"
mkdir -p "$STAGE/inc"
cp "$SRC/src/hev-main.h" "$STAGE/inc/"

slice() {   # $1 = slice name, $2 = sdk, $3 = target flag
  make -C "$SRC" clean >/dev/null 2>&1 || true
  taskpolicy -b make -C "$SRC" -j2 \
    PP="xcrun --sdk $2 clang" CC="xcrun --sdk $2 clang" \
    CFLAGS="$3" LFLAGS="$3 -Wl,-Bsymbolic-functions" static >/dev/null
  mkdir -p "$STAGE/$1"
  libtool -static -o "$STAGE/$1/libhev-socks5-tunnel.a" \
    "$SRC/bin/libhev-socks5-tunnel.a" "$SRC/third-part/lwip/bin/liblwip.a" \
    "$SRC/third-part/yaml/bin/libyaml.a" "$SRC/third-part/hev-task-system/bin/libhev-task-system.a"
  echo "-- slice $1"
}
slice phone  iphoneos "-arch arm64 -mios-version-min=15.0"
slice mac    macosx   "-target arm64-apple-ios15.0-macabi"

if [ -d "$OUT" ]; then
  python3 -c "import os,sys,time; os.rename(sys.argv[1], sys.argv[1]+'.prev-'+str(int(time.time())))" "$OUT"
fi
taskpolicy -b xcodebuild -create-xcframework \
  -library "$STAGE/phone/libhev-socks5-tunnel.a" -headers "$STAGE/inc" \
  -library "$STAGE/mac/libhev-socks5-tunnel.a"   -headers "$STAGE/inc" \
  -output "$OUT" >/dev/null
echo "tunnel slices: $(ls "$OUT" | grep -v Info.plist | tr '\n' ' ')"

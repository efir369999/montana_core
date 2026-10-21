#!/bin/bash
# MontanaXray.xcframework — the Xray-core engine for the packet-tunnel extension.
# Source: github.com/xtls/xray-core (official XTLS).
#
# One framework, both slices: phone (ios-arm64) and Mac (ios-arm64-maccatalyst). gomobile must run
# with this directory as the current module — it looks for golang.org/x/mobile in the module of the
# working directory, not of the package given on the command line.
#
# Mac slice is arm64 only: Apple's clang refuses the Intel Catalyst target this toolchain asks for
# ("invalid version number in -target x86_64-apple-ios13.0-macabi"), and the Rust core is arm64-only
# on this platform as well — one architecture on both sides, no half-built framework.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
export PATH="$PATH:$(go env GOPATH)/bin"
# Every core (the author's word 29.09: the limits are lifted): no GOMAXPROCS, no background policy. The gomobile tools
# are installed once; a machine that already holds them builds without the network.
command -v gomobile >/dev/null 2>&1 || go install golang.org/x/mobile/cmd/gomobile@latest
command -v gobind >/dev/null 2>&1 || go install golang.org/x/mobile/cmd/gobind@latest
STAGE="$(mktemp -d)"
python3 - "$HERE" "$STAGE/MontanaXray.xcframework" <<'PY'
import os, subprocess, sys
here, out = sys.argv[1], sys.argv[2]
r = subprocess.run(["gomobile","bind","-target=ios,maccatalyst/arm64","-iosversion=15.0","-o",out,"."], cwd=here)
sys.exit(r.returncode)
PY
DEST="$ROOT/Code/Frameworks/MontanaXray.xcframework"   # where the project links it from
if [ -d "$DEST" ]; then
  python3 -c "import os,sys,time; os.rename(sys.argv[1], sys.argv[1]+'.prev-'+str(int(time.time())))" "$DEST"
fi
cp -R "$STAGE/MontanaXray.xcframework" "$DEST"
echo "engine slices: $(ls "$DEST" | grep -v Info.plist | tr '\n' ' ')"

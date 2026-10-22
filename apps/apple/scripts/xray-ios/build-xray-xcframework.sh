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
#
# Reproducible (Constitution [I-14]; 2274 carried 1561 paths of the Go module cache and of the session tree it was built in):
# the Go toolchain is the one go.mod names, gomobile and gobind are the x/mobile version go.mod pins (never whatever is newest), and
# -trimpath with no VCS stamp leaves no path and no checkout state of the builder in the engine. Two clocks remain in what gomobile
# hands back (measured 21.10.2026: two binds of one tree differed in 23 bytes and in Info.plist): Apple's ar stamps each archive
# member with the time it ran -- ZERO_AR_DATE=1 writes zero instead -- and gomobile names the framework version 0.0.UNIX-SECONDS
# (cmd/gomobile/bind_iosapp.go) -- the version becomes the xray-core release go.mod pins. One path remains, not trimmed by -trimpath:
# gomobile replaces this module with its directory, and Go writes that directory into the build information the app carries
# (measured 21.10.2026, Agent 217: «montanaxray => /Users/.../scripts/xray-ios» in the tree build, «/private/tmp/.../scripts/xray-ios»
# in the public-slice build). The module is bound from one fixed directory, the same for every builder, under a lock.
# Usage: build-xray-xcframework.sh [TARGETS DEST] -- by default both slices into Code/Frameworks, where the phone links them.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
TARGETS="${1:-ios,maccatalyst/arm64}"
DEST="${2:-$ROOT/Code/Frameworks/MontanaXray.xcframework}"
GO_VERSION="$(sed -n 's/^go //p' "$HERE/go.mod")"
export GOTOOLCHAIN="go$GO_VERSION" GOFLAGS="-buildvcs=false -mod=readonly"
python3 - "$HERE" "$DEST" "$TARGETS" <<'PY'
import fcntl, os, plistlib, shutil, subprocess, sys, tempfile, time
source, dest, targets = sys.argv[1:]
here = '/private/tmp/montana-xray-ios'
lock = open(here + '.lock', 'w')
fcntl.flock(lock, fcntl.LOCK_EX)
shutil.rmtree(here, ignore_errors=True)
os.makedirs(here)
for name in sorted(os.listdir(source)):
    if name in ('go.mod', 'go.sum') or (name.endswith('.go') and not name.endswith('_test.go')):
        shutil.copyfile(os.path.join(source, name), os.path.join(here, name))
with tempfile.TemporaryDirectory(prefix='xray-') as work:
    env = dict(os.environ, GOBIN=os.path.join(work, 'bin'), ZERO_AR_DATE='1', PWD=here)
    subprocess.run(['go', 'install', 'golang.org/x/mobile/cmd/gomobile', 'golang.org/x/mobile/cmd/gobind'], cwd=here, env=env, check=True)
    env['PATH'] = env['GOBIN'] + os.pathsep + env['PATH']
    out = os.path.join(work, 'MontanaXray.xcframework')
    subprocess.run([os.path.join(env['GOBIN'], 'gomobile'), 'bind', '-trimpath', '-target=' + targets, '-iosversion=15.0',
                    '-o', out, '.'], cwd=here, env=env, check=True)
    engine = next(l.split()[-1].lstrip('v') for l in open(os.path.join(here, 'go.mod')) if l.split()[:2] == ['require', 'github.com/xtls/xray-core'])
    for d, _, files in os.walk(out):
        if 'Info.plist' in files and '.framework' in d:
            p = os.path.join(d, 'Info.plist')
            with open(p, 'rb') as f:
                info = plistlib.load(f)
            info['CFBundleShortVersionString'] = info['CFBundleVersion'] = engine
            with open(p, 'wb') as f:
                plistlib.dump(info, f)
    if os.path.isdir(dest):
        os.rename(dest, dest + '.prev-' + str(int(time.time())))
    shutil.copytree(out, dest, symlinks=True)
shutil.rmtree(here, ignore_errors=True)
print('engine slices: ' + ' '.join(sorted(n for n in os.listdir(dest) if n != 'Info.plist')))
PY

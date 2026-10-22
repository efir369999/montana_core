#!/usr/bin/env python3
"""Build Montana Core for Mac from this source and compare it with a published release, byte for byte."""
# Constitution [I-14]: «Every release build of an official Montana client is reproducible byte for byte from publicly published
# source by any independent builder.» The author's words 21.10.2026 20:4x and 20:58 MSK: «there is no reproducible build of the
# applications -- fix it», «while you wait for OZ 1, close OZ 2».
#
# One archive road for the release and for anyone who checks it: the release road (mt-mac-release.sh) archives through --archive
# with its signing switches after «--», a builder without Montana's keys archives through the same call with signing off. A signature
# is the one thing no other builder can make, so the canonical form of an app is the app with every signature removed:
# _CodeSignature folders, CodeResources (where the notarization ticket is stapled) and embedded.provisionprofile are left out,
# and every Mach-O is passed through «codesign --remove-signature». Measured on 2274 (21.10.2026): the archive's app and the app
# exported for Developer ID are equal in this form, file for file (53 files) -- the export changes signatures and nothing else.
# Info.plist carries BuildMachineOSBuild, the build of macOS the builder runs: it names a machine, not the source, so --scrub
# removes it from the product before the export signs it, on both roads alike.
#
# What no flag reaches is made canonical in the product itself, before the export signs it, on both roads alike (measured 22.10.2026,
# Agent 218 and Agent 219, the tree build against the public-slice build and actool run four times on one tree):
# - the Swift compiler writes a file's name remapped by -file-prefix-map yet its length from the path it read
#   (swift_isEscapingClosureAtFileLocation), and the Swift packages compile inside the build home: every build is made from one copy
#   of the tree in one folder, the same for every builder, one build at a time;
# - Xcode rewrites an Info.plist with its keys in the random order of its own dictionaries: every Info.plist is written back with
#   sorted keys, in its own format;
# - actool gives the appearance names of Assets.car their blocks in a random order, names the 1024 icon it renders from AppIcon.icon
#   after a temporary file (UUID, process, time), and lets the AppIcon facet name either of two parts of its renditions: the blocks
#   are renumbered in the order of the tree, the temporary part of the name is cut, the facet names the highest part it holds;
# - the linker builds stubs and GOT in parallel, and a target held by two GOT slots -- _objc_msgSend, once for the stubs and once for
#   the selector stubs, and a function folded by deduplication -- gets them in either order (Agent 220: one link command four times,
#   three different binaries): with deduplication off and the selector stubs branching to the one stub, no target holds two slots.
#
# Usage, from a clone of montana_core (apps/apple):
#   python3 tools/mt-reproduce-mac.py --core ../../core/Code --compare Montana-Core-1.0-N.zip
# prints the reproducible digest of the rebuilt app and of the release, and every file that differs. Needs the Xcode, Rust and
# Go versions the release names (Xcode 26.2 17C52, Rust 1.92.0 with aarch64-apple-ios-macabi, Go from go.mod).
import argparse
import fcntl
import hashlib
import os
from pathlib import Path
import plistlib
import re
import shutil
import struct
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent
SETTINGS = ['ARCHS=arm64', 'MONTANA_APP_ID=quest.montana.app', 'MTL_ENABLE_DEBUG_INFO=NO', 'COMPILER_INDEX_STORE_ENABLE=NO',
            'DEVELOPMENT_TEAM=S8JCA5MBVD', 'OTHER_LDFLAGS=$(inherited) -Xlinker -no_deduplicate -Xlinker -objc_stubs_small']
LEFT_OUT_DIRS = {'_CodeSignature'}
LEFT_OUT_FILES = {'CodeResources', 'embedded.provisionprofile'}
BUILDER_KEYS = ['BuildMachineOSBuild']
FIXED = Path('/private/tmp/montana-apple')
ICON_NAME = re.compile(rb'(AppIcon[^\x00/]*?)_[0-9A-F]{8}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{12}-[0-9]+-[0-9A-F]{16}(\.png)\x00')
ELEMENT, PART, IDENTIFIER = 1, 2, 17


def run(args, **kwargs):
    return subprocess.run([str(x) for x in args], check=True, **kwargs)


def remove(path):
    if path.is_symlink() or path.is_file():
        path.unlink()
    elif path.is_dir():
        shutil.rmtree(path)


def framework_links(src, frameworks):
    # Xcode processes the frameworks MONTANA_FRAMEWORKS_DIR names on the command line, yet plans the archive against the project's
    # own value, $(PROJECT_DIR)/Code/Frameworks, and silently drops a framework missing there: a clean clone (no links, .gitignore)
    # then fails with «Unable to find module dependency: 'MontanaBindings'». Measured 21.10.2026: repro-209 without the links failed
    # three times with no ProcessXCFramework; with links to the same frameworks it archived. Whatever the tree held there gives way.
    here = src / 'Code/Frameworks'
    for name in sorted(os.listdir(here)):
        if name.endswith('.xcframework'):
            remove(here / name)
    for name in sorted(os.listdir(frameworks)):
        if name.endswith('.xcframework'):
            (here / name).symlink_to(frameworks / name)


def archive(path, frameworks, extra):
    jobs = os.environ.get('MT_CORES') or str(os.cpu_count())
    src, linked, home = FIXED / 'src', FIXED / 'frameworks', FIXED / 'home'
    with open(str(FIXED) + '.lock', 'w') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        remove(src)
        remove(linked)
        if home.is_dir():
            for name in os.listdir(home):
                if name != 'SourcePackages':
                    remove(home / name)
        for folder in (src, linked, home):
            folder.mkdir(parents=True, exist_ok=True)
        for name in sorted(os.listdir(ROOT)):
            if name != '.git' and not name.startswith('build'):
                run(['/bin/cp', '-cR', ROOT / name, src / name])
        for name in sorted(os.listdir(frameworks)):
            if name.endswith('.xcframework'):
                run(['/bin/cp', '-cR', Path(frameworks) / name, linked / name])
        framework_links(src, linked)
        # Xcode's own tools keep Swift dictionaries, whose order is seeded at random per process, so the hashing seed is fixed.
        run(['xcodebuild', '-project', src / 'Montana.xcodeproj', '-scheme', 'Montana', '-configuration', 'Release',
             '-destination', 'generic/platform=macOS,variant=Mac Catalyst', '-jobs', jobs, '-derivedDataPath', home,
             '-archivePath', path, '-onlyUsePackageVersionsFromResolvedFile', 'archive', 'MONTANA_FRAMEWORKS_DIR=' + str(linked)]
            + SETTINGS + extra, env=dict(os.environ, SWIFT_DETERMINISTIC_HASHING='1'))
        scrub(Path(path) / 'Products/Applications/Montana Core.app')


def canonical_plist(path):
    with open(path, 'rb') as f:
        raw = f.read()
    info = plistlib.loads(raw)
    for key in BUILDER_KEYS:
        info.pop(key, None)
    with open(path, 'wb') as f:
        f.write(plistlib.dumps(info, fmt=plistlib.FMT_BINARY if raw.startswith(b'bplist00') else plistlib.FMT_XML, sort_keys=True))


def canonical_car(data):
    car = bytearray(data)
    if car[:8] != b'BOMStore':
        raise ValueError('Assets.car is not a BOM store')
    index, = struct.unpack_from('!I', car, 16)
    variables, = struct.unpack_from('!I', car, 24)

    def block(i):
        return struct.unpack_from('!II', car, index + 4 + 8 * i)

    def leaves(tree):
        at, _ = block(tree)
        if car[at:at + 4] != b'tree':
            raise ValueError('Assets.car: a variable is not a tree')
        node, = struct.unpack_from('!I', car, at + 8)
        while True:
            at, _ = block(node)
            leaf, count, forward, _ = struct.unpack_from('!HHII', car, at)
            pairs = [struct.unpack_from('!II', car, at + 12 + 8 * k) for k in range(count)]
            if not leaf:
                node = pairs[0][0]
                continue
            yield at, pairs
            if not forward:
                return
            node = forward

    named = {}
    p = variables + 4
    for _ in range(struct.unpack_from('!I', car, variables)[0]):
        i, n = struct.unpack_from('!IB', car, p)
        named[bytes(car[p + 5:p + 5 + n])] = i
        p += 5 + n
    if b'APPEARANCEKEYS' in named:
        nodes = list(leaves(named[b'APPEARANCEKEYS']))
        if len(nodes) != 1:
            raise ValueError('Assets.car: the appearances span more than one leaf')
        node, pairs = nodes[0]
        ids = sorted(i for pair in pairs for i in pair)
        if ids and ids != list(range(ids[0], ids[0] + len(ids))):
            raise ValueError('Assets.car: the appearance blocks are not one run')
        bodies = dict((i, bytes(car[a:a + n])) for i in ids for a, n in [block(i)])
        at = min((block(i)[0] for i in ids), default=0)
        end = max((a + n for i in ids for a, n in [block(i)]), default=0)
        for k, (value, key) in enumerate(pairs):
            for i, old in ((ids[2 * k], key), (ids[2 * k + 1], value)):
                body = bodies[old]
                pad = -(at + len(body)) % 16
                car[at:at + len(body) + pad] = body + bytes(pad)
                struct.pack_into('!II', car, index + 4 + 8 * i, at, len(body))
                at += len(body) + pad
            struct.pack_into('!II', car, node + 12 + 8 * k, ids[2 * k + 1], ids[2 * k])
        if ids and at != end + (-end % 16):
            raise ValueError('Assets.car: the appearance blocks left their run')
    if b'KEYFORMAT' in named and b'FACETKEYS' in named:
        at, n = block(named[b'KEYFORMAT'])
        attributes = list(struct.unpack_from('<' + 'I' * struct.unpack_from('<I', car, at + 8)[0], car, at + 12))
        held = {}
        for _, pairs in leaves(named[b'RENDITIONS']):
            for _, key in pairs:
                a, n = block(key)
                v = struct.unpack_from('<' + 'H' * (n // 2), car, a)
                held.setdefault((v[attributes.index(ELEMENT)], v[attributes.index(IDENTIFIER)]), set()).add(v[attributes.index(PART)])
        for _, pairs in leaves(named[b'FACETKEYS']):
            for value, _ in pairs:
                a, _ = block(value)
                count, = struct.unpack_from('<H', car, a + 4)
                facet = dict(struct.unpack_from('<HH', car, a + 6 + 4 * k) for k in range(count))
                parts = held.get((facet.get(ELEMENT), facet.get(IDENTIFIER)), set())
                if 1 < len(parts) and facet.get(PART) in parts:
                    k = [struct.unpack_from('<H', car, a + 6 + 4 * j)[0] for j in range(count)].index(PART)
                    struct.pack_into('<H', car, a + 8 + 4 * k, max(parts))

    def cut(m):
        if car[m.start() - 40:m.start() - 36] != b'ISTC':
            raise ValueError('Assets.car: a temporary icon name outside a rendition header')
        name = m.group(1) + m.group(2)
        return name + bytes(len(m.group(0)) - len(name))
    return ICON_NAME.sub(cut, bytes(car))


def scrub(app):
    for d, _, files in os.walk(app):
        for f in files:
            p = os.path.join(d, f)
            if f == 'Info.plist':
                canonical_plist(p)
            elif f == 'Assets.car':
                with open(p, 'rb') as h:
                    data = canonical_car(h.read())
                with open(p, 'wb') as h:
                    h.write(data)


def macho(path):
    with open(path, 'rb') as f:
        return f.read(4) in (bytes.fromhex('cffaedfe'), bytes.fromhex('cafebabe'))


def manifest(app):
    with tempfile.TemporaryDirectory(prefix='mt-reproduce-') as work:
        copy = os.path.join(work, 'Montana Core.app')
        run(['ditto', '--norsrc', '--noextattr', '--noqtn', '--noacl', app, copy])
        rows = []
        for d, dirs, files in os.walk(copy):
            dirs[:] = sorted(x for x in dirs if x not in LEFT_OUT_DIRS)
            for f in sorted(files):
                p = os.path.join(d, f)
                rel = os.path.relpath(p, copy)
                if f in LEFT_OUT_FILES:
                    continue
                if os.path.islink(p):
                    rows.append(('link:' + os.readlink(p), rel))
                    continue
                if macho(p):
                    run(['codesign', '--remove-signature', p])
                rows.append((hashlib.sha256(open(p, 'rb').read()).hexdigest(), rel))
    rows.sort(key=lambda r: r[1])
    return ''.join(h + '  ' + rel + '\n' for h, rel in rows)


def digest(text):
    return hashlib.sha256(text.encode()).hexdigest()


def released_app(zip_path, work):
    run(['ditto', '-x', '-k', zip_path, work])
    return os.path.join(work, 'Montana Core.app')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--core', help='the core source (core/Code of montana_core); default: the committed core beside this tree')
    parser.add_argument('--home', help='the build folder (default: build-reproduce/ of this tree)')
    parser.add_argument('--compare', help='a published Montana-Core ZIP to compare the rebuilt app with')
    parser.add_argument('--archive', help='archive here with the frameworks the build home names, then scrub (the release road)')
    parser.add_argument('--scrub', help='make every Info.plist and Assets.car of this app canonical')
    parser.add_argument('--digest', help='print the reproducible digest of this app or ZIP')
    parser.add_argument('extra', nargs='*', help='after --: the release road\'s signing switches')
    args = parser.parse_args()
    if args.scrub:
        scrub(args.scrub)
        return
    if args.digest:
        with tempfile.TemporaryDirectory(prefix='mt-release-') as work:
            app = released_app(args.digest, work) if args.digest.endswith('.zip') else args.digest
            print(digest(manifest(app)))
        return
    home = Path(args.home).resolve() if args.home else ROOT / 'build-reproduce'
    home.mkdir(parents=True, exist_ok=True)
    if args.archive:
        frameworks = (home / 'mac-frameworks.path').read_text().strip()
        archive(args.archive, Path(frameworks), args.extra)
        return
    env = dict(os.environ)
    if args.core:
        env['MONTANA_CORE_SRC'] = str(Path(args.core).resolve())
    run([sys.executable, ROOT / 'tools/mt-mac-dependencies.py', '--prepare', '--home', home], env=env)
    frameworks = (home / 'mac-frameworks.path').read_text().strip()
    path = home / 'Montana-Core.xcarchive'
    archive(path, Path(frameworks), ['CODE_SIGNING_ALLOWED=NO'])
    mine = manifest(path / 'Products/Applications/Montana Core.app')
    (home / 'reproduced.manifest').write_text(mine)
    print('rebuilt  ' + digest(mine) + '  (' + str(home / 'reproduced.manifest') + ')')
    if not args.compare:
        return
    with tempfile.TemporaryDirectory(prefix='mt-release-') as work:
        theirs = manifest(released_app(args.compare, work))
    print('release  ' + digest(theirs) + '  ' + os.path.basename(args.compare))
    a = dict((rel, h) for h, rel in (line.split('  ', 1) for line in mine.splitlines()))
    b = dict((rel, h) for h, rel in (line.split('  ', 1) for line in theirs.splitlines()))
    differ = sorted(rel for rel in set(a).union(b) if a.get(rel) != b.get(rel))
    for rel in differ:
        print('differs  ' + rel)
    print('REPRODUCED byte for byte: ' + str(len(a)) + ' files' if not differ else 'NOT REPRODUCED: ' + str(len(differ)) + ' files differ')
    sys.exit(1 if differ else 0)


if __name__ == '__main__':
    main()

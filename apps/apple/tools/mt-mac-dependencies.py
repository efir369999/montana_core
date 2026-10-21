#!/usr/bin/env python3
"""Prepare Catalyst libraries without replacing the phone's frameworks."""
import argparse
import os
from pathlib import Path
import plistlib
import re
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent.parent
TARGET = 'aarch64-apple-ios-macabi'


def run(args, **kwargs):
    return subprocess.run([str(x) for x in args], check=True, **kwargs)


def native_slice(framework):
    info = plistlib.loads((framework / 'Info.plist').read_bytes())
    for lib in info['AvailableLibraries']:
        if lib['SupportedPlatform'] == 'ios' and lib.get('SupportedPlatformVariant') == 'maccatalyst' and 'arm64' in lib['SupportedArchitectures']:
            return lib
    return None


def openssl_macabi(core, home, jobs, env):
    # openssl-src has no Catalyst target by name («don't know how to configure OpenSSL for aarch64-apple-ios-macabi», 2273's Mac
    # road). Its own source, at the version the lock file pins, configured as macOS arm64 with the Catalyst triple and the same
    # switches openssl-src gives an Apple target (no-async as OpenSSL's own iOS configuration), is the library the phone links.
    out = home / 'openssl-macabi'
    if (out / 'lib/libcrypto.a').is_file() and (out / 'lib/libssl.a').is_file():
        return out
    lock = (core / 'Cargo.lock').read_text()
    version = re.search(r'name = "openssl-src"\nversion = "([^"]+)"', lock).group(1)
    registry = Path.home() / ('.' + 'cargo') / 'registry/src'
    sources = [d / ('openssl-src-' + version) / 'openssl' for d in registry.iterdir() if (d / ('openssl-src-' + version)).is_dir()]
    if not sources:
        raise SystemExit('openssl-src ' + version + ' is not in the registry: fetch the core once')
    work = Path(tempfile.mkdtemp(prefix='openssl-macabi-', dir=home)) / 'openssl'
    shutil.copytree(sources[0], work)
    e = env.copy()
    e['SDKROOT'] = subprocess.check_output(['xcrun', '--sdk', 'macosx', '--show-sdk-path'], text=True).strip()
    e['CC'] = 'clang'
    e['CFLAGS'] = '-target arm64-apple-ios' + env['IPHONEOS_DEPLOYMENT_TARGET'] + '-macabi'
    run(['perl', './Configure', 'darwin64-arm64-cc',
         '--prefix=' + str(out), '--openssldir=' + str(out / 'ssl'), '--libdir=lib', 'no-shared', 'no-module', 'no-tests',
         'no-comp', 'no-zlib', 'no-zlib-dynamic', 'no-legacy', 'no-ssl3', 'no-md2', 'no-rc5', 'no-weak-ssl-ciphers',
         'no-camellia', 'no-idea', 'no-seed', 'no-async'], cwd=work, env=e)
    run(['make', '-j' + jobs, 'build_libs'], cwd=work, env=e)
    run(['make', 'install_dev'], cwd=work, env=e)
    shutil.rmtree(work.parent)
    return out


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--prepare', action='store_true')
    args = parser.parse_args()
    common = Path(subprocess.check_output(['git', '-C', str(ROOT), 'rev-parse', '--path-format=absolute', '--git-common-dir'], text=True).strip())
    canonical = common.parent
    core = Path(os.environ.get('MONTANA_CORE_SRC', canonical.parent.parent / 'Montana-Protocol/Code'))
    frameworks = ROOT / 'Code/Frameworks'
    names = ['MontanaBindings', 'MontanaXray', 'HevSocks5Tunnel', 'WebRTC']
    for name in names:
        path = frameworks / (name + '.xcframework')
        if not (path / 'Info.plist').is_file():
            raise SystemExit('Missing framework: ' + str(path))
        if name != 'MontanaBindings' and native_slice(path) is None:
            raise SystemExit('Missing Catalyst slice: ' + name)
    if not (core / 'crates/mt-bindings/Cargo.toml').is_file():
        raise SystemExit('Set MONTANA_CORE_SRC to the workspace that owns mt-bindings')
    rustup = Path.home() / '.cargo/bin/rustup'
    targets = subprocess.check_output([str(rustup), 'target', 'list', '--installed', '--toolchain', '1.92.0'], text=True)
    if TARGET not in targets.splitlines():
        raise SystemExit('Rust 1.92.0 needs the aarch64-apple-ios-macabi target')
    if not args.prepare:
        print('Mac dependencies: source, toolchain and existing tunnel slices verified; mt-bindings needs a Catalyst build')
        return
    # The cargo threads come from the Mac's one gate (heavy.py; the author's word 06.10.2026 15:3x MSK: «only 3 threads at once»).
    jobs = os.environ.get('MT_CORES')
    if not jobs:
        raise SystemExit('Run inside the Mac gate: bash tools/mt-build-mac.sh N')
    # A fresh output preserves earlier artifacts and cannot mutate a framework
    # shared with another task. All generated material stays in the build home.
    home = ROOT / 'build'
    home.mkdir(exist_ok=True)
    out = Path(tempfile.mkdtemp(prefix='mac-frameworks-', dir=home))
    env = os.environ.copy()
    env['CARGO_TARGET_DIR'] = str(home / 'mac-core-target')
    env['IPHONEOS_DEPLOYMENT_TARGET'] = '17.2'
    env['MT_OPENSSL_DIR'] = str(openssl_macabi(core, home, jobs, env))
    # The tool reads its configuration from the directory it is called in, walking up: inside the core it merges the core's
    # RUST_TEST_THREADS (a string) with the root's (a forced table) and refuses to load at all (2272's Mac road, 21.10.2026);
    # from the build home only the root's bound stands, and the thread count is passed here anyway.
    run([rustup, 'run', '1.92.0', 'cargo', 'rustc', '--locked', '-p', 'mt-bindings', '--features', 'network',
         '--release', '--target', TARGET, '--crate-type', 'staticlib', '--manifest-path', core / 'Cargo.toml',
         '--jobs', jobs], env=env, cwd=home)
    lib = home / 'mac-core-target' / TARGET / 'release/libmt_bindings.a'
    headers = out / 'Headers'
    headers.mkdir()
    for name in ['mt_bindings.h', 'montana_ffi.h']:
        shutil.copy2(core / 'crates/mt-bindings/include' / name, headers / name)
    (headers / 'module.modulemap').write_text('module MontanaBindings {\n header "mt_bindings.h"\n header "montana_ffi.h"\n export *\n}\n')
    run(['xcodebuild', '-create-xcframework', '-library', lib, '-headers', headers,
         '-output', out / 'MontanaBindings.xcframework'])
    for name in names[1:]:
        (out / (name + '.xcframework')).symlink_to((frameworks / (name + '.xcframework')).resolve())
    assert native_slice(out / 'MontanaBindings.xcframework') is not None
    (home / 'mac-frameworks.path').write_text(str(out) + '\n')
    print('Mac frameworks ready: ' + str(out))


if __name__ == '__main__':
    main()

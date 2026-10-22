#!/usr/bin/env python3
"""Prepare the Mac Catalyst frameworks of Montana Core from public source, reproducibly."""
# REPRODUCIBLE BY ANY BUILDER (Constitution [I-14]: «every release build ... is reproducible byte for byte from publicly published
# source by any independent builder»; the author's words 21.10.2026 20:4x and 20:58 MSK: «there is no reproducible build of the
# applications -- fix it», «while you wait for OZ 1, close OZ 2»). Measured on the release 2274 (21.10.2026): its main binary held
# 1788 absolute paths of this Mac -- 1561 of the Go module cache and the tree xray was built in, 174 of the cargo registry and the
# core, 7 of OPENSSLDIR in a session tree -- and its core was built from a working folder with uncommitted changes. So:
#   * the core is the tree of one committed commit (git archive), never a working folder, and the commit is stamped in SOURCE;
#   * every path a compiler could write is rewritten to a fixed name under /montana (no file lives there on any machine);
#   * OpenSSL is configured with a fixed prefix and directory, installed under DESTDIR, its build date pinned to the epoch;
#   * the Xray engine is built here, -trimpath, by the Go toolchain and gomobile its go.mod names; WebRTC is the published binary
#     whose SHA-256 fetch-webrtc.sh pins. Nothing is taken from a framework folder some other road left behind.
import argparse
import io
import os
from pathlib import Path
import plistlib
import re
import shutil
import subprocess
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parent.parent
TARGET = 'aarch64-apple-ios-macabi'
FIXED = '/montana'
RUST = '1.92.0'
RUSTUP = shutil.which('rustup') or str(Path.home() / '.cargo/bin/rustup')
DEPLOYMENT = '17.2'


def run(args, **kwargs):
    return subprocess.run([str(x) for x in args], check=True, **kwargs)


def native_slice(framework):
    info = plistlib.loads((framework / 'Info.plist').read_bytes())
    for lib in info['AvailableLibraries']:
        if lib['SupportedPlatform'] == 'ios' and lib.get('SupportedPlatformVariant') == 'maccatalyst' and 'arm64' in lib['SupportedArchitectures']:
            return lib
    return None


def core_source(home):
    # A core named by the caller (a clone of montana_core: its core/Code) is used as it stands; otherwise the committed HEAD of the
    # core's repository beside this one is archived, so an uncommitted change can never reach a release.
    named = os.environ.get('MONTANA_CORE_SRC')
    if named:
        return Path(named).resolve(), {'repo': 'given', 'path': str(Path(named).resolve().name)}
    common = Path(subprocess.check_output(['git', '-C', str(ROOT), 'rev-parse', '--path-format=absolute', '--git-common-dir'], text=True).strip())
    repo = common.parent.parent.parent / 'Montana-Protocol'
    commit = subprocess.check_output(['git', '-C', str(repo), 'rev-parse', 'HEAD'], text=True).strip()
    out = home / ('core-' + commit[:12])
    # The core reads its word list from beside Code/ (mt-mnemonic: include_str! of «../../../../Montana wordlist.txt»), and the public
    # core folder carries it there; measured 21.10.2026 21:37 MSK: an archive of Code/ alone could not compile mt-mnemonic.
    words = 'Montana wordlist.txt'
    if not (out / 'Code/Cargo.toml').is_file() or not (out / words).is_file():
        blob = subprocess.run(['git', '-C', str(repo), 'archive', '--format=tar', commit, 'Code', words], capture_output=True, check=True).stdout
        with tarfile.open(fileobj=io.BytesIO(blob)) as tar:
            tar.extractall(out, filter='tar')
    tree = subprocess.check_output(['git', '-C', str(repo), 'rev-parse', commit + ':Code'], text=True).strip()
    return out / 'Code', {'repo': 'Montana-Protocol', 'commit': commit, 'tree': tree, 'clean': 'yes'}


def openssl_macabi(core, home, jobs, env):
    # openssl-src has no Catalyst target by name («don't know how to configure OpenSSL for aarch64-apple-ios-macabi», 2273's Mac
    # road). Its own source, at the version the lock file pins, configured as macOS arm64 with the Catalyst triple and the same
    # switches openssl-src gives an Apple target (no-async as OpenSSL's own iOS configuration), is the library the phone links.
    # The prefix and the directory are compiled into the library as strings: both are the fixed name, the files go under DESTDIR.
    lock = (core / 'Cargo.lock').read_text()
    version = re.search(r'name = "openssl-src"\nversion = "([^"]+)"', lock).group(1)
    root = home / ('openssl-macabi-' + version)
    out = root / FIXED.lstrip('/') / 'openssl'
    if (out / 'lib/libcrypto.a').is_file() and (out / 'lib/libssl.a').is_file():
        return out
    registry = Path(env.get('CARGO_HOME', Path.home() / '.cargo')) / 'registry/src'
    sources = [d / ('openssl-src-' + version) / 'openssl' for d in registry.iterdir() if (d / ('openssl-src-' + version)).is_dir()]
    if not sources:
        raise SystemExit('openssl-src ' + version + ' is not in the registry: run cargo fetch in the core once')
    work = Path(tempfile.mkdtemp(prefix='openssl-macabi-', dir=home)) / 'openssl'
    shutil.copytree(sources[0], work)
    e = env.copy()
    e['SDKROOT'] = subprocess.check_output(['xcrun', '--sdk', 'macosx', '--show-sdk-path'], text=True).strip()
    e['CC'] = 'clang'
    e['CFLAGS'] = '-target arm64-apple-ios' + DEPLOYMENT + '-macabi'
    e['SOURCE_DATE_EPOCH'] = '0'
    run(['perl', './Configure', 'darwin64-arm64-cc',
         '--prefix=' + FIXED + '/openssl', '--openssldir=' + FIXED + '/openssl/ssl', '--libdir=lib', 'no-shared', 'no-module', 'no-tests',
         'no-comp', 'no-zlib', 'no-zlib-dynamic', 'no-legacy', 'no-ssl3', 'no-md2', 'no-rc5', 'no-weak-ssl-ciphers',
         'no-camellia', 'no-idea', 'no-seed', 'no-async'], cwd=work, env=e)
    run(['make', '-j' + jobs, 'build_libs'], cwd=work, env=e)
    run(['make', 'install_dev', 'DESTDIR=' + str(root)], cwd=work, env=e)
    shutil.rmtree(work.parent)
    return out


def bindings(core, home, out, jobs):
    env = os.environ.copy()
    target = home / 'mac-core-target'
    env['CARGO_TARGET_DIR'] = str(target)
    env['IPHONEOS_DEPLOYMENT_TARGET'] = DEPLOYMENT
    env['MT_OPENSSL_DIR'] = str(openssl_macabi(core, home, jobs, env))
    cargo_home = Path(env.get('CARGO_HOME', Path.home() / '.cargo')).resolve()
    # rustc keeps the last rule that matches: the general home first, the precise folders after it
    remap = [(Path.home(), FIXED + '/home'), (cargo_home, FIXED + '/cargo'), (core, FIXED + '/core'), (target, FIXED + '/target')]
    env['CARGO_ENCODED_RUSTFLAGS'] = chr(31).join('--remap-path-prefix=' + str(a) + '=' + b for a, b in remap)
    env['CFLAGS_aarch64_apple_ios_macabi'] = ' '.join('-ffile-prefix-map=' + str(a) + '=' + b for a, b in remap)
    # The tool reads its configuration from the directory it is called in, walking up: inside the core it merges the core's
    # RUST_TEST_THREADS (a string) with the root's (a forced table) and refuses to load at all (2272's Mac road, 21.10.2026);
    # from the build home only the root's bound stands, and the thread count is passed here anyway.
    run([RUSTUP, 'run', RUST, 'cargo', 'rustc', '--locked', '-p', 'mt-bindings', '--features', 'network',
         '--release', '--target', TARGET, '--crate-type', 'staticlib', '--manifest-path', core / 'Cargo.toml',
         '--jobs', jobs], env=env, cwd=home)
    lib = target / TARGET / 'release/libmt_bindings.a'
    headers = home / 'mac-bindings-headers'
    shutil.rmtree(headers, ignore_errors=True)
    headers.mkdir()
    for name in ['mt_bindings.h', 'montana_ffi.h']:
        shutil.copyfile(core / 'crates/mt-bindings/include' / name, headers / name)
    (headers / 'module.modulemap').write_text('module MontanaBindings {\n header "mt_bindings.h"\n header "montana_ffi.h"\n export *\n}\n')
    run(['xcodebuild', '-create-xcframework', '-library', lib, '-headers', headers, '-output', out / 'MontanaBindings.xcframework'])


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--prepare', action='store_true')
    parser.add_argument('--home', help='the build home (default: build/ of this tree)')
    args = parser.parse_args()
    targets = subprocess.check_output([RUSTUP, 'target', 'list', '--installed', '--toolchain', RUST], text=True)
    if TARGET not in targets.splitlines():
        raise SystemExit('Rust ' + RUST + ' needs the ' + TARGET + ' target: rustup target add --toolchain ' + RUST + ' ' + TARGET)
    if not args.prepare:
        print('Mac dependencies: Rust ' + RUST + ' with the Catalyst target found; --prepare builds the frameworks')
        return
    jobs = os.environ.get('MT_CORES') or str(os.cpu_count())
    home = Path(args.home).resolve() if args.home else ROOT / 'build'
    home.mkdir(parents=True, exist_ok=True)
    # A fresh output preserves earlier artifacts and cannot mutate a framework shared with another task.
    out = Path(tempfile.mkdtemp(prefix='mac-frameworks-', dir=home))
    core, stamp = core_source(home)
    if not (core / 'crates/mt-bindings/Cargo.toml').is_file():
        raise SystemExit('No mt-bindings in the core at ' + str(core))
    bindings(core, home, out, jobs)
    run(['bash', ROOT / 'scripts/xray-ios/build-xray-xcframework.sh', 'maccatalyst/arm64', out / 'MontanaXray.xcframework'])
    webrtc = home / 'webrtc-144.7559.10/WebRTC.xcframework'
    run(['bash', ROOT / 'fetch-webrtc.sh', webrtc])
    (out / 'WebRTC.xcframework').symlink_to(webrtc)
    for name in ['MontanaBindings', 'MontanaXray', 'WebRTC']:
        if native_slice(out / (name + '.xcframework')) is None:
            raise SystemExit('Missing Catalyst slice: ' + name)
    (out / 'MontanaBindings.xcframework/SOURCE').write_text(''.join(k + '=' + v + '\n' for k, v in stamp.items()))
    (home / 'mac-frameworks.path').write_text(str(out) + '\n')
    print('Mac frameworks ready: ' + str(out) + ' (core ' + (stamp.get('commit') or stamp['path'])[:12] + ')')


if __name__ == '__main__':
    main()

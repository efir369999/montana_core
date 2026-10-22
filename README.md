# Montana Core

Montana Core is the native desktop application of Montana for Mac computers with Apple silicon. It is built from the same
Apple sources as Montana for iPhone and iPad: one source tree and one version. The desktop's version is the build number of
the iOS application, 1.0 (N), and a Mac build N and an iPhone build N are the same commit.

This repository is the reference copy of those sources and of the protocol core they link. Its folders are staged by the
build road at every build of the main line and are not edited by hand; [`RELEASES.md`](RELEASES.md) names, for every folder,
the build, the source commit and the git tree of the folder.

The first release is planned for 31 October 2026, for review on the Metzdowd cryptography list.

## Status

Protocol closure is open. The 13 acceptance criteria of the protocol and their current status, with source evidence, are
kept in the audit of the Montana repository: [montana_messenger/Audit](https://github.com/efir369999/montana_messenger/tree/main/Audit).
Nothing in this repository claims more than those reports state.

<!-- download -->
## Download

Montana Core 1.0 (2282) for Mac, signed with a Developer ID and notarized by Apple.

|   |   |
| --- | --- |
| Release | [`v1.0-2282`](https://github.com/efir369999/montana_core/releases/tag/v1.0-2282) |
| File | [`Montana-Core-1.0-2282.zip`](https://github.com/efir369999/montana_core/releases/download/v1.0-2282/Montana-Core-1.0-2282.zip), the same bytes as https://montana.quest/download/mac/Montana-Core.zip |
| SHA-256 | `29338b1b08aa6282165f3c867e2ca3c070100afaba79234602f3098fbbc05b6c` |
| Source | `apps/apple` at commit `e200b3169fd7` (folder tree `f14779092eea0efbc9cd6719c45f7ce163045a99`), staged from commit `51a2d7c9b023` of the application's repository |
| Requires | A Mac with Apple silicon, macOS 14.2 or later |

The Mac build carries no VPN: a packet tunnel distributed outside the Mac App Store must be a system extension, and a Mac
Catalyst application cannot activate one.

### Verify

```sh
shasum -a 256 Montana-Core-1.0-2282.zip
ditto -x -k Montana-Core-1.0-2282.zip .
codesign --verify --deep --strict --verbose=2 "Montana Core.app"
xcrun stapler validate "Montana Core.app"
spctl --assess --type execute --verbose=2 "Montana Core.app"
```

The checksum must equal the one above, and Gatekeeper must answer `source=Notarized Developer ID`.

### Reproduce

Anyone can rebuild this app from this repository and compare it with the release, file for file, signatures left out:

```sh
git clone --branch v1.0-2282 https://github.com/efir369999/montana_core.git
cd montana_core/apps/apple
curl -LO https://github.com/efir369999/montana_core/releases/download/v1.0-2282/Montana-Core-1.0-2282.zip
python3 tools/mt-reproduce-mac.py --core ../../core/Code --compare Montana-Core-1.0-2282.zip
```

The two digests it prints must be equal, and it must answer `REPRODUCED byte for byte`. It needs Xcode 26.2 (17C52), Rust 1.92.0 with the `aarch64-apple-ios-macabi` target, and the Go that `go.mod` names.
<!-- /download -->

## What this repository holds

| Path | What it is |
|---|---|
| [`apps/apple/`](apps/apple/) | Montana for Mac, iPhone and iPad (Swift). The Mac application is the Mac Catalyst arm64 product of the same Xcode project and target |
| [`core/`](core/) | The protocol core in Rust: the specification (`core/docs`), the reference implementation (`core/Montana-Core`) and the client core the applications link (`core/Code`) |
| [`RELEASES.md`](RELEASES.md) | The source commit, build and git tree of every folder, written by the publisher in the commit that changes the folder |

A reader pins the exact bytes under review with `git rev-parse HEAD:apps/apple` and `git rev-parse HEAD:core`, and compares
them with the Folder tree column of [`RELEASES.md`](RELEASES.md).

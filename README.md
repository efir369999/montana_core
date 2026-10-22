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

Montana Core 1.0 (2281) for Mac, signed with a Developer ID and notarized by Apple.

|   |   |
| --- | --- |
| Release | [`v1.0-2281`](https://github.com/efir369999/montana_core/releases/tag/v1.0-2281) |
| File | [`Montana-Core-1.0-2281.zip`](https://github.com/efir369999/montana_core/releases/download/v1.0-2281/Montana-Core-1.0-2281.zip), the same bytes as https://montana.quest/download/mac/Montana-Core.zip |
| SHA-256 | `1bb589d011f5b20ae9edc819e61e08c837dc0ac404c7714ee832d733aa6847cb` |
| Source | `apps/apple` at commit `6c09e325938d` (folder tree `497103957be8428a23abbb07044baa3f3077c0e4`), staged from commit `13041e634423` of the application's repository |
| Requires | A Mac with Apple silicon, macOS 14.2 or later |

The Mac build carries no VPN: a packet tunnel distributed outside the Mac App Store must be a system extension, and a Mac
Catalyst application cannot activate one.

### Verify

```sh
shasum -a 256 Montana-Core-1.0-2281.zip
ditto -x -k Montana-Core-1.0-2281.zip .
codesign --verify --deep --strict --verbose=2 "Montana Core.app"
xcrun stapler validate "Montana Core.app"
spctl --assess --type execute --verbose=2 "Montana Core.app"
```

The checksum must equal the one above, and Gatekeeper must answer `source=Notarized Developer ID`.
<!-- /download -->

## What this repository holds

| Path | What it is |
|---|---|
| [`apps/apple/`](apps/apple/) | Montana for Mac, iPhone and iPad (Swift). The Mac application is the Mac Catalyst arm64 product of the same Xcode project and target |
| [`core/`](core/) | The protocol core in Rust: the specification (`core/docs`), the reference implementation (`core/Montana-Core`) and the client core the applications link (`core/Code`) |
| [`RELEASES.md`](RELEASES.md) | The source commit, build and git tree of every folder, written by the publisher in the commit that changes the folder |

A reader pins the exact bytes under review with `git rev-parse HEAD:apps/apple` and `git rev-parse HEAD:core`, and compares
them with the Folder tree column of [`RELEASES.md`](RELEASES.md).

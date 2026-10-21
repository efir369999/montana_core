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

No signed Mac build has been published yet. Distribution outside the App Store requires a Developer ID signature and Apple
notarization; until both exist, this repository holds sources only.

## What this repository holds

| Path | What it is |
|---|---|
| [`apps/apple/`](apps/apple/) | Montana for Mac, iPhone and iPad (Swift). The Mac application is the Mac Catalyst arm64 product of the same Xcode project and target |
| [`core/`](core/) | The protocol core in Rust: the specification (`core/docs`), the reference implementation (`core/Montana-Core`) and the client core the applications link (`core/Code`) |
| [`RELEASES.md`](RELEASES.md) | The source commit, build and git tree of every folder, written by the publisher in the commit that changes the folder |

A reader pins the exact bytes under review with `git rev-parse HEAD:apps/apple` and `git rev-parse HEAD:core`, and compares
them with the Folder tree column of [`RELEASES.md`](RELEASES.md).

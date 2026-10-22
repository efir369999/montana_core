# Releases

This file names what each folder of this repository is. The table Sources is written by the publisher in the same commit as
the folder it names.

## Sources

Each folder of this repository is the tree of one commit of the project's private history, staged as it is built. The
publisher writes this table in the same commit as the folder it names, and the message of that commit names the source
commit in full. The column Folder tree is the git tree of the folder in the commit that carries the table:
`git rev-parse HEAD:FOLDER` answers it.

<!-- sources:start -->
| Folder | Build | Source commit | Folder tree | Staged |
|---|---|---|---|---|
| `apps/apple` | 1.0 (2290) | `026de4f2ad51` | `c48d676c886ee20c6a558776f6e67a9ebabbf14e` | 2026-10-22 21:01 UTC |
| `core` | 1.0 (2290) | `32a9a4749847` | `3372ae236765cbe272cbcad9acf5d9e32951af33` | 2026-10-22 21:01 UTC |
<!-- sources:end -->

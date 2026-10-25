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
| `apps/apple` | 1.0 (2313) | `7c3624af1f00` | `ef4317efc6590f5c5c676ce376725f1b923ce183` | 2026-10-25 21:16 UTC |
| `core` | 1.0 (2313) | `ec2d343cebe1` | `d411702c5a09d4c275abac4e58e35ede1080fadb` | 2026-10-25 21:16 UTC |
<!-- sources:end -->

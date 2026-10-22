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
| `apps/apple` | 1.0 (2280) | `08d3cf128fe5` | `78b6e25c06b207f3c4fc51280daa3a1eedd69db1` | 2026-10-22 12:08 UTC |
| `core` | core line | `3848245e308b` | `4ce3d6705224c8c99db0db1710a70bbde3d04361` | 2026-10-21 15:20 UTC |
<!-- sources:end -->

#!/bin/bash
# The upstream Makefiles call "$(AR) csq ARCHIVE OBJECTS": ar writes this Mac's time, owner and group into every member, and
# libtool -D rewrites only the symbol table's header, never the members it copies from an archive (22.10.2026: two builds of one
# commit differed in nothing but those headers). Born here with libtool -D, every member header is zero from the start.
set -euo pipefail
[ "$1" = csq ] || { echo "REFUSED: archive.sh knows only csq, got $1"; exit 3; }
exec xcrun libtool -static -D -o "$2" "${@:3}"

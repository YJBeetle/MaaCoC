#!/usr/bin/env bash
# Put the MaaFramework dylibs next to the built binary, matching how the
# shipped app lays them out (the binary carries an @executable_path rpath).
set -euo pipefail
cd "$(dirname "$0")/.."

PROFILE="${1:-debug}"
DEST="target/$PROFILE"
if [ ! -d "$DEST" ]; then
    echo "先 cargo build --$PROFILE" >&2
    exit 1
fi

cp -f vendor/bin/*.dylib "$DEST"/ 2>/dev/null || true
echo "已同步 $(ls "$DEST"/*.dylib | wc -l | tr -d ' ') 个 dylib 到 $DEST"

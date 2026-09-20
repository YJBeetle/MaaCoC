#!/usr/bin/env bash
# Put the MaaFramework libraries next to the built binary, matching how the
# shipped app lays them out (the binary carries an rpath pointing at its own
# directory, and on Windows the loader only looks there).
set -euo pipefail
cd "$(dirname "$0")/.."

PROFILE="${1:-debug}"
DEST="target/$PROFILE"
if [ ! -d "$DEST" ]; then
    echo "先 cargo build --$PROFILE" >&2
    exit 1
fi

copied=0
for file in vendor/bin/*; do
    case "$file" in
        *.dylib|*.so|*.so.*|*.dll) cp -f "$file" "$DEST"/ && copied=$((copied + 1)) ;;
        *) ;;
    esac
done
if [ "$copied" = 0 ]; then
    echo "vendor/bin 里没有动态库，先跑 scripts/fetch-sdk.sh" >&2
    exit 1
fi
echo "已同步 $copied 个动态库到 $DEST"

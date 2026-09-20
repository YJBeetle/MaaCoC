#!/usr/bin/env bash
# Collect the runtime libraries the bundle has to carry. Tauri copies everything
# under app/src-tauri/native into the resource directory, which is next to the
# executable on Windows and inside the .app on macOS.
set -euo pipefail
cd "$(dirname "$0")/.."

DEST=app/src-tauri/native
rm -rf "$DEST"
mkdir -p "$DEST"

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
echo "已收集 $copied 个运行库到 $DEST"

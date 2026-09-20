#!/usr/bin/env bash
# Prove the bundle actually carries what the app needs at runtime: the pipeline
# assets and the MaaFramework libraries. A green build that ships neither is
# exactly the failure mode this project keeps hitting.
set -euo pipefail
cd "$(dirname "$0")/.."

BUNDLE=app/src-tauri/target/release/bundle
fail() { echo "打包检查失败: $*" >&2; exit 1; }

ls app/src-tauri/native/* >/dev/null 2>&1 || fail "app/src-tauri/native 是空的，运行库没有进包"

case "$(uname -s)" in
  Darwin)
    APP="$BUNDLE/macos/MaaCoC.app"
    [ -d "$APP" ] || fail "找不到 $APP"
    BIN="$APP/Contents/MacOS/MaaCoC"
    [ -x "$BIN" ] || fail "可执行文件缺失: $BIN"
    [ -f "$APP/Contents/Resources/assets/pipeline/main.json" ] || fail "包里缺 assets/pipeline/main.json"
    [ -f "$APP/Contents/Resources/libMaaFramework.dylib" ] || fail "包里缺 libMaaFramework.dylib"
    otool -L "$BIN" | grep -q MaaFramework || fail "可执行文件没有链接 MaaFramework"
    otool -l "$BIN" | grep -q "path @executable_path/../Resources" || fail "缺少指向 Resources 的 rpath"
    echo "macOS 包检查通过: $(du -sh "$APP" | cut -f1)"
    ;;
  Linux)
    deb=$(ls "$BUNDLE"/deb/*.deb 2>/dev/null | head -1)
    [ -n "${deb:-}" ] || fail "没有生成 .deb"
    dpkg -c "$deb" | grep -q "assets/pipeline/main.json" || fail ".deb 里缺 assets/pipeline"
    dpkg -c "$deb" | grep -q "libMaaFramework.so" || fail ".deb 里缺 libMaaFramework.so"
    echo "Linux 包检查通过: $deb"
    ;;
  MINGW*|MSYS*|CYGWIN*)
    exe=$(ls "$BUNDLE"/nsis/*.exe 2>/dev/null | head -1)
    [ -n "${exe:-}" ] || fail "没有生成 NSIS 安装包"
    echo "Windows 包已生成: $exe ($(du -sh "$exe" | cut -f1))"
    ;;
  *)
    fail "不支持的平台 $(uname -s)"
    ;;
esac

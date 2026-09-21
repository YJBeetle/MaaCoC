#!/usr/bin/env bash
# Download the MaaFramework SDK for this platform into vendor/.
# The SDK is never committed; CI and local builds use the same script.
set -euo pipefail

MAA_VERSION="${MAA_VERSION:-v5.14.0-beta.1}"
cd "$(dirname "$0")/.."

case "$(uname -s)" in
    Darwin) os=macos ;;
    Linux)  os=linux ;;
    MINGW*|MSYS*|CYGWIN*) os=win ;;
    *) echo "不支持的平台: $(uname -s)" >&2; exit 1 ;;
esac

arch="$(uname -m)"
case "$arch" in
    arm64|aarch64) cpu=aarch64 ;;
    x86_64|amd64)  cpu=x86_64 ;;
    *) echo "不支持的架构: $arch" >&2; exit 1 ;;
esac

# Windows SDK zips only ship x86_64 for now.
if [ "$os" = win ] && [ "$cpu" = aarch64 ]; then cpu=x86_64; fi

name="MAA-${os}-${cpu}-${MAA_VERSION}"
if [ -d "vendor/$name" ] || [ -f vendor/.sdk-version ] && [ "$(cat vendor/.sdk-version)" = "$name" ]; then
    if [ -d vendor/bin ]; then echo "SDK 已就绪: $name"; exit 0; fi
fi

url="https://github.com/MaaXYZ/MaaFramework/releases/download/${MAA_VERSION}/${name}.zip"
echo "下载 $url"
rm -rf vendor && mkdir -p vendor
curl -sSLf --retry 3 -o vendor/sdk.zip "$url"
unzip -q vendor/sdk.zip -d vendor
rm vendor/sdk.zip
# Some archives wrap everything in a single directory named after the build,
# others (the Windows one) already have bin/ at the top level. Only flatten the
# wrapper when it is actually there — guessing with "the first directory"
# dismembers the tree.
if [ -d "vendor/$name/bin" ]; then
    mv "vendor/$name"/* vendor/
    rm -rf "vendor/$name"
fi
rm -rf vendor/__MACOSX
find vendor -name .DS_Store -delete
echo "$name" > vendor/.sdk-version
test -d vendor/bin || { echo "SDK 解压后缺少 bin/，实际内容：$(ls vendor)" >&2; exit 1; }
echo "SDK 就绪: ${name}（$(ls vendor/bin | wc -l | tr -d ' ') 个文件）"

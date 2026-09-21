#!/usr/bin/env bash
# Render the UI headlessly with Chrome so layout can be inspected without a
# window on screen. Dev-only URL params pick the page and the colour scheme.
set -euo pipefail

OUT=${1:-var/shots}
PAGES=${2:-"run stats settings"}
RUN=${3:-0}
mkdir -p "$OUT"

for page in $PAGES; do
  url="http://localhost:5173/?page=$page&theme=dark"
  [ "$RUN" = "1" ] && url="$url&run=1"
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
    --headless=new \
    --disable-gpu \
    --hide-scrollbars \
    --force-device-scale-factor=1 \
    --window-size=1180,847 \
    --virtual-time-budget=6000 \
    --screenshot="$OUT/$page.png" \
    "$url" 2>/dev/null
  echo "wrote $OUT/$page.png"
done

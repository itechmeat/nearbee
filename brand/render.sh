#!/usr/bin/env bash
# Render the Nearbee bee from brand/bee/bee.html with headless Chromium.
# Usage: brand/render.sh [size]   every preset view on both grounds, plus the logo mark
#        brand/render.sh shot <name> <w> <h> <query>
#            one poster shot into renders/<name>.png, for example:
#            brand/render.sh shot poster-a 2400 1350 'az=130&el=35&dist=8&bg=none'
#            query keys: view, az, el, dist, fov, tx, ty, tz, bg=light|dark|none, shadow=0
# CHROME names the Chromium or Chrome binary (default: chromium, then google-chrome).
# The page loads three.js from cdn.jsdelivr.net, so a render needs network access.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
chrome=${CHROME:-$(command -v chromium || command -v chromium-browser || command -v google-chrome || true)}
[ -n "$chrome" ] || { echo "render.sh: set CHROME to a Chromium binary" >&2; exit 2; }
out="$here/renders"
mkdir -p "$out" "$here/logo"

shoot() { # file width height query
  "$chrome" --headless=new --use-angle=swiftshader --enable-unsafe-swiftshader \
    --hide-scrollbars --window-size="$2,$3" --virtual-time-budget=30000 \
    --default-background-color=00000000 --screenshot="$1" \
    "file://$here/bee/bee.html?w=$2&h=$3&$4" >/dev/null 2>&1
  echo "$1"
}

if [ "${1:-}" = shot ]; then
  [ $# -eq 5 ] || { echo "render.sh: shot <name> <w> <h> <query>" >&2; exit 2; }
  shoot "$out/$2.png" "$3" "$4" "$5"
  exit 0
fi

size=${1:-1600}
for view in hero top side head front rear low; do
  for bg in light dark; do
    shoot "$out/bee-$view-$bg.png" "$size" "$size" "view=$view&bg=$bg"
  done
done
shoot "$here/logo/nearbee-mark.png" 1024 1024 "view=logo"

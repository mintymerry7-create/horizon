#!/bin/bash
# Zips the game files the Horizon launcher prepared on this Mac or Linux PC, for the iOS app (docs/ios.md).
#   bash ios/export-resources.sh [zip]
set -euo pipefail
out=${1:-$HOME/Desktop/horizon-resources.zip}
out="$(cd "$(dirname "$out")" && pwd)/$(basename "$out")"
if [ -n "${HORIZON_LAUNCHER_HOME:-}" ]; then root=$HORIZON_LAUNCHER_HOME
elif [ "$(uname)" = Darwin ]; then root="$HOME/Library/Application Support/horizon-launcher"
else root="${XDG_DATA_HOME:-$HOME/.local/share}/horizon-launcher"; fi
manifest=$(find "$root/versions" -path '*/.local/horizon-client/*/horizon-resources.json' -print0 2>/dev/null \
  | xargs -0 ls -t 2>/dev/null | head -1 || true)
[ -n "$manifest" ] || { echo "No prepared game files under $root/versions: press Play once in the launcher first." >&2; exit 1; }
dir=$(dirname "$manifest")
echo "Zipping $dir ..."
rm -f "$out"
(cd "$(dirname "$dir")" && zip -qr "$out" "$(basename "$dir")")
echo "Done: $out ($(du -h "$out" | cut -f1))"

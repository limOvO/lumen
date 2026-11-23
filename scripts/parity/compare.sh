#!/usr/bin/env bash
# Build side-by-side sheets for every state both builds captured.
set -u
PROJ="$(cd "$(dirname "$0")/../.." && pwd)"
BASE="$PROJ/.cache/parity"
[ -d "$BASE/web" ] || { echo "run capture.sh web first"; exit 1; }
[ -d "$BASE/egui" ] || { echo "run capture.sh egui first"; exit 1; }
OUT="$PROJ/.cache/parity/pairs"; mkdir -p "$OUT"
status=0
for f in "$BASE/web"/*.png; do
  name=$(basename "$f")
  other="$BASE/egui/$name"
  if [ ! -f "$other" ]; then echo "MISSING egui: $name"; status=1; continue; fi
  magick "$f" -resize 45% /tmp/a.png
  magick "$other" -resize 45% /tmp/b.png
  montage /tmp/a.png /tmp/b.png -tile 2x1 -geometry +4+4 -background '#000' "$OUT/$name" 2>/dev/null
done
echo "sheets in $OUT"
exit $status

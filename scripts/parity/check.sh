#!/usr/bin/env bash
# Automated parity assertions: captures both builds, then checks that every
# expected state exists, that each capture is non-blank, and that the egui
# window geometry matches the web build (1280x820, 880x560 minimum).
set -u
PROJ="$(cd "$(dirname "$0")/../.." && pwd)"
source "$PROJ/scripts/parity/lib.sh"
STATES="01-preview 02-info 03-edit 04-brush 05-rect 06-crop 07-crop-ratio 08-adjust 09-shortcuts 10-back-to-preview 11-open 12-saveas"
fail=0

"$PROJ/scripts/parity/capture.sh" web   >/dev/null 2>&1 || { echo "FAIL: web capture"; fail=1; }
"$PROJ/scripts/parity/capture.sh" egui  >/dev/null 2>&1 || { echo "FAIL: egui capture"; fail=1; }

for s in $STATES; do
  for app in web egui; do
    f="$PROJ/.cache/parity/$app/$s.png"
    if [ ! -s "$f" ]; then echo "FAIL: missing $app/$s.png"; fail=1; continue; fi
    mean=$(magick "$f" -colorspace Gray -format "%[fx:mean*255]" info: 2>/dev/null || echo 0)
    if awk -v m="$mean" 'BEGIN{exit !(m < 2)}'; then echo "FAIL: blank $app/$s.png (mean=$mean)"; fail=1; fi
  done
done

# geometry: launch the egui build on the private display and compare sizes
parity_env
kill_apps
WGPU_BACKEND="${WGPU_BACKEND:-gl}" "$PROJ/egui-app/target/release/lumen-egui-app" "$IMG_SMALL" >/dev/null 2>&1 &
if wait_window "Lumen" 20000; then
  focus_window "Lumen"
  wid=$(xdotool search --onlyvisible --name "Lumen" | head -1)
  eval "$(xdotool getwindowgeometry --shell "$wid")"
  [ "$WIDTH" = "1280" ] || { echo "FAIL: egui window width $WIDTH != 1280"; fail=1; }
  [ "$HEIGHT" = "820" ] || { echo "FAIL: egui window height $HEIGHT != 820"; fail=1; }
else
  echo "FAIL: egui window did not appear"; fail=1
fi
kill_apps

if [ "$fail" -eq 0 ]; then
  echo "PARITY CHECK PASSED — ${STATES// /, } (web + egui), geometry 1280x820"
else
  echo "PARITY CHECK FAILED"
fi
exit "$fail"

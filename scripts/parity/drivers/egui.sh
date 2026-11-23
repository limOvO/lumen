#!/usr/bin/env bash
# Drives the egui build through the same parity states.
# Coordinates assume the 1280x820 window at 80,40 (the Lead updates them after
# UI integration; the captures are paired by filename, not by pixel equality).
set -u
PROJ="$(cd "$(dirname "$0")/../../.." && pwd)"
source "$PROJ/scripts/parity/lib.sh"
OUT="${1:?outdir}"
"$PROJ/egui-app/target/release/lumen-egui-app" "$IMG_SMALL" >/dev/null 2>&1 &
wait_window "Lumen" 20000
focus_window "Lumen"
sleep 1.5
shot "$OUT" 01-preview
key i;  shot "$OUT" 02-info;  key i
key e; sleep 0.6; shot "$OUT" 03-edit
key b; drag 340 340 620 560
sleep 0.4; shot "$OUT" 04-brush
key r; drag 760 300 940 460
sleep 0.4; shot "$OUT" 05-rect
key c; sleep 0.6; shot "$OUT" 06-crop
key question; sleep 0.6; shot "$OUT" 09-shortcuts; key Escape
key v; key e; sleep 0.6; shot "$OUT" 10-back-to-preview

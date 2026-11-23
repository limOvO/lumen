#!/usr/bin/env bash
# Drives the reference Tauri build through the parity states.
set -u
PROJ="$(cd "$(dirname "$0")/../../.." && pwd)"
source "$PROJ/scripts/parity/lib.sh"
OUT="${1:?outdir}"
"$PROJ/dist/lumen" "$IMG_SMALL" >/dev/null 2>&1 &
wait_window "Lumen" 9000
focus_window "Lumen"
sleep 1.2
shot "$OUT" 01-preview
key i;  shot "$OUT" 02-info;  key i
click 1330 72 2.4
shot "$OUT" 03-edit
key b; drag 340 340 620 560
sleep 0.4; shot "$OUT" 04-brush
key r; drag 760 300 940 460
sleep 0.4; shot "$OUT" 05-rect
key c; sleep 0.6; shot "$OUT" 06-crop
click 1298 172 0.6; shot "$OUT" 07-crop-ratio
click 1206 102 0.7; shot "$OUT" 08-adjust
key question; sleep 0.6; shot "$OUT" 09-shortcuts; key Escape
key ctrl+o; sleep 0.8; shot "$OUT" 11-open; key Escape
key ctrl+shift+s; sleep 0.9; shot "$OUT" 12-saveas; key Escape
key v; key Escape; sleep 0.6; shot "$OUT" 10-back-to-preview

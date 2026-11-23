#!/usr/bin/env bash
# Capture parity screenshots for one build: ./capture.sh web|egui [outdir]
set -u
PROJ="$(cd "$(dirname "$0")/../.." && pwd)"
source "$PROJ/scripts/parity/lib.sh"
APP="${1:?usage: capture.sh web|egui [outdir]}"
OUT="${2:-$PROJ/.cache/parity/$APP}"
DRIVER="$PROJ/scripts/parity/drivers/$APP.sh"
[ -x "$DRIVER" ] || { echo "missing driver: $DRIVER"; exit 1; }
parity_env
kill_apps
mkdir -p "$OUT"
"$DRIVER" "$OUT"
kill_apps
echo "captured into $OUT:"
ls "$OUT"

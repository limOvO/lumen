#!/usr/bin/env bash
# Full parity pass: capture both builds, build pair sheets, measure benchmarks.
set -u
PROJ="$(cd "$(dirname "$0")/../.." && pwd)"
"$PROJ/scripts/parity/capture.sh" web || exit 1
"$PROJ/scripts/parity/capture.sh" egui || exit 1
"$PROJ/scripts/parity/compare.sh"
echo "--- benchmarks ---"
"$PROJ/scripts/bench-vs-loupe.sh" 3 || true
"$PROJ/scripts/bench-egui.sh" 3 || true

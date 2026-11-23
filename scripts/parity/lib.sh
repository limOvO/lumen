#!/usr/bin/env bash
# Shared helpers for parity captures. Uses a private Xvfb display (default :97)
# so it never fights with the other tests/teammates.
set -u
PROJ="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DISPLAY_NUM="${PARITY_DISPLAY:-97}"
IMG_SMALL="${IMG_SMALL:-/tmp/parity-small.jpg}"
IMG_BIG="${IMG_BIG:-/tmp/parity-big.jpg}"
export XDG_RUNTIME_DIR="$PROJ/.cache/parity/rt"
export TMPDIR="$PROJ/.cache/parity/tmp"
export DISPLAY=":$DISPLAY_NUM"
export LIBGL_ALWAYS_SOFTWARE=1
export WEBKIT_DISABLE_COMPOSITING_MODE=1
export GDK_BACKEND=x11
export WGPU_BACKEND="${WGPU_BACKEND:-gl}"

parity_env() {
  mkdir -p "$PROJ/.cache/parity" "$XDG_RUNTIME_DIR" "$TMPDIR" /tmp/.X11-unix
  chmod 1777 /tmp/.X11-unix 2>/dev/null || true
  rm -f "/tmp/.X${DISPLAY_NUM}-lock" 2>/dev/null || true
  [ -f "$IMG_SMALL" ] || cp "$PROJ/src/samples/coast.jpg" "$IMG_SMALL"
  if [ ! -f "$IMG_BIG" ]; then
    python3 - "$IMG_SMALL" "$IMG_BIG" <<'PY'
import sys
from PIL import Image
src, dst = sys.argv[1], sys.argv[2]
Image.open(src).convert('RGB').resize((4000, 3000)).save(dst, quality=88)
PY
  fi
  pgrep -f "Xvfb :$DISPLAY_NUM" >/dev/null 2>&1 || {
    Xvfb ":$DISPLAY_NUM" -screen 0 1440x900x24 -nolisten tcp -extension GLX >/dev/null 2>&1 &
    sleep 1.5
  }
}

shot() { # shot <dir> <name>
  mkdir -p "$1"
  import -display ":$DISPLAY_NUM" -window root "$1/$2.png" 2>/dev/null
  echo "$2.png"
}

focus_window() { # focus_window <regex>
  local id
  id=$(xdotool search --onlyvisible --name "$1" 2>/dev/null | head -1) || return 0
  [ -n "$id" ] && xdotool windowfocus "$id" >/dev/null 2>&1 || true
  sleep 0.15
}

wait_window() { # wait_window <regex> [timeout_ms]
  local re="$1" limit="${2:-8000}" start=$(date +%s%N)
  while :; do
    if xdotool search --onlyvisible --name "$re" >/dev/null 2>&1; then return 0; fi
    [ $(( ($(date +%s%N) - start) / 1000000 )) -gt "$limit" ] && return 1
    sleep 0.05
  done
}

click() { # click <x> <y> [sleep]
  xdotool mousemove "$1" "$2" click 1
  sleep "${3:-0.35}"
}

key() { xdotool key --clearmodifiers "$1"; sleep 0.35; }

drag() { # drag <x1> <y1> <x2> <y2>
  local x1=$1 y1=$2 x2=$3 y2=$4 i
  xdotool mousemove "$x1" "$y1"; sleep 0.15
  xdotool mousedown 1; sleep 0.15
  for i in 1 2 3 4; do
    xdotool mousemove $(( x1 + (x2-x1)*i/4 )) $(( y1 + (y2-y1)*i/4 )); sleep 0.08
  done
  xdotool mouseup 1; sleep 0.25
}

kill_apps() { pkill -x lumen >/dev/null 2>&1; pkill -x lumen-egui-app >/dev/null 2>&1; sleep 0.4; }

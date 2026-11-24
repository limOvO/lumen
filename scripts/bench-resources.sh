#!/usr/bin/env bash
# Resource comparison: Lumen (egui rewrite) vs Lumen (Tauri/WebKit) vs Loupe.
# Usage: scripts/bench-resources.sh [runs]
set -uo pipefail
PROJ="$(cd "$(dirname "$0")/.." && pwd)"
RUNS="${1:-3}"
DISP="${DISP:-:99}"
export DISPLAY="$DISP"
export XDG_RUNTIME_DIR="$PROJ/.cache/bench-rt"
export TMPDIR="$PROJ/.cache/bench-tmp"
export LIBGL_ALWAYS_SOFTWARE=1 GDK_BACKEND=x11 WGPU_BACKEND=gl
export WEBKIT_DISABLE_COMPOSITING_MODE=1
export LUMEN_LANG=zh_TW
mkdir -p "$XDG_RUNTIME_DIR" "$TMPDIR" /tmp/.X11-unix
chmod 1777 /tmp/.X11-unix 2>/dev/null
rm -f "/tmp/.X${DISP#:}-lock" 2>/dev/null
IMG="${IMG:-/tmp/qa-big.jpg}"

start_xvfb() {
  pkill -x Xvfb >/dev/null 2>&1
  sleep 0.3
  Xvfb "$DISP" -screen 0 1440x900x24 -nolisten tcp >/dev/null 2>&1 &
  sleep 1.2
}

tree_pids() { # root pid -> pid list (root + descendants)
  local root="$1"; echo "$root"
  local kids; kids=$(pgrep -P "$root" 2>/dev/null || true)
  for k in $kids; do tree_pids "$k"; done
}

sample_pss() { # pid list -> "PSS_KB RSS_KB ANON_KB THREADS FDS"
  local pss=0 rss=0 anon=0 thr=0 fds=0
  for p in "$@"; do
    [ -r "/proc/$p/smaps_rollup" ] || continue
    pss=$((pss + $(awk '/^Pss:/{print $2}' "/proc/$p/smaps_rollup")))
    anon=$((anon + $(awk '/^Pss_Anon:/{print $2}' "/proc/$p/smaps_rollup")))
    rss=$((rss + $(awk '/^Rss:/{print $2}' "/proc/$p/smaps_rollup")))
    thr=$((thr + $(awk '/^Threads:/{print $2}' "/proc/$p/status" 2>/dev/null || echo 0)))
    fds=$((fds + $(ls "/proc/$p/fd" 2>/dev/null | wc -l)))
  done
  echo "$pss $rss $anon $thr $fds"
}

cpu_ticks() { # pid list -> sum utime+stime
  local sum=0
  for p in "$@"; do
    [ -r "/proc/$p/stat" ] || continue
    sum=$((sum + $(awk '{print $14+$15}' "/proc/$p/stat")))
  done
  echo "$sum"
}

measure_once() { # binary label args...
  local bin="$1" label="$2" extra="${3:-}"
  local pid wid
  "$bin" $extra "$IMG" >/dev/null 2>&1 & pid=$!
  for i in $(seq 1 200); do
    wid=$(xdotool search --onlyvisible --pid "$pid" 2>/dev/null | head -1)
    [ -z "$wid" ] && wid=$(xdotool search --onlyvisible --name . 2>/dev/null | tail -1)
    [ -n "$wid" ] && break
    sleep 0.05
  done
  sleep 3
  local pids; pids=$(tree_pids "$pid" | tr '\n' ' ')
  local m1; m1=$(sample_pss $pids)
  local t1; t1=$(cpu_ticks $pids)
  sleep 6
  local t2; t2=$(cpu_ticks $pids)
  local idle_ticks=$((t2 - t1))
  xdotool windowfocus "$wid" 2>/dev/null
  xdotool key --clearmodifiers e 2>/dev/null
  sleep 1.2
  local t3; t3=$(cpu_ticks $pids)
  sleep 6
  local t4; t4=$(cpu_ticks $pids)
  local edit_idle=$((t4 - t3))
  local hwm=0
  for p in $pids; do
    [ -r "/proc/$p/status" ] && hwm=$((hwm + $(awk '/^VmHWM:/{print $2}' "/proc/$p/status" 2>/dev/null || echo 0)))
  done
  kill "$pid" 2>/dev/null; wait "$pid" 2>/dev/null
  echo "$label $m1 $hwm $idle_ticks $edit_idle"
}

median() { sort -n | awk '{a[NR]=$1} END{ if (NR%2) print a[(NR+1)/2]; else print (a[NR/2]+a[NR/2+1])/2 }'; }

for label in egui web loupe; do
  case $label in
    egui) bin="$PROJ/egui-app/target/release/lumen-egui-app";;
    web) bin="$PROJ/dist/lumen";;
    loupe) bin="$(command -v loupe)";;
  esac
  start_xvfb
  results=()
  for r in $(seq 1 "$RUNS"); do
    results+=("$(measure_once "$bin" "$label" | sed "s/^$label //")")
  done
  echo "== $label =="
  printf '%s\n' "${results[@]}"
done
pkill -x Xvfb >/dev/null 2>&1

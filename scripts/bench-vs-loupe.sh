#!/usr/bin/env bash
# Reproduces the "Why Lumen" measurements: window latency, time until the photo
# is on screen, and PSS memory, for Lumen and (optionally) Loupe.
#
# Requirements: Xvfb, xdotool, ffmpeg, ImageMagick, loupe (for the comparison),
#               a display-less session works fine.
# Usage: scripts/bench-vs-loupe.sh [path-to-image] [runs]
set -u
PROJ="$(cd "$(dirname "$0")/.." && pwd)"
IMG="${1:-$PROJ/src/samples/coast.jpg}"
RUNS="${2:-4}"
OUT="$PROJ/.cache/bench"
mkdir -p "$OUT" "$PROJ/.cache/rt" "$PROJ/.cache/tmp" /tmp/.X11-unix
chmod 1777 /tmp/.X11-unix 2>/dev/null
rm -f /tmp/.X99-lock
export XDG_RUNTIME_DIR="$PROJ/.cache/rt" TMPDIR="$PROJ/.cache/tmp"
Xvfb :99 -screen 0 1440x900x24 -nolisten tcp -extension GLX >/dev/null 2>&1 &
XPID=$!
sleep 1.5
export DISPLAY=:99 LIBGL_ALWAYS_SOFTWARE=1 WEBKIT_DISABLE_COMPOSITING_MODE=1 GDK_BACKEND=x11 GSK_RENDERER=cairo

LUMEN="$PROJ/dist/lumen"
[ -x "$LUMEN" ] || { echo "build first: ./scripts/build.sh"; kill $XPID; exit 1; }

win_ms() { local class="$1" t0="$2" i
  for i in $(seq 1 3000); do
    if xdotool search --onlyvisible --classname "$class" >/dev/null 2>&1; then
      echo $(( ($(date +%s%N) - t0) / 1000000 )); return
    fi
  done; echo -1; }

pss_sum() {
  local root="$1"
  local total=0
  local pids="$root"
  local frontier="$root"
  local next p c v
  while [ -n "$frontier" ]; do
    next=""
    for p in $frontier; do
      for c in $(pgrep -P "$p" 2>/dev/null); do next="$next $c"; pids="$pids $c"; done
    done
    frontier="$next"
  done
  for p in $pids; do
    if [ -r "/proc/$p/smaps_rollup" ]; then
      v=$(awk '/^Pss:/{print $2}' "/proc/$p/smaps_rollup" 2>/dev/null)
      [ -n "$v" ] && total=$((total + v))
    fi
  done
  printf '%s' "$total"
}

# usage: measure <tag> <class> <cmd...>
measure() {
  local tag="$1" class="$2"; shift 2
  local run raw ff t0 w app mem
  for run in $(seq 1 "$RUNS"); do
    raw="$OUT/$tag-$run.raw"
    ffmpeg -loglevel error -f x11grab -framerate 60 -video_size 1440x900 -i :99 -t 4.2 \
      -vf "scale=96:60,format=rgb24" -f rawvideo -pix_fmt rgb24 -y "$raw" & ff=$!
    sleep 0.15
    t0=$(date +%s%N)
    "$@" >/dev/null 2>&1 & app=$!
    w=$(win_ms "$class" "$t0")
    wait $ff 2>/dev/null
    sleep 0.3
    mem=$(pss_sum "$app")
    echo "${tag} ${w:-0} ${mem:-0}" >> "$OUT/results.txt"
    kill $app 2>/dev/null; wait $app 2>/dev/null
    sleep 1.0
  done
}

: > "$OUT/results.txt"
echo "measuring Lumen ($IMG)…"
measure lumen lumen "$LUMEN" "$IMG"
if command -v loupe >/dev/null 2>&1; then
  echo "measuring Loupe…"
  measure loupe loupe loupe "$IMG"
fi

python3 - "$OUT" <<'PY'
import glob, os, statistics, sys
out = sys.argv[1]
n = 96*60*3
def frames(path):
    d = open(path, 'rb').read()
    return [d[i:i+n] for i in range(0, len(d), n) if len(d[i:i+n]) == n]
def stats(f):
    mean = sum(f)/len(f)
    coloured = sum(abs(f[i]-f[i+2]) + abs(f[i+1]-f[i+2]) for i in range(0, len(f), 21)) / (len(f)/21)
    return mean, coloured
rows = {}
for line in open(os.path.join(out, 'results.txt')):
    tag, win, mem = line.split()
    rows.setdefault(tag, []).append({'win': int(win), 'mem': int(mem)})
for tag in rows:
    for run, path in enumerate(sorted(glob.glob(os.path.join(out, tag + '-*.raw')))):
        fs = frames(path)
        if not fs or run >= len(rows[tag]):
            continue
        ms = [stats(f) for f in fs]
        base = ms[0][0]
        fw = next((i for i, (m, c) in enumerate(ms) if m > max(3, base + 2)), None)
        fi = next((i for i, (m, c) in enumerate(ms) if c > 15), None)
        if fw is not None and fi is not None:
            rows[tag][run]['photo'] = rows[tag][run]['win'] + (fi - fw) * 1000 / 60
for tag, rs in rows.items():
    win = statistics.median([r['win'] for r in rs])
    mem = statistics.median([r['mem'] for r in rs]) / 1024
    ph = [r['photo'] for r in rs if 'photo' in r]
    photo = statistics.median(ph) if ph else float('nan')
    print('%-6s window %4d ms   photo %4d ms   PSS %5.1f MB   (%d runs)' % (tag, win, photo, mem, len(rs)))
PY
rm -rf "$OUT"/*.raw
kill $XPID 2>/dev/null

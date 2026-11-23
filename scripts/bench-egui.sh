#!/usr/bin/env bash
# Measure the egui build: time to window, time to photo, PSS.
# Usage: scripts/bench-egui.sh [runs]   (needs Xvfb + ffmpeg + xdotool)
set -u
PROJ="$(cd "$(dirname "$0")/.." && pwd)"
RUNS="${1:-3}"
BIN="$PROJ/egui-app/target/release/lumen-egui-app"
[ -x "$BIN" ] || { echo "build first: cd egui-app && cargo build --release"; exit 1; }
OUT="$PROJ/.cache/bench-egui"
mkdir -p "$OUT" /tmp/.X11-unix "$PROJ/.cache/rt" "$PROJ/.cache/tmp"
chmod 1777 /tmp/.X11-unix 2>/dev/null; rm -f /tmp/.X99-lock
export XDG_RUNTIME_DIR="$PROJ/.cache/rt" TMPDIR="$PROJ/.cache/tmp"
Xvfb :99 -screen 0 1440x900x24 -nolisten tcp -extension GLX >/dev/null 2>&1 & XPID=$!
sleep 1.5
export DISPLAY=:99 LIBGL_ALWAYS_SOFTWARE=1 GDK_BACKEND=x11 WGPU_BACKEND=gl
: > "$OUT/results.txt"
for tag in small big; do
  IMG="/tmp/qa-$tag.jpg"
  for run in $(seq 1 "$RUNS"); do
    before=$(xdotool search --onlyvisible --name "Lumen" 2>/dev/null | sort | tr '\n' ' ')
    raw="$OUT/$tag-$run.raw"
    ffmpeg -loglevel error -f x11grab -framerate 60 -video_size 1440x900 -i :99 -t 3.0 \
      -vf "scale=96:60,format=rgb24" -f rawvideo -pix_fmt rgb24 -y "$raw" & ff=$!
    sleep 0.15; t0=$(date +%s%N)
    "$BIN" "$IMG" >/dev/null 2>&1 & app=$!
    w=-1
    for i in $(seq 1 3000); do
      now=$(xdotool search --onlyvisible --name "Lumen" 2>/dev/null | sort | tr '\n' ' ')
      if [ "$now" != "$before" ]; then w=$(( ($(date +%s%N)-t0)/1000000 )); break; fi
    done
    wait $ff 2>/dev/null; sleep 0.4
    pss=$(awk '/^Pss:/{print $2}' /proc/$app/smaps_rollup 2>/dev/null); pss=${pss:-0}
    echo "$tag $run $w $pss" >> "$OUT/results.txt"
    kill $app 2>/dev/null; wait $app 2>/dev/null; sleep 0.6
  done
done
kill $XPID 2>/dev/null
python3 - "$OUT" <<'PY'
import sys, statistics
out = sys.argv[1]
n = 96*60*3
def frames(p):
    d = open(p, 'rb').read()
    return [d[i:i+n] for i in range(0, len(d), n) if len(d[i:i+n]) == n]
def metrics(f):
    mean = sum(f)/len(f)
    cf = sum(abs(f[i]-f[i+2]) + abs(f[i+1]-f[i+2]) for i in range(0, len(f), 21)) / (len(f)/21)
    return mean, cf
rows = {}
for line in open(out + '/results.txt'):
    tag, run, w, pss = line.split()
    rows[(tag, int(run))] = (int(w), int(pss))
res = {}
for (tag, run), (w, pss) in sorted(rows.items()):
    fs = frames('%s/%s-%d.raw' % (out, tag, run))
    ms = [metrics(f) for f in fs]
    base = ms[0][0]
    fw = next((i for i, (m, c) in enumerate(ms) if m > max(3, base + 2)), None)
    fi = next((i for i, (m, c) in enumerate(ms) if c > 15), None)
    photo = (w + (fi - fw) * 1000/60) if (fw is not None and fi is not None and w >= 0) else None
    res.setdefault(tag, []).append((w, photo, pss))
for tag, rs in res.items():
    ph = [r[1] for r in rs if r[1] is not None]
    print('%-5s window %4d ms   photo %s ms   PSS %5.1f MB' % (
        tag, statistics.median([r[0] for r in rs]),
        '%4d' % int(statistics.median(ph)) if ph else ' n/a',
        statistics.median([r[2] for r in rs]) / 1024))
PY

#!/usr/bin/env bash
# Lumen app icon generator — no external image assets, only vector math.
#
#   scripts/gen-icons.sh            # regenerate src-tauri/icons + packaging/icons
#
# Requires: python3, ImageMagick 7 (`magick`, which rasterises the SVG through
# its librsvg delegate). 512px master is the source of truth for every size.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK="$ROOT/.cache/icons"
mkdir -p "$WORK"

BG="#15171c"          # app background (dark rounded square)
APERTURE="#ffb457"    # warm amber aperture ring
BRUSH="#ff5c5c"       # primary red brush stroke

python3 - "$WORK/lumen-icon.svg" "$BG" "$APERTURE" "$BRUSH" <<'PY'
import math, sys

out, bg, aperture, brush = sys.argv[1:5]
S = 512
CX, CY, R = 232.0, 214.0, 118.0   # aperture centre / ring radius
RING = 34.0                       # ring thickness (heavy so 16px stays legible)
DARK = bg


def bezier(p0, p1, p2, p3, t):
    mt = 1.0 - t
    x = mt**3 * p0[0] + 3 * mt**2 * t * p1[0] + 3 * mt * t**2 * p2[0] + t**3 * p3[0]
    y = mt**3 * p0[1] + 3 * mt**2 * t * p1[1] + 3 * mt * t**2 * p2[1] + t**3 * p3[1]
    return x, y


def bezier_tangent(p0, p1, p2, p3, t):
    mt = 1.0 - t
    dx = 3 * mt**2 * (p1[0] - p0[0]) + 6 * mt * t * (p2[0] - p1[0]) + 3 * t**2 * (p3[0] - p2[0])
    dy = 3 * mt**2 * (p1[1] - p0[1]) + 6 * mt * t * (p2[1] - p1[1]) + 3 * t**2 * (p3[1] - p2[1])
    return dx, dy


# Tapered brush sweep: fat heel bottom-right, thinning tail toward the corner.
P0, P1, P2, P3 = (300.0, 436.0), (326.0, 392.0), (388.0, 362.0), (452.0, 296.0)
W_HEEL, W_TIP = 54.0, 9.0
SAMPLES = 48


def width_at(t):
    return W_TIP + (W_HEEL - W_TIP) * (1.0 - t) ** 1.45


left, right = [], []
for i in range(SAMPLES + 1):
    t = i / SAMPLES
    x, y = bezier(P0, P1, P2, P3, t)
    dx, dy = bezier_tangent(P0, P1, P2, P3, t)
    n = math.hypot(dx, dy) or 1.0
    nx, ny = -dy / n, dx / n
    half = width_at(t) / 2.0
    left.append((x + nx * half, y + ny * half))
    right.append((x - nx * half, y - ny * half))

pts = left + right[::-1]
d = "M " + " L ".join(f"{x:.2f} {y:.2f}" for x, y in pts) + " Z"

parts = []
parts.append('<?xml version="1.0" encoding="UTF-8"?>')
parts.append(f'<svg xmlns="http://www.w3.org/2000/svg" width="{S}" height="{S}" viewBox="0 0 {S} {S}">')

# Rounded square, 16px breathing room so it does not butt against the tile edge.
parts.append(f'<rect x="16" y="16" width="480" height="480" rx="108" fill="{DARK}"/>')

# Soft inner hairline keeps the tile from looking like a flat blob on dark docks.
parts.append('<rect x="21" y="21" width="470" height="470" rx="103" fill="none" '
             'stroke="#ffffff" stroke-opacity="0.06" stroke-width="2"/>')

# Brush sweep first, so the aperture reads as the foreground subject.
parts.append(f'<path d="{d}" fill="{brush}"/>')
parts.append(f'<circle cx="{P0[0]}" cy="{P0[1]}" r="{W_HEEL / 2:.2f}" fill="{brush}"/>')

# Aperture ring.
parts.append(f'<circle cx="{CX}" cy="{CY}" r="{R}" fill="none" stroke="{aperture}" '
             f'stroke-width="{RING}"/>')

# Six dark blades cutting the ring — these make it read as an aperture rather
# than a plain circle, and stay visible at 16/32px because they are wide.
blade_w = 17.0
for k in range(6):
    a = math.radians(15 + k * 60)
    r0, r1 = R - RING / 2 - 4, R + RING / 2 + 4
    x0, y0 = CX + r0 * math.cos(a), CY + r0 * math.sin(a)
    x1, y1 = CX + r1 * math.cos(a), CY + r1 * math.sin(a)
    parts.append(f'<line x1="{x0:.2f}" y1="{y0:.2f}" x2="{x1:.2f}" y2="{y1:.2f}" '
                 f'stroke="{DARK}" stroke-width="{blade_w}" stroke-linecap="butt"/>')

parts.append('</svg>')
open(out, 'w', encoding='utf-8').write('\n'.join(parts))
print(out)
PY

MASTER="$WORK/lumen-512.png"
magick -background none "$WORK/lumen-icon.svg" -resize 512x512 "$MASTER"

# Tauri expects these exact names.
TAURI="$ROOT/src-tauri/icons"
mkdir -p "$TAURI"
cp "$MASTER" "$TAURI/icon.png"
for size in 16 32 48 64 128 256 512; do
  magick "$MASTER" -filter Lanczos -resize "${size}x${size}" \
    -define png:color-type=6 "$TAURI/${size}x${size}.png"
done
cp "$TAURI/512x512.png" "$TAURI/icon.png"
# 128x128@2x is the HiDPI slot some Tauri bundlers look for.
cp "$TAURI/256x256.png" "$TAURI/128x128@2x.png"

# Freedesktop hicolor tree for scripts/install.sh.
HICOLOR="$ROOT/packaging/icons/hicolor"
rm -rf "$HICOLOR"
for size in 16 22 24 32 48 64 96 128 256 512; do
  dir="$HICOLOR/${size}x${size}/apps"
  mkdir -p "$dir"
  magick "$MASTER" -filter Lanczos -resize "${size}x${size}" \
    -define png:color-type=6 "$dir/lumen.png"
done

# Contact sheet for eyeballing the small sizes (not shipped).
magick montage "$TAURI/16x16.png" "$TAURI/32x32.png" "$TAURI/48x48.png" \
  "$TAURI/64x64.png" "$TAURI/128x128.png" "$TAURI/256x256.png" \
  -tile 6x1 -geometry +8+8 -background '#2a2d33' "$WORK/contact-sheet.png"

echo "icon: $(magick identify -format '%wx%h' "$MASTER") master"
echo "tauri icons:"; ls -1 "$TAURI"
echo "hicolor:"; ls -1 "$HICOLOR"
echo "alpha check (corner must be 0):"
magick "$TAURI/icon.png" -format 'corner_alpha=%[fx:p{2,2}.a] centre_alpha=%[fx:p{256,256}.a]' info:
echo

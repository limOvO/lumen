#!/usr/bin/env python3
"""Generate ui/tool_icons.rs from the web build's inline SVGs (rail tools).

The egui build originally hand-traced icons, which drifted (brush/text/crop).
This script flattens the real SVG paths so both builds share identical geometry.
"""
import re
from pathlib import Path

from svg.path import parse_path

ROOT = Path(__file__).resolve().parents[1]
HTML = (ROOT / "src/index.html").read_text(encoding="utf-8")
OUT = ROOT / "egui-app/src/ui/svg_icons.rs"

STEP = 0.35  # sampling step in the 24x24 viewBox


def flatten_d(d: str):
    """Flatten one SVG `d` attribute into subpaths of sampled points."""
    subpaths = []
    for chunk in re.split(r"(?=M)", d):
        chunk = chunk.strip()
        if not chunk:
            continue
        path = parse_path(chunk)
        pts = []
        for seg in path:
            kind = type(seg).__name__
            if kind == "Line":
                for t in (0.0, 1.0):
                    p = seg.point(t)
                    pts.append((round(p.real, 3), round(p.imag, 3)))
                continue
            try:
                length = seg.length(error=1e-3)
            except Exception:
                length = 1.0
            n = max(2, min(120, int(length / STEP) + 1))
            for i in range(n + 1):
                p = seg.point(i / n)
                pts.append((round(p.real, 3), round(p.imag, 3)))
        # drop consecutive duplicates
        dedup = [pts[0]] if pts else []
        for p in pts[1:]:
            if p != dedup[-1]:
                dedup.append(p)
        if len(dedup) >= 2:
            subpaths.append(dedup)
    return subpaths


def rounded_rect(x, y, w, h, r):
    r = min(r, w / 2, h / 2)
    pts = []
    import math
    corners = [
        (x + w - r, y + r, -math.pi / 2, 0.0),
        (x + w - r, y + h - r, 0.0, math.pi / 2),
        (x + r, y + h - r, math.pi / 2, math.pi),
        (x + r, y + r, math.pi, 3 * math.pi / 2),
    ]
    for cx, cy, a0, a1 in corners:
        for i in range(9):
            a = a0 + (a1 - a0) * i / 8
            pts.append((round(cx + r * math.cos(a), 3), round(cy + r * math.sin(a), 3)))
    pts.append(pts[0])
    return [pts]


def ellipse(cx, cy, rx, ry):
    import math
    pts = []
    for i in range(49):
        a = 2 * math.pi * i / 48
        pts.append((round(cx + rx * math.cos(a), 3), round(cy + ry * math.sin(a), 3)))
    return [pts]


def all_ui_icons():
    """Every inline SVG in index.html, named by its parent element id."""
    out = []
    seen = set()
    for m in re.finditer(r"<svg\b[^>]*>.*?</svg>", HTML, re.S):
        start = m.start()
        before = HTML[max(0, start - 500):start]
        ids = re.findall(r'id="([^"]+)"', before)
        if not ids:
            continue
        name = ids[-1]
        if name in seen:
            continue
        svg = m.group(0)
        attrs = dict(re.findall(r'(\w[\w-]*)="([^"]*)"', svg[:svg.index(">")]))
        paths = []
        for dm in re.finditer(r'<path[^>]*\sd="([^"]+)"', svg):
            paths.extend(flatten_d(dm.group(1)))
        for rm in re.finditer(r'<rect([^>]*)/?>', svg):
            ra = dict(re.findall(r'(\w[\w-]*)="([^"]*)"', rm.group(1)))
            paths.extend(rounded_rect(float(ra.get("x", 0)), float(ra.get("y", 0)),
                                      float(ra.get("width", 0)), float(ra.get("height", 0)),
                                      float(ra.get("rx", 0))))
        for em in re.finditer(r'<ellipse([^>]*)/?>', svg):
            ea = dict(re.findall(r'(\w[\w-]*)="([^"]*)"', em.group(1)))
            paths.extend(ellipse(float(ea.get("cx", 12)), float(ea.get("cy", 12)),
                                 float(ea.get("rx", 8)), float(ea.get("ry", 8))))
        for cm in re.finditer(r'<circle([^>]*)/?>', svg):
            ca = dict(re.findall(r'(\w[\w-]*)="([^"]*)"', cm.group(1)))
            paths.extend(ellipse(float(ca.get("cx", 12)), float(ca.get("cy", 12)),
                                 float(ca.get("r", 8)), float(ca.get("r", 8))))
        if paths:
            seen.add(name)
            out.append((name, float(attrs.get("stroke-width", 1.8)), paths))
    return out


def icons():
    out = []
    for m in re.finditer(
        r'<button id="(t-[a-z]+)" class="rail-btn".*?</button>', HTML, re.S
    ):
        name = m.group(1)[2:]
        body = m.group(0)
        paths = []
        for dm in re.finditer(r'<path[^>]*\sd="([^"]+)"', body):
            paths.extend(flatten_d(dm.group(1)))
        for rm in re.finditer(r'<rect([^>]*)/?>', body):
            attrs = dict(re.findall(r'(\w[\w-]*)="([^"]*)"', rm.group(1)))
            paths.extend(
                rounded_rect(
                    float(attrs.get("x", 0)),
                    float(attrs.get("y", 0)),
                    float(attrs.get("width", 0)),
                    float(attrs.get("height", 0)),
                    float(attrs.get("rx", 0)),
                )
            )
        for em in re.finditer(r'<ellipse([^>]*)/?>', body):
            attrs = dict(re.findall(r'(\w[\w-]*)="([^"]*)"', em.group(1)))
            paths.extend(
                ellipse(
                    float(attrs.get("cx", 12)),
                    float(attrs.get("cy", 12)),
                    float(attrs.get("rx", 8)),
                    float(attrs.get("ry", 8)),
                )
            )
        if paths:
            out.append((name, paths))
    return out


def num(v):
    t = f"{v:g}"
    return t if "." in t else t + ".0"


def main():
    data = icons()
    lines = [
        "// @generated by scripts/gen-icons.py from src/index.html — do not edit.",
        "// Exact SVG geometry (24x24 viewBox) for the rail tool buttons.",
        "",
        "pub struct ToolIcon {",
        "    pub name: &'static str,",
        "    pub paths: &'static [&'static [(f32, f32)]],",
        "}",
        "",
        "pub static RAIL: &[ToolIcon] = &[",
    ]
    for name, paths in data:
        lines.append(f'    ToolIcon {{ name: "{name}", paths: &[')
        for pts in paths:
            body = ", ".join(f"({num(x)}, {num(y)})" for x, y in pts)
            lines.append(f"        &[{body}],")
        lines.append("    ]},")
    lines.append("];")
    lines.append("")
    ui = all_ui_icons()
    lines.append("pub static UI: &[ToolIcon] = &[")
    for name, stroke, paths in ui:
        lines.append(f'    ToolIcon {{ name: "{name}", paths: &[')
        for pts in paths:
            body = ", ".join(f"({num(x)}, {num(y)})" for x, y in pts)
            lines.append(f"        &[{body}],")
        lines.append("    ]},")
    lines.append("];")
    lines.append("")
    lines.append("pub fn rail(name: &str) -> Option<&'static ToolIcon> {")
    lines.append("    RAIL.iter().find(|i| i.name == name)")
    lines.append("}")
    lines.append("")
    lines.append("pub fn ui(name: &str) -> Option<&'static ToolIcon> {")
    lines.append("    UI.iter().find(|i| i.name == name)")
    lines.append("}")
    OUT.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"wrote {OUT.relative_to(ROOT)} with {len(data)} rail + {len(ui)} ui icons")


if __name__ == "__main__":
    main()

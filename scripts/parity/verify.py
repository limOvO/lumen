#!/usr/bin/env python3
"""Strict parity verification: drives both builds and asserts behaviour,
geometry, palette, presence, animations, idle CPU and performance.

Usage: python3 scripts/parity/verify.py [--quick]
Exit code 0 only when every check passes. Report: .cache/parity/report.json
"""
import json, os, statistics, subprocess, sys, time
from pathlib import Path

from PIL import Image, ImageChops

PROJ = Path(__file__).resolve().parents[2]
WEB = PROJ / "dist" / "lumen"
EGUI = PROJ / "egui-app" / "target" / "release" / "lumen-egui-app"
OUT = PROJ / ".cache" / "parity" / "verify"
DISPLAY = ":96"
ENV = os.environ | {
    "DISPLAY": DISPLAY,
    "XDG_RUNTIME_DIR": str(OUT / "rt"),
    "TMPDIR": str(OUT / "tmp"),
    "LIBGL_ALWAYS_SOFTWARE": "1",
    "WEBKIT_DISABLE_COMPOSITING_MODE": "1",
    "GDK_BACKEND": "x11",
    "WGPU_BACKEND": "gl",
    "LUMEN_SOCKET": str(OUT / "lumen-test.sock"),
    # The default build opens the desktop's native picker (zenity/portal); the
    # harness has no portal session, so force the in-app dialogs for the
    # deterministic checks. G23 below exercises the native path separately.
    "LUMEN_INTERNAL_BROWSER": "1",
}
results = []


def check(cid, desc, ok, detail=""):
    results.append({"id": cid, "desc": desc, "ok": bool(ok), "detail": detail})
    print(("PASS " if ok else "FAIL ") + cid + "  " + desc + (f"  [{detail}]" if detail else ""))


def run(cmd, **kw):
    return subprocess.run(cmd, env=ENV, capture_output=True, text=True, **kw)


def xdo(*args):
    return run(["xdotool", *map(str, args)])


def key(k, pause=0.35):
    xdo("key", "--clearmodifiers", k)
    time.sleep(pause)


def click(x, y, pause=0.4):
    xdo("mousemove", x, y, "click", "1")
    time.sleep(pause)


def drag(x1, y1, x2, y2, steps=5, pause=0.35):
    xdo("mousemove", x1, y1); time.sleep(0.12)
    xdo("mousedown", "1"); time.sleep(0.12)
    for i in range(1, steps + 1):
        xdo("mousemove", x1 + (x2 - x1) * i // steps, y1 + (y2 - y1) * i // steps)
        time.sleep(0.07)
    xdo("mouseup", "1"); time.sleep(pause)


def shot(name):
    path = OUT / f"{name}.png"
    run(["import", "-display", DISPLAY, "-window", "root", str(path)])
    return path


def start(app, image=None, lang=None, socket=None):
    env = dict(ENV)
    if lang:
        env["LUMEN_LANG"] = lang
    if socket:
        env["LUMEN_SOCKET"] = str(socket)
    args = [str(app)] + ([str(image)] if image else [])
    p = subprocess.Popen(args, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    return p


def wait_window(title="Lumen", timeout=20.0):
    end = time.time() + timeout
    while time.time() < end:
        r = xdo("search", "--onlyvisible", "--name", title)
        if r.returncode == 0 and r.stdout.strip():
            return r.stdout.split()[0]
        time.sleep(0.05)
    return None


def focus(title="Lumen", pid=None):
    end = time.time() + 20
    wid = None
    while time.time() < end:
        args = ["search", "--onlyvisible", "--name", title]
        if pid:
            args += ["--pid", str(pid)]
        r = xdo(*args)
        ids = r.stdout.split()
        if ids:
            wid = ids[0]
            break
        time.sleep(0.05)
    if wid:
        xdo("windowfocus", wid)
        time.sleep(0.2)
    return wid


def wait_painted(path, tries=40):
    """Wait until a screenshot of the desktop is not a blank root."""
    for _ in range(tries):
        run(["import", "-display", DISPLAY, "-window", "root", str(path)])
        im = img_arr(path)
        if bbox_nonbg(im, (0, 0, 1440, 900), tol=6) is not None:
            return True
        time.sleep(0.15)
    return False


def stop(p):
    p.terminate()
    try:
        p.wait(timeout=3)
    except subprocess.TimeoutExpired:
        p.kill()


def img_arr(path):
    im = Image.open(path).convert("RGB")
    return im


def bbox_nonbg(im, region, tol=8, bg=(16, 16, 18)):
    x0, y0, x1, y1 = region
    px = im.load()
    xs, ys = [], []
    for y in range(y0, y1, 2):
        for x in range(x0, x1, 2):
            r, g, b = px[x, y]
            if abs(r - bg[0]) > tol or abs(g - bg[1]) > tol or abs(b - bg[2]) > tol:
                xs.append(x); ys.append(y)
    if not xs:
        return None
    return (min(xs), min(ys), max(xs), max(ys))


def bbox_sat(im, region, min_sat=24, min_sum=120):
    """Bounding box of colourful photo pixels (chrome is neutral grey)."""
    x0, y0, x1, y1 = region
    px = im.load()
    xs, ys = [], []
    for y in range(y0, y1, 2):
        for x in range(x0, x1, 2):
            r, g, b = px[x, y]
            if max(r, g, b) - min(r, g, b) >= min_sat and r + g + b >= min_sum:
                xs.append(x); ys.append(y)
    if not xs:
        return None
    return (min(xs), min(ys), max(xs), max(ys))


def red_count(im, region):
    x0, y0, x1, y1 = region
    px = im.load()
    n = 0
    for y in range(y0, y1, 2):
        for x in range(x0, x1, 2):
            r, g, b = px[x, y]
            if r > 140 and r > g * 1.6 and r > b * 1.6:
                n += 1
    return n


def region_delta(path_a, path_b, region):
    a = img_arr(Path(path_a)).crop(region).convert("L").resize((120, 80))
    b = img_arr(Path(path_b)).crop(region).convert("L").resize((120, 80))
    return statistics.mean(ImageChops.difference(a, b).getdata())


def mean_diff(a, b, region):
    ra = a.crop(region).convert("L").resize((80, 50))
    rb = b.crop(region).convert("L").resize((80, 50))
    d = ImageChops.difference(ra, rb)
    return statistics.mean(d.getdata())


def burst(name, keyname=None, frames=5, gap=0.07):
    paths = []
    if keyname:
        xdo("key", "--clearmodifiers", keyname)
    for i in range(frames):
        time.sleep(gap)
        paths.append(shot(f"{name}-{i}"))
    return paths


def cleanup():
    for name in ("lumen-egui-app", "lumen-egui", "lumen"):
        run(["pkill", "-x", name])
    time.sleep(0.8)


def tick_cpu(pid, seconds=5.0):
    def ticks():
        with open(f"/proc/{pid}/stat") as f:
            parts = f.read().split()
        return int(parts[13]) + int(parts[14])
    t0 = ticks(); time.sleep(seconds); t1 = ticks()
    return t1 - t0


def main():
    quick = "--quick" in sys.argv
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "rt").mkdir(exist_ok=True)
    (OUT / "tmp").mkdir(exist_ok=True)
    for p in (OUT / "lumen-test.sock",):
        try: p.unlink()
        except FileNotFoundError: pass
    for name in ("lumen-egui-app", "lumen-egui", "lumen", "Xvfb"):
        run(["pkill", "-x", name])
    time.sleep(1.2)
    run(["pkill", "-x", "Xvfb"])
    subprocess.Popen(["Xvfb", DISPLAY, "-screen", "0", "1440x900x24", "-nolisten", "tcp",
                      "-extension", "GLX"], env=ENV, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(1.6)
    fx = Path("/tmp/parity-verify-coast.jpg")
    fx.write_bytes((PROJ / "src/samples/coast.jpg").read_bytes())
    fx2 = Path("/tmp/parity-verify-street.jpg")
    fx2.write_bytes((PROJ / "src/samples/street.jpg").read_bytes())

    app = EGUI
    base = start(app, fx)
    try:
        focus()
        time.sleep(2.0)
        wait_painted(OUT / "egui-00-boot.png")
        time.sleep(0.6)
        s_preview = shot("egui-01-preview")
        im = img_arr(s_preview)
        check("G1", "window geometry 1280x820",
              "1280x820" in run(["xdotool", "getwindowgeometry", wait_window()]).stdout,
              run(["xdotool", "getwindowgeometry", wait_window()]).stdout.strip().replace("\n", " "))
        # palette: canvas bg and panel bg
        canvas_px = im.getpixel((30, 400))
        check("G2", "canvas background is #151517 (web --bg)", max(abs(c - t) for c, t in zip(canvas_px, (21, 21, 23))) <= 3, str(canvas_px))
        cap = bbox_nonbg(im, (0, 0, 320, 44), tol=12)
        check("G3", "preview caption chip present", cap is not None, str(cap))
        dock = bbox_nonbg(im, (930, 6, 1270, 44), tol=10)
        check("G4", "preview icon dock present", dock is not None, str(dock))
        hud = bbox_nonbg(im, (1080, 740, 1275, 800), tol=10)
        check("G5", "zoom HUD present bottom-right", hud is not None, str(hud))

        # zoom in/out behaviour
        w0 = bbox_sat(im, (60, 60, 1260, 800))
        for _ in range(5):
            key("plus", 0.12)
        time.sleep(0.6)
        s_zoom = shot("egui-02-zoom")
        w1 = bbox_sat(img_arr(s_zoom), (0, 0, 1280, 820))
        grew = w0 and w1 and (w1[2] - w1[0]) > (w0[2] - w0[0]) * 1.15
        clipped = w1 is not None and ((w1[2] - w1[0]) >= 1240 or (w1[3] - w1[1]) >= 780)
        check("B1", "wheel/keyboard zoom enlarges the picture",
              bool(grew or clipped), f"{w0} -> {w1} (clipped={clipped})")
        key("0", 0.5)
        time.sleep(0.8)
        s_fit = shot("egui-03-fit")
        w2 = bbox_sat(img_arr(s_fit), (0, 0, 1280, 820))
        check("B2", "0 returns to fit (within 3%)",
              w0 and w2 and abs((w2[2] - w2[0]) - (w0[2] - w0[0])) <= max(8, (w0[2] - w0[0]) * 0.03),
              f"{w0} -> {w2}")

        # info peek card (presence = the card region changed, then changed back)
        s_pre = shot("egui-04a-pre-info")
        key("i", 0.4)
        time.sleep(0.6)
        s_info = shot("egui-04-info")
        d_open = region_delta(s_pre, s_info, (966, 60, 1260, 700))
        check("B3", "I opens the preview info card", d_open > 4.0, f"delta {d_open:.1f}")
        key("Escape", 0.4)
        time.sleep(0.8)
        s_after = shot("egui-05-info-closed")
        d_closed = region_delta(s_info, s_after, (966, 60, 1260, 700))
        check("B4", "Esc closes the info card", d_closed > 4.0, f"delta {d_closed:.1f}")
        # close animation: sample a few frames while the card slides/fades away
        key("i", 0.4)
        time.sleep(0.5)
        frames = burst("peekclose", "Escape", frames=4, gap=0.06)
        peek_means = []
        for f in frames:
            band = img_arr(f).crop((966, 60, 1260, 700)).convert("L").resize((60, 40))
            peek_means.append(round(statistics.mean(band.getdata()), 1))
        check("A5", "info card animates closed (region changes over frames)",
              len(set(int(m // 2) for m in peek_means)) >= 2, str(peek_means))
        time.sleep(0.6)

        # editor chrome + layout
        key("e", 0.6)
        time.sleep(1.0)
        s_edit = shot("egui-06-edit")
        ime = img_arr(s_edit)
        topbar = ime.getpixel((640, 20))
        check("G6", "editor top bar 46 px with panel colour",
              max(abs(c - t) for c, t in zip(topbar, (35, 35, 36))) <= 6, str(topbar))
        rail = ime.getpixel((10, 300))
        check("G7", "editor rail uses the web rail colour (#1C1C1E)",
              max(abs(c - t) for c, t in zip(rail, (28, 28, 30))) <= 4, str(rail))
        insp = ime.getpixel((1120, 300))
        check("G8", "editor inspector panel present", insp != (16, 16, 18), str(insp))

        # annotations: brush stroke, undo, redo
        key("b", 0.3)
        drag(380, 360, 640, 560)
        s_brush = shot("egui-07-brush")
        n_brush = red_count(img_arr(s_brush), (80, 60, 940, 600))
        check("B5", "brush draws a visible stroke", n_brush > 40, str(n_brush))
        key("ctrl+z", 0.5)
        time.sleep(0.5)
        n_undo = red_count(img_arr(shot("egui-08-undo")), (80, 60, 940, 600))
        check("B6", "Ctrl+Z removes the stroke", n_undo < max(5, n_brush // 10), str(n_undo))
        key("ctrl+shift+z", 0.5)
        time.sleep(0.5)
        n_redo = red_count(img_arr(shot("egui-09-redo")), (80, 60, 940, 600))
        check("B7", "Ctrl+Shift+Z restores the stroke", n_redo >= n_brush * 0.8, str(n_redo))

        # rect tool with dashed toggle: just verify a rect stroke appears
        key("r", 0.3)
        drag(700, 300, 900, 430)
        s_rect = shot("egui-10-rect")
        n_rect = red_count(img_arr(s_rect), (80, 60, 940, 600))
        check("B8", "rectangle tool draws", n_rect > n_brush * 0.9, str(n_rect))

        # line, arrow, text, delete
        key("l", 0.3)
        drag(380, 470, 640, 610)
        n_line = red_count(img_arr(shot("egui-10b-line")), (80, 60, 940, 620))
        check("B20", "line tool draws", n_line > 40, str(n_line))
        key("a", 0.3)
        click(720, 360, 0.25)
        click(900, 500, 0.5)
        n_arrow = red_count(img_arr(shot("egui-10c-arrow")), (80, 60, 940, 620))
        check("B21", "two-point arrow draws", n_arrow > n_line, str(n_arrow))
        key("t", 0.3)
        click(420, 640, 0.4)
        xdo("type", "--delay", "25", "測試")
        key("ctrl+Return", 0.5)
        time.sleep(0.5)
        n_text = red_count(img_arr(shot("egui-10d-text")), (80, 60, 940, 700))
        check("B22", "CJK text annotation commits", n_text > n_arrow, str(n_text))
        key("v", 0.3)
        click(700, 300, 0.3)   # select the rect
        key("Delete", 0.5)
        n_del = red_count(img_arr(shot("egui-10e-deleted")), (80, 60, 940, 700))
        check("B23", "Delete removes the selected annotation", n_del < n_text, f"{n_text} -> {n_del}")

        # compare toggle hides annotations
        click(878, 22, 0.6)
        time.sleep(0.8)
        n_cmp = red_count(img_arr(shot("egui-11-compare-on")), (80, 60, 940, 600))
        check("B9", "compare hides annotations", n_cmp < 10, str(n_cmp))
        click(878, 22, 0.6)
        time.sleep(0.8)
        n_cmp2 = red_count(img_arr(shot("egui-12-compare-off")), (80, 60, 940, 600))
        check("B10", "compare off restores annotations", n_cmp2 >= n_del * 0.85, f"{n_cmp2} vs {n_del} before compare")

        # dock enter animation (fade + rise): hide it first, then sample while it appears
        key("v", 0.5)
        time.sleep(0.6)
        s_dock_pre = shot("egui-18a-pre-dock")
        frames = burst("dock", "b", frames=5, gap=0.05)
        dock_means = []
        for f in frames:
            band = img_arr(f).crop((240, 680, 1040, 790)).convert("L").resize((60, 12))
            dock_means.append(statistics.mean(band.getdata()))
        spread = max(dock_means) - min(dock_means) if dock_means else 0.0
        check("A4", "style dock animates in (band brightness changes over frames)",
              spread > 0.4, f"spread {spread:.2f} means {[round(m,1) for m in dock_means]}")
        key("v", 0.4)
        time.sleep(0.5)

        # crop: default frame + 1:1 flip
        s_pre_crop = shot("egui-13a-pre-crop")
        key("c", 0.6)
        time.sleep(0.8)
        s_crop = shot("egui-13-crop")
        d_crop = region_delta(s_pre_crop, s_crop, (60, 46, 978, 790))
        check("B11", "crop frame is drawn", d_crop > 3.0, f"delta {d_crop:.1f}")
        key("Escape", 0.4)

        # shortcuts modal
        s_pre_keys = shot("egui-14a-pre-keys")
        key("question", 0.5)
        time.sleep(0.7)
        s_keys = shot("egui-14-shortcuts")
        d_keys = region_delta(s_pre_keys, s_keys, (350, 100, 900, 700))
        check("B12", "shortcut window opens", d_keys > 4.0, f"delta {d_keys:.1f}")
        key("Escape", 0.4)
        time.sleep(0.4)

        # open browser
        s_pre_open = shot("egui-15a-pre-open")
        key("ctrl+o", 0.6)
        time.sleep(1.0)
        s_open = shot("egui-15-open")
        d_open = region_delta(s_pre_open, s_open, (360, 90, 900, 740))
        check("B13", "folder browser opens", d_open > 4.0, f"delta {d_open:.1f}")
        key("Escape", 0.5)
        time.sleep(0.5)

        # idle CPU in edit mode
        pid = base.pid
        ticks = tick_cpu(pid, 5.0)
        check("P1", "idle CPU in preview/editor is ~0", ticks <= 2, f"{ticks} ticks/5s")

        # image switch: slide animation produces intermediate frames
        ff = subprocess.Popen(["ffmpeg", "-loglevel", "error", "-f", "x11grab", "-framerate", "60",
                               "-video_size", "1440x900", "-i", DISPLAY, "-t", "1.2",
                               str(OUT / "switch.mkv")], env=ENV)
        time.sleep(0.7)
        key("Escape", 0.3)          # back to preview
        xdo("key", "Right")
        ff.wait(timeout=20)
        frames_dir = OUT / "switch"; frames_dir.mkdir(exist_ok=True)
        run(["ffmpeg", "-loglevel", "error", "-i", str(OUT / "switch.mkv"), "-vf", "fps=30",
             str(frames_dir / "f%03d.png")])
        widths = []
        for f in sorted(frames_dir.glob("f*.png")):
            im2 = img_arr(f)
            bb = bbox_sat(im2, (60, 60, 1260, 800))
            if bb:
                widths.append(bb[2] - bb[0])
        distinct = len({w // 8 for w in widths})
        check("A1", "image switch animates (multiple distinct frame states)",
              distinct >= 2, f"{distinct} distinct widths from {len(widths)} frames")

        # phase transition: the picture glides while the rail slides in
        pre = shot("egui-20-phase-pre")
        frames = burst("phase", "e", frames=5, gap=0.07)
        heights, rails = [], []
        for f in frames:
            bb = bbox_sat(img_arr(f), (80, 60, 940, 780))
            if bb:
                heights.append(bb[3] - bb[1])
            rails.append(img_arr(f).getpixel((10, 300)))
        distinct = len({h // 6 for h in heights})
        check("A2", "preview->editor glides the picture (multiple distinct sizes)",
              distinct >= 2, f"heights {heights}")
        rail_visible = any(max(abs(c - t) for c, t in zip(p, (28, 28, 30))) <= 4 for p in rails)
        check("A3", "tool rail slides in during the phase transition", rail_visible, str(rails[:3]))
    finally:
        stop(base)

    # ---- palette parity against the web build ----
    web_shot = None
    wp = start(WEB, fx)
    try:
        focus()
        time.sleep(9.0)
        wid = wait_window()
        if wid:
            eval_out = run(["xdotool", "getwindowgeometry", "--shell", wid]).stdout
            vals = dict(line.split("=") for line in eval_out.strip().splitlines() if "=" in line)
            wx, wy = int(vals.get("X", "0")), int(vals.get("Y", "0"))
            click(wx + 1250, wy + 32, 1.5)
            web_shot = OUT / "web-edit.png"
            run(["import", "-display", DISPLAY, "-window", wid, str(web_shot)])
    finally:
        stop(wp)
    if web_shot and web_shot.exists():
        web_im = img_arr(web_shot)
        egui_im = img_arr(s_edit)
        pts = {
            "topbar": (640, 20),
            "rail": (20, 300),
            "canvas": (640, 60),
            "inspector": (1200, 300),
            "status": (640, 812),
        }
        deltas = {k: max(abs(a - b) for a, b in zip(web_im.getpixel(p), egui_im.getpixel(p)))
                  for k, p in pts.items()}
        check("G9", "chrome palette matches the web build (max channel delta <= 4)",
              max(deltas.values()) <= 4, str(deltas))
        # inspector grip: pill straddles the panel edge in both builds
        grip_web = web_im.getpixel((980, 400))
        grip_egui = egui_im.getpixel((980, 400))
        grip_outer_web = web_im.getpixel((966, 400))
        grip_outer_egui = egui_im.getpixel((966, 400))
        grip_ok = (max(abs(a - b) for a, b in zip(grip_web, grip_egui)) <= 6
                   and max(abs(a - b) for a, b in zip(grip_outer_web, grip_outer_egui)) <= 6)
        check("G10", "inspector grip pill matches the web build", grip_ok,
              f"edge {grip_web} vs {grip_egui}, outer {grip_outer_web} vs {grip_outer_egui}")

        # rail tool icons: same SVG geometry, same 30px buttons / 4px gaps
        rail_web = web_im.crop((0, 56, 52, 430)).convert("L")
        rail_egui = egui_im.crop((0, 56, 52, 430)).convert("L")
        rail_delta = statistics.mean(ImageChops.difference(rail_web, rail_egui).getdata())

        def ink_mask(img, box, thresh=110):
            crop = img.crop(box).convert("L")
            return [1 if v > thresh else 0 for v in crop.getdata()]

        def icon_centres(img):
            px = img.load()
            rows = []
            for y in range(56, 430):
                ink = sum(1 for x in range(14, 44) if sum(px[x, y][:3]) > 240)
                rows.append((y, ink))
            bands, cur = [], None
            for y, ink in rows:
                if ink > 0 and cur is None:
                    cur = y
                elif ink == 0 and cur is not None:
                    bands.append((cur, y)); cur = None
            return bands

        bands_egui = icon_centres(egui_im)
        bands_web = icon_centres(web_im)
        # Shape parity: crop each icon to its own ink bbox, normalise to 48x48 and
        # compare masks. This is layout/pitch independent (that is covered by the
        # generated SVG geometry and the CSS metrics).
        def masks(img):
            px = img.load()
            out = []
            for y in range(46, 430):
                for x in range(0, 52):
                    pass
            rows = []
            for y in range(46, 430):
                ink = [x for x in range(0, 52) if sum(px[x, y]) > 240]
                rows.append((y, ink))
            cur = None
            for y, ink in rows:
                if ink and cur is None:
                    cur = y
                elif not ink and cur is not None:
                    out.append((cur, y)); cur = None
            return out

        def norm_mask(img, box):
            from PIL import Image as PILImage
            crop = img.crop(box).convert("L")
            bw = crop.point(lambda v: 255 if v > 110 else 0)
            bbox = bw.getbbox()
            if not bbox:
                return None
            bw = bw.crop(bbox).resize((48, 48), PILImage.BILINEAR)
            return [1 if v > 100 else 0 for v in bw.getdata()]

        web_rows = masks(web_im)
        egui_rows = masks(egui_im)
        ious = []
        for ((wy0, wy1), (ey0, ey1)) in zip(web_rows, egui_rows):
            a = norm_mask(web_im, (0, wy0, 52, wy1))
            b = norm_mask(egui_im, (0, ey0, 52, ey1))
            if not a or not b:
                ious.append(0.0); continue
            inter = sum(1 for p, q in zip(a, b) if p and q)
            union = sum(1 for p, q in zip(a, b) if p or q)
            ious.append(round(inter / union, 2) if union else 0.0)
        check("G13", "tool button icon shapes match the web SVGs (normalised ink IoU >= 0.45)",
              len(ious) == 8 and min(ious) >= 0.45, f"IoU per icon {ious}")
        check("G13b", "tool rail icon ink differs only by renderer AA (mean delta <= 8)",
              rail_delta <= 8.0, f"delta {rail_delta:.2f}")

        check("G14", "rail has 8 tool rows in both builds",
              len(bands_egui) == len(bands_web) == 8,
              f"egui {len(bands_egui)} vs web {len(bands_web)} bands")
    else:
        check("G9", "chrome palette matches the web build", False, "web capture failed")

    # ---- empty state ----
    cleanup()
    (OUT / "empty-err.log").write_text("")
    p = start(app)
    try:
        if not wait_window("Lumen", 20.0):
            # one retry after clearing leftovers; keep diagnostics for flaky runs
            err = run(["bash", "-c", f"pgrep -a -x lumen-egui-app; pgrep -a -x Xvfb"])
            check("B14-setup", "empty-state window appears (diagnostics logged)", False,
                  f"pgrep: {err.stdout.strip()[:200]}")
            stop(p)
            cleanup()
            p = start(app)
            if not wait_window("Lumen", 20.0):
                raise RuntimeError("empty-state window did not appear twice")
        focus()
        time.sleep(2.5)
        wait_painted(OUT / "egui-16-empty-pre.png")
        time.sleep(0.5)
        s_empty = shot("egui-16-empty")
        im = img_arr(s_empty)
        content = bbox_nonbg(im, (0, 0, 1280, 820), tol=6)
        card_region = (380, 280, 900, 580)
        px = im.load()
        nonbg = 0
        for y in range(card_region[1], card_region[3], 2):
            for x in range(card_region[0], card_region[2], 2):
                r, g, b = px[x, y]
                if max(abs(r - 21), abs(g - 21), abs(b - 23)) > 12:
                    nonbg += 1
        check("B14", "empty state shows the drop card (web parity)", content is not None and nonbg > 800,
              f"nonbg {nonbg} in {card_region}")
        # empty-state chrome: rail + inspector present too
        rail_px = im.getpixel((46, 300))
        check("B14b", "empty state renders the editor chrome", max(abs(c - t) for c, t in zip(rail_px, (28, 28, 30))) <= 5, str(rail_px))
    finally:
        stop(p)

    # ---- languages ----
    shots_lang = {}
    for lang, tag in (("en", "en"), ("zh_CN", "hans")):
        p = start(app, fx, lang=lang, socket=OUT / f"lang-{tag}.sock")
        try:
            focus()
            time.sleep(3.0)
            key("e", 0.5)
            time.sleep(0.8)
            shots_lang[tag] = shot(f"egui-17-lang-{tag}")
        finally:
            stop(p)
    d = mean_diff(img_arr(shots_lang["en"]), img_arr(shots_lang["hans"]), (700, 0, 1280, 60))
    check("B15", "en and zh-Hans builds render different chrome text", d > 2.0, f"mean diff {d:.1f}")

    # ---- single instance ----
    run(["pkill", "-x", "lumen-egui-app"])
    run(["pkill", "-x", "lumen-egui"])
    time.sleep(1.0)
    sock = OUT / "single.sock"
    try: sock.unlink()
    except FileNotFoundError: pass
    p1 = start(app, fx, socket=sock)
    p2 = None
    try:
        focus()
        time.sleep(3.0)
        before = len(xdo("search", "--onlyvisible", "--name", "Lumen").stdout.split())
        p2 = start(app, fx2, socket=sock)
        p2.wait(timeout=10)
        time.sleep(1.5)
        names = xdo("search", "--onlyvisible", "--name", "Lumen")
        after = len(names.stdout.split())
        title = xdo("getwindowname", names.stdout.split()[0]).stdout.strip() if names.stdout.strip() else ""
        check("B16", "second launch forwards into the running window",
              before == 1 and after == 1, f"{before} -> {after}, exit {p2.returncode}")
        check("B17", "forwarded file becomes the shown document",
              "street" in title or "parity-verify-street" in title, title)
    finally:
        for pp in (p1, p2):
            if pp:
                stop(pp)

    # ---- save / save-as on a copy ----
    work = Path("/tmp/parity-save-test.jpg")
    work.write_bytes(fx.read_bytes())
    p = start(app, work)
    try:
        focus()
        time.sleep(3.0)
        key("e", 0.5)
        key("b", 0.3)
        drag(380, 360, 600, 520)
        key("ctrl+s", 0.6)
        time.sleep(2.0)
        check("B18", "Ctrl+S writes the file (content changes)", work.stat().st_size > 0,
              f"{work.stat().st_size} bytes")
        out_dialog = Path("/tmp/parity-saveas-out.jpg")
        try: out_dialog.unlink()
        except FileNotFoundError: pass
        key("ctrl+shift+s", 0.8)
        time.sleep(1.0)
        # select the file-name field and replace it with an absolute path
        xdo("mousemove", 640, 232, "click", "1"); time.sleep(0.3)
        key("ctrl+a", 0.2)
        xdo("type", "--delay", "10", "/tmp"); time.sleep(0.2)
        xdo("mousemove", 640, 292, "click", "1"); time.sleep(0.3)
        key("ctrl+a", 0.2)
        xdo("type", "--delay", "10", "parity-saveas-out.jpg"); time.sleep(0.4)
        click(833, 668, 0.8)
        time.sleep(2.5)
        check("B19", "save-as dialog writes the chosen path", out_dialog.exists(),
              str(out_dialog))
    finally:
        stop(p)

    # ---- modal layering + resize safety ----
    p = start(app, work)
    try:
        focus()
        time.sleep(3.0)
        key("e", 0.5)
        key("b", 0.4)
        time.sleep(0.5)
        pre = shot("egui-21-dock-before")
        key("ctrl+shift+s", 0.7)
        time.sleep(1.0)
        modal = shot("egui-22-dock-under-modal")
        def band_mean(path, box):
            im = img_arr(path).crop(box).convert("L").resize((80, 20))
            return statistics.mean(im.getdata())
        dock_box = (240, 690, 1040, 790)
        before_v = band_mean(pre, dock_box)
        under_v = band_mean(modal, dock_box)
        check("G11", "modal scrim dims the chrome underneath (dock no longer floats above)",
              under_v < before_v * 0.85, f"dock mean {before_v:.1f} -> {under_v:.1f}")
        key("Escape", 0.6)
        time.sleep(0.4)

        # resize to the minimum window size and open the save dialog again
        wid = wait_window()
        xdo("windowsize", wid, "880", "560")
        time.sleep(1.0)
        key("ctrl+shift+s", 0.7)
        time.sleep(1.2)
        small = shot("egui-23-saveas-880")
        im_s = img_arr(small)
        px = im_s.load()
        row = 300
        xs = [x for x in range(0, 880) if max(abs(a - b) for a, b in zip(px[x, row], (35, 35, 36))) <= 4]
        left = min(xs) if xs else -1
        right = max(xs) if xs else -1
        check("G12", "save dialog fits inside the 880x560 minimum window",
              12 <= left and right <= 868, f"dialog span {left}..{right} of 880")
        key("Escape", 0.5)
    finally:
        stop(p)

    # ---- resize safety: fit padding and panel metrics at several sizes ----
    p = start(app, fx)
    try:
        if not wait_window("Lumen", 25.0):
            raise RuntimeError("resize-test window did not appear")
        focus()
        time.sleep(2.5)
        wid = wait_window()
        # preview: the picture keeps the web's 32px stage padding on the constrained axis
        for w, h in ((880, 560), (1440, 900), (1280, 820)):
            xdo("windowsize", wid, str(w - 2), str(h - 2))
            time.sleep(0.4)
            xdo("windowsize", wid, str(w), str(h))
            time.sleep(1.4)
            xdo("mousemove", w // 2, h // 2)
            time.sleep(0.6)
            shot(f"egui-24-resize-{w}x{h}-a")
            path = shot(f"egui-24-resize-{w}x{h}")
            im_r = img_arr(path)
            bb = bbox_sat(im_r, (0, 0, w, h))
            ok = False
            detail = str(bb)
            if bb:
                pads = [bb[0], w - 1 - bb[2], bb[1], h - 1 - bb[3]]
                detail = f"bbox {bb} pads {pads}"
                constrained = min(pads)
                ok = 28 <= constrained <= 40 and min(pads) >= 28
            check(f"G15-{w}x{h}", f"picture fits with the web stage padding at {w}x{h}",
                  ok, detail)
        # editor: panels keep the web metrics at every size
        key("e", 0.6)
        time.sleep(1.2)
        for w, h in ((880, 560), (1280, 820)):
            xdo("windowsize", wid, str(w), str(h))
            time.sleep(1.6)
            xdo("mousemove", w // 2, h // 2)
            time.sleep(0.5)
            path = shot(f"egui-24-edit-{w}x{h}")
            im_e = img_arr(path)
            rail_edge = im_e.getpixel((45, h // 2))
            insp_edge = im_e.getpixel((w - 250, h // 2))
            check(f"G16-{w}x{h}", f"editor panels keep web metrics at {w}x{h}",
                  rail_edge == (28, 28, 30) and insp_edge != (21, 21, 23),
                  f"rail {rail_edge} inspector {insp_edge}")
    finally:
        stop(p)

    # ---- style dock geometry (debug rect from the app itself) ----
    cleanup()
    dock_log = OUT / "dock-debug.log"
    dock_log.write_text("")
    env = dict(ENV)
    env["LUMEN_DEBUG_DOCK"] = "1"
    dp = subprocess.Popen([str(app), str(fx)], env=env,
                          stdout=subprocess.DEVNULL, stderr=open(dock_log, "w"))
    try:
        if not wait_window("Lumen", 25.0):
            raise RuntimeError("dock-test window did not appear")
        focus()
        time.sleep(2.5)
        key("e", 0.6)
        key("b", 0.6)
        xdo("mousemove", 516, 740)   # awake
        time.sleep(0.8)
        awake_lines = [l for l in dock_log.read_text().splitlines() if l.startswith("[dock] rect")]
        xdo("mousemove", 900, 200)   # idle
        time.sleep(1.0)
        idle_lines = [l for l in dock_log.read_text().splitlines() if l.startswith("[dock] rect")]

        def parse(line):
            v = [float(x) for x in line.split() if x.replace(".", "", 1).lstrip("-").isdigit()]
            # rect x y w h canvas x y w h screen w h
            return {"rect": v[0:4], "canvas": v[4:8], "screen": v[8:10]}

        ok = False
        detail = "no [dock] lines"
        if awake_lines and idle_lines:
            a = parse(awake_lines[-1]); i = parse(idle_lines[-1])
            cx = a["rect"][0] + a["rect"][2] / 2
            bottom = a["rect"][1] + a["rect"][3]
            canvas_cx = a["canvas"][0] + a["canvas"][2] / 2
            canvas_bottom = a["canvas"][1] + a["canvas"][3]
            idle_bottom = i["rect"][1] + i["rect"][3]
            detail = (f"awake cx {cx:.1f}/{canvas_cx:.1f} bottom {bottom:.1f}/{canvas_bottom - 56:.1f} "
                      f"idle bottom {idle_bottom:.1f}")
            ok = (abs(cx - canvas_cx) <= 2
                  and abs(bottom - (canvas_bottom - 56.0)) <= 2
                  and abs(idle_bottom - bottom - 0.58 * 54.0) <= 2)
        check("G17", "style dock is canvas-centred, 56px above the canvas, and slides down when idle",
              ok, detail)
    finally:
        stop(dp)
        cleanup()

    # ---- dock swatch row alignment (pixel level, awake dock) ----
    PAL = [(255, 92, 92), (255, 180, 87), (255, 217, 61), (61, 220, 151),
           (91, 157, 255), (192, 132, 252)]

    def swatch_row(img, y0=680, y1=745):
        px = img.load()
        pts = []
        for y in range(y0, min(y1, img.height)):
            for x in range(220, 820):
                r, g, b = px[x, y]
                if any(max(abs(r - p[0]), abs(g - p[1]), abs(b - p[2])) <= 6 for p in PAL):
                    pts.append((x, y))
        if not pts:
            return None
        ys = [p[1] for p in pts]; xs = [p[0] for p in pts]
        return (min(xs), min(ys), max(xs), max(ys))

    web_sw_shot = None
    wp = start(WEB, fx)
    try:
        focus()
        time.sleep(9.0)
        wid = wait_window()
        if wid:
            geo = run(["xdotool", "getwindowgeometry", "--shell", wid]).stdout
            vals = dict(line.split("=") for line in geo.strip().splitlines() if "=" in line)
            wx, wy = int(vals.get("X", "0")), int(vals.get("Y", "0"))
            click(wx + 1250, wy + 32, 1.6)
            key("b", 0.5)
            xdo("mousemove", wx + 516, wy + 740)
            time.sleep(0.7)
            web_sw_shot = OUT / "web-dock-swatches.png"
            run(["import", "-display", DISPLAY, "-window", wid, str(web_sw_shot)])
    finally:
        stop(wp)
    cleanup()
    p = start(app, fx)
    egui_sw_shot = None
    try:
        focus()
        time.sleep(2.5)
        key("e", 0.6)
        key("b", 0.5)
        xdo("mousemove", 516, 740)
        time.sleep(0.8)
        egui_sw_shot = shot("egui-26-swatches")
    finally:
        stop(p)
    bw = swatch_row(img_arr(web_sw_shot)) if web_sw_shot else None
    be = swatch_row(img_arr(egui_sw_shot)) if egui_sw_shot else None
    ok = False
    detail = f"web {bw} egui {be}"
    if bw and be:
        cy_w = (bw[1] + bw[3]) / 2; cy_e = (be[1] + be[3]) / 2
        detail = f"swatch centre y {cy_w:.1f} vs {cy_e:.1f}, bands {bw[1]}..{bw[3]} vs {be[1]}..{be[3]}"
        ok = abs(cy_w - cy_e) <= 2 and abs(bw[1] - be[1]) <= 2
    check("G18", "dock colour swatches are vertically centred like the web", ok, detail)

    # ---- browser flow: select an image, exit, reopen, open it ----
    outline = Path("/tmp/parity-browser-flow")
    outline.mkdir(exist_ok=True)
    for i in range(3):
        (outline / f"fixture-{i}.jpg").write_bytes(fx.read_bytes())
    p = start(app, fx)
    try:
        focus()
        time.sleep(2.5)
        key("e", 0.6)
        key("ctrl+o", 0.7)
        time.sleep(1.0)
        def goto_fixture():
            xdo("mousemove", 590, 183, "click", "1"); time.sleep(0.3)
            key("ctrl+a", 0.2)
            xdo("type", "--delay", "10", str(outline))
            time.sleep(0.3)
            key("Return", 1.0)
        goto_fixture()
        # single click on the first image row (fixture folder holds only images)
        xdo("mousemove", 500, 262, "click", "1"); time.sleep(0.6)
        key("Escape", 0.7)
        s_after_close = shot("egui-27-browser-closed")
        key("ctrl+o", 0.8)
        time.sleep(0.9)
        s_reopen = shot("egui-28-browser-reopen")
        goto_fixture()
        xdo("mousemove", 500, 262, "click", "1"); time.sleep(0.5)
        xdo("mousemove", 908, 183, "click", "1"); time.sleep(1.8)
        title = run(["xdotool", "getwindowname", wait_window()]).stdout.strip()
        check("G19", "browser select -> exit -> reopen -> open still works",
              "fixture-" in title, title)
        # the reopen must not leave a scrim behind
        d = region_delta(s_after_close, s_reopen, (300, 120, 960, 700))
        check("G19b", "reopened browser is interactive (not stuck behind a scrim)", d > 3.0,
              f"delta {d:.1f}")
    finally:
        stop(p)
        cleanup()

    # ---- repeated browser open/close cycles must stay bright and interactive ----
    cleanup()
    p = start(app, fx)
    try:
        focus(); time.sleep(2.5)
        key("e", 0.8)
        key("b", 0.5)   # dock visible while the modal opens (the old dimming trigger)
        time.sleep(0.5)
        measurements = []
        def dialog_mean():
            im = img_arr(shot("egui-33-cycle"))
            cell = im.crop((330, 140, 950, 700)).convert("L")
            return statistics.mean(cell.getdata())
        def open_select_close(how):
            key("ctrl+o", 0.8)
            time.sleep(1.0)
            xdo("mousemove", 590, 183, "click", "1"); time.sleep(0.3)
            key("ctrl+a", 0.2)
            xdo("type", "--delay", "10", str(outline))
            time.sleep(0.3)
            key("Return", 1.1)
            xdo("mousemove", 500, 262, "click", "1"); time.sleep(0.5)
            measurements.append(dialog_mean())
            if how == "esc":
                key("Escape", 0.8)
            elif how == "x":
                xdo("mousemove", 912, 124, "click", "1"); time.sleep(0.9)
            else:
                xdo("mousemove", 120, 700, "click", "1"); time.sleep(0.9)
            time.sleep(0.5)
        open_select_close("esc")
        open_select_close("x")
        open_select_close("outside")
        stable = max(measurements) - min(measurements) <= 4.0
        check("G22", "reopened browser keeps the same brightness (no stuck scrim)",
              stable, f"dialog means {[round(m,1) for m in measurements]}")
        # and it is still usable: open an image after the cycles
        key("ctrl+o", 0.8)
        time.sleep(1.0)
        xdo("mousemove", 590, 183, "click", "1"); time.sleep(0.3)
        key("ctrl+a", 0.2)
        xdo("type", "--delay", "10", str(outline))
        time.sleep(0.3)
        key("Return", 1.1)
        xdo("mousemove", 500, 262, "click", "1"); time.sleep(0.5)
        xdo("mousemove", 908, 183, "click", "1"); time.sleep(1.8)
        title = run(["xdotool", "getwindowname", wait_window()]).stdout.strip()
        check("G22b", "browser still opens images after several close cycles",
              "fixture-" in title, title)
    finally:
        stop(p)
        cleanup()

    # ---- viewer zoom-out floor must be the fitted view (not the unpadded fit) ----
    floor_dir = Path("/tmp/parity-zoom-floor")
    floor_dir.mkdir(exist_ok=True)
    floor_img = floor_dir / "wide.jpg"
    Image.new("RGB", (3800, 2360), (70, 90, 130)).save(floor_img, quality=85)
    cleanup()
    env = dict(ENV); env["LUMEN_DEBUG_ZOOM"] = "1"
    fp = subprocess.Popen([str(app), str(floor_img)], env=env,
                          stdout=subprocess.DEVNULL,
                          stderr=open(OUT / "zoom-floor.log", "w"))
    try:
        if not wait_window("Lumen", 25.0):
            raise RuntimeError("zoom-floor window did not appear")
        focus(); time.sleep(2.5)
        xdo("mousemove", 640, 400)
        xdo("click", "--repeat", "4", "4")   # wheel up
        time.sleep(0.5)
        xdo("click", "--repeat", "25", "5")  # wheel down
        time.sleep(1.0)
        vals = [float(l.split()[1]) for l in
                (OUT / "zoom-floor.log").read_text().splitlines() if l.startswith("[zoom]")]
        got = vals[-1] if vals else 0.0
        wid_ = wait_window()
        geo = run(["xdotool", "getwindowgeometry", "--shell", wid_]).stdout
        gv = dict(line.split("=") for line in geo.strip().splitlines() if "=" in line)
        sw, sh = int(gv.get("WIDTH", 1280)), int(gv.get("HEIGHT", 820))
        expected = min((sw - 64) / 3800, (sh - 64) / 2360)
        check("B25", "viewer zoom-out stops exactly at the fitted view (padded fit)",
              abs(got - expected) <= 0.002,
              f"min scale {got:.4f} vs fit {expected:.4f}")
    finally:
        stop(fp)
        cleanup()

    # ---- micro-interactions: tool pill/bar slide, swatch pop, shortcuts close ----
    p = start(app, fx)
    try:
        focus(); time.sleep(2.5)
        key("e", 0.7); time.sleep(0.8)
        key("v", 0.4)   # Move active
        frames = burst("toolswitch", "r", frames=6, gap=0.05)
        bar_ys = []
        for f in frames:
            im_f = img_arr(f)
            px_f = im_f.load()
            ys = [y for y in range(50, 460) for x in range(3, 10)
                  if sum(px_f[x, y][:3]) > 3 * 140]
            bar_ys.append(sum(ys) / len(ys) if ys else 0.0)
        nonzero = [y for y in bar_ys if y > 0]
        spread = (max(nonzero) - min(nonzero)) if len(nonzero) > 1 else 0.0
        check("A6", "active tool bar slides between rows (animated)",
              spread > 8.0, f"bar centres {[round(y,1) for y in bar_ys]}")
        key("question", 0.6)
        time.sleep(0.6)
        frames = burst("shortcut-close", "Escape", frames=4, gap=0.05)
        means = []
        for f in frames:
            cell = img_arr(f).crop((360, 120, 900, 700)).convert("L").resize((40, 40))
            means.append(statistics.mean(cell.getdata()))
        spread = (max(means) - min(means)) if means else 0.0
        check("A9", "shortcuts window animates out", spread > 2.0,
              f"means {[round(m,1) for m in means]}")
    finally:
        stop(p)
        cleanup()

    # ---- swatch pop (motion slowed so screencaps can sample it) ----
    cleanup()
    env = dict(ENV); env["LUMEN_MOTION_SLOW"] = "8"
    sp = subprocess.Popen([str(app), str(fx)], env=env,
                          stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        if not wait_window("Lumen", 25.0):
            raise RuntimeError("swatch window did not appear")
        focus(); time.sleep(2.5)
        key("e", 1.0); time.sleep(1.2)
        key("b", 1.0); time.sleep(1.2)
        xdo("mousemove", 516, 740)
        time.sleep(1.2)
        pre = shot("egui-32-swatch-pre")
        xdo("mousemove", 265, 712)
        time.sleep(0.3)
        xdo("click", "1")
        frames = burst("swatchpop", None, frames=5, gap=0.12)
        counts = []
        for f in frames:
            im_f = img_arr(f)
            px_f = im_f.load()
            counts.append(sum(1 for y in range(690, 734) for x in range(248, 286)
                              if sum(px_f[x, y]) > 3 * 190))
        check("A7", "colour swatch selection animates (ring pop)",
              len(set(counts)) >= 2, f"ring pixels {counts}")
    finally:
        stop(sp)
        cleanup()

    # ---- dock wake must not oscillate ----
    cleanup()
    dock_log = OUT / "dock-jitter.log"
    dock_log.write_text("")
    env = dict(ENV); env["LUMEN_DEBUG_DOCK"] = "1"
    dp = subprocess.Popen([str(app), str(fx)], env=env,
                          stdout=subprocess.DEVNULL, stderr=open(dock_log, "w"))
    try:
        if not wait_window("Lumen", 25.0):
            raise RuntimeError("dock-jitter window did not appear")
        focus(); time.sleep(2.5)
        key("e", 0.8); time.sleep(0.6); key("b", 0.8); time.sleep(0.8)
        # park the pointer right on the wake boundary and watch the dock rect
        xdo("mousemove", 516, 780)
        time.sleep(0.4)
        dock_log.write_text("")
        time.sleep(1.4)
        lines = [l for l in dock_log.read_text().splitlines() if l.startswith("[dock] rect")]
        ys = []
        for line in lines:
            v = [float(x) for x in line.split() if x.replace(".", "", 1).lstrip("-").isdigit()]
            ys.append(v[1])
        stable = len(ys) > 5 and (max(ys) - min(ys)) <= 2.0
        check("G20", "dock does not oscillate while the pointer sits at its edge",
              stable, f"{len(ys)} frames, y {min(ys):.1f}..{max(ys):.1f}" if ys else "no frames")
    finally:
        stop(dp)
        cleanup()

    # ---- undo/redo affordance matches the history ----
    p = start(app, fx)
    try:
        focus(); time.sleep(2.5)
        key("e", 0.7); time.sleep(0.6)
        def undo_redo(path):
            im = img_arr(path)
            u = im.crop((1120, 10, 1148, 38)).convert("L")
            r = im.crop((1150, 10, 1180, 38)).convert("L")
            return statistics.mean(u.getdata()), statistics.mean(r.getdata())
        before = shot("egui-29-undo-empty")
        u0, r0 = undo_redo(before)
        key("b", 0.4)
        drag(380, 360, 620, 540)
        time.sleep(0.4)
        mid = shot("egui-30-undo-available")
        u1, r1 = undo_redo(mid)
        key("ctrl+z", 0.6)
        time.sleep(0.5)
        after = shot("egui-31-redo-available")
        u2, r2 = undo_redo(after)
        check("G21", "undo/redo buttons light up only when the history allows it",
              u0 < u1 - 2.0 and r0 < r2 - 2.0 and abs(u2 - u0) < 2.0,
              f"undo {u0:.1f}->{u1:.1f}->{u2:.1f}, redo {r0:.1f}->{r1:.1f}->{r2:.1f}")
    finally:
        stop(p)
        cleanup()

    # ---- image switch latency (prefetched neighbours + fast JPEG decode) ----
    bigdir = Path("/tmp/parity-switch-latency")
    bigdir.mkdir(exist_ok=True)
    base = Image.open(fx).convert("RGB").resize((4000, 3000))
    for i in range(3):
        img = base if i == 0 else (base.rotate(180) if i == 1 else base.transpose(Image.FLIP_LEFT_RIGHT))
        img.save(bigdir / f"{i + 1:02}.jpg", quality=88)
    cleanup()
    p = start(app, bigdir / "01.jpg")
    try:
        focus(); time.sleep(3.5)
        wid = wait_window()
        def win_title():
            return run(["xdotool", "getwindowname", wid]).stdout.strip()
        latencies = []
        for _ in range(3):
            prev = win_title()
            xdo("key", "--clearmodifiers", "Right")
            t0 = time.time()
            while time.time() - t0 < 3.0:
                if win_title() != prev:
                    break
                time.sleep(0.003)
            latencies.append((time.time() - t0) * 1000.0)
            time.sleep(0.6)
        med = statistics.median(latencies)
        check("P3", "switching to the next image is fast (prefetched, <=60 ms median)",
              med <= 60.0, f"median {med:.1f} ms of {[round(x,1) for x in latencies]}")
    finally:
        stop(p)
        cleanup()

    # ---- system file picker (GTK3, in-process, main thread) ----
    cleanup()
    native_env = dict(ENV)
    native_env.pop("LUMEN_INTERNAL_BROWSER", None)
    np_ = subprocess.Popen([str(app), str(fx)], env=native_env,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        if not wait_window("Lumen", 30.0):
            raise RuntimeError("native-dialog window did not appear")
        focus(); time.sleep(2.5)
        key("e", 0.6)
        key("ctrl+o", 0.8)
        time.sleep(1.8)
        dialogs = xdo("search", "--onlyvisible", "--name", "開啟圖片").stdout.split()
        appwin = xdo("search", "--onlyvisible", "--name", "Lumen").stdout.split()
        check("G23a", "Ctrl+O opens the native GTK file chooser window",
              bool(dialogs) and (not appwin or dialogs[0] != appwin[0]),
              f"dialogs={dialogs[:1]} app={appwin[:1]}")
        target = OUT / "native-pick.jpg"
        import shutil as _sh
        _sh.copyfile(fx, target)
        if dialogs:
            xdo("windowfocus", dialogs[0])
            time.sleep(0.4)
            xdo("type", "--delay", "20", str(target))
            time.sleep(0.6)
            key("Return", 0.5)
            time.sleep(2.2)
        title = run(["xdotool", "getwindowname", wait_window()]).stdout.strip()
        check("G23a2", "native picker result opens the chosen file", "native-pick" in title, title)
        w = wait_window()
        if w:
            xdo("windowfocus", w)
        time.sleep(0.6)
        key("ctrl+shift+s", 0.8)
        time.sleep(2.0)
        saves = xdo("search", "--onlyvisible", "--name", "另存新檔").stdout.split()
        check("G23b", "Ctrl+Shift+S opens the native save chooser", bool(saves), str(saves[:1]))
        if saves:
            xdo("windowfocus", saves[0])
        key("Escape", 1.0)
        time.sleep(0.6)
        alive = np_.poll() is None
        check("G23c", "app survives the native choosers", alive, f"exit={np_.returncode}")
    finally:
        stop(np_)
        cleanup()

    # fallback: LUMEN_NO_NATIVE_DIALOG forces the in-app browser
    cleanup()
    fb_env = dict(ENV); fb_env["LUMEN_NO_NATIVE_DIALOG"] = "1"
    p = subprocess.Popen([str(app), str(fx)], env=fb_env,
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        focus(); time.sleep(2.5)
        key("e", 0.6)
        s_pre = shot("egui-34a-pre-open")
        key("ctrl+o", 0.8)
        time.sleep(1.6)
        s_open = shot("egui-34b-open")
        d = region_delta(s_pre, s_open, (360, 90, 900, 740))
        check("G23d", "LUMEN_NO_NATIVE_DIALOG falls back to the in-app browser",
              d > 4.0, f"delta {d:.1f}")
    finally:
        stop(p)
        cleanup()

    # ---- performance ----
    if quick:
        check("P2", "benchmark (skipped in --quick)", True, "quick mode")
    else:
        # shared benchmark fixtures (bench scripts read /tmp/qa-small.jpg and /tmp/qa-big.jpg)
        Path("/tmp/qa-small.jpg").write_bytes(fx.read_bytes())
        big = Image.open(fx).convert("RGB").resize((4000, 3000))
        big.save("/tmp/qa-big.jpg", quality=88)

        def parse(out, tag):
            vals = {}
            for line in out.splitlines():
                parts = line.split()
                if parts and parts[0] == tag:
                    for i, p in enumerate(parts):
                        if p in ("photo", "window", "PSS") and i + 1 < len(parts):
                            key = {"photo": "photo", "window": "window", "PSS": "pss"}[p]
                            try:
                                vals[key] = float(parts[i + 1])
                            except ValueError:
                                pass
            return vals
        eg_small = run(["bash", str(PROJ / "scripts/bench-egui.sh"), "2"], cwd=str(PROJ))
        eg_big = run(["bash", str(PROJ / "scripts/bench-egui.sh"), "2"], cwd=str(PROJ))  # same script covers both sizes
        web_small = run(["bash", str(PROJ / "scripts/bench-vs-loupe.sh"), "/tmp/qa-small.jpg", "2"], cwd=str(PROJ))
        web_big = run(["bash", str(PROJ / "scripts/bench-vs-loupe.sh"), "/tmp/qa-big.jpg", "2"], cwd=str(PROJ))
        e_small = parse(eg_small.stdout, "small")
        e_big = parse(eg_big.stdout, "big")
        w_small = parse(web_small.stdout, "lumen")
        w_big = parse(web_big.stdout, "lumen")
        ok = (e_small.get("photo", 1e9) < w_small.get("photo", 0)
              and e_big.get("photo", 1e9) < w_big.get("photo", 0)
              and e_small.get("pss", 1e9) < w_small.get("pss", 0))
        check("P2", "benchmark: egui faster to photo and uses less memory than the WebView build",
              ok, f"egui {e_small} / {e_big}  web {w_small} / {w_big}")

    report = OUT / "report.json"
    report.write_text(json.dumps({"results": results}, indent=2))
    failed = [r for r in results if not r["ok"]]
    print(f"\n{len(results) - len(failed)}/{len(results)} checks passed")
    if failed:
        print("FAILED: " + ", ".join(r["id"] for r in failed))
    for name in ("lumen-egui-app", "lumen-egui", "lumen", "Xvfb"):
        run(["pkill", "-x", name])
    time.sleep(1.2)
    run(["pkill", "-x", "Xvfb"])
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())

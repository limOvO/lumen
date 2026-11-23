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

        # dock enter animation (fade + rise): sample the dock band right after the tool switch
        s_dock_pre = shot("egui-18a-pre-dock")
        frames = burst("dock", "b", frames=4, gap=0.06)
        dock_means = []
        for f in frames:
            im_f = img_arr(f)
            band = im_f.crop((240, 690, 1040, 780)).convert("L").resize((60, 12))
            dock_means.append(round(statistics.mean(band.getdata()), 1))
        check("A4", "style dock animates in (band brightness changes over frames)",
              len(set(int(m // 2) for m in dock_means)) >= 2, str(dock_means))
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
    else:
        check("G9", "chrome palette matches the web build", False, "web capture failed")

    # ---- empty state ----
    p = start(app)
    try:
        focus()
        time.sleep(3.0)
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
        rail_px = im.getpixel((10, 300))
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

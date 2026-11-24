# Lumen

**English** · [繁體中文](README.zh-TW.md)

A lightweight image viewer for Linux that also annotates.
One 4 MB binary, no runtime, window up in ~80 ms — a drop-in replacement for the
preinstalled GNOME image viewer (Loupe).

![Lumen demo: preview → next image → editor → annotations](docs/showcase/demo.gif)

| Preview | Edit |
|---|---|
| ![Preview](docs/showcase/preview.png) | ![Edit](docs/showcase/edit.png) |

## Why Lumen

| | **Lumen** | **Image Viewer** (GNOME, Loupe 51.2) |
|---|---|---|
| **Built with** | Tauri 2 (Rust + system WebView), vanilla HTML/CSS | GTK4 / libadwaita (Rust) |
| **App binary** | **4.0 MB** single ELF | 6.8 MB (package 8.1 MiB) |
| **Window appears** | **81 ms** (1400×934) · **86 ms** (12 MP) | 146 ms · 148 ms |
| **Photo visible** | 447 ms · 469 ms | **212 ms** · **339 ms** |
| **Memory while viewing** (PSS) | 263 MB · 269 MB | **113 MB** · **137 MB** |
| **Runtime dependencies** | webkit2gtk-4.1 (141 MiB) + GTK3 (54 MiB) | GTK4 (57 MiB) + libadwaita (5.4) + glycin (19.6) |
| **What it does** | view, **crop**, **resize**, **compress to a target KB**, **annotate** | view, **crop**, rotate, flip |
| **Annotations** | brush, line, rectangle, ellipse, two-point arrow, text — colour, width, opacity, dashed | — |
| **Crop** | ✅ free crop + ratio presets, Shift-lock, non-destructive until save | ✅ ratio presets (任意/1:1/4:3/3:2/16:9/5:4), rotate & mirror, applied on save |
| **Resize / compress to a target KB** | ✅ long-edge limit, target-KB quality search | — |
| **EXIF / GPS panel** | ✅ full EXIF (camera, lens, exposure, GPS) | ✅ properties panel (dimensions, format, size, dates, ISO, GPS location) |
| **Folder browsing** | ✅ built-in folder browser, ← → through the folder | ‹ › between neighbouring images, no folder browser |
| **Undo** | 80-step history, edits stay non-destructive | cancel / reset inside the edit view (no history) |
| **Privacy on save** | EXIF/GPS stripped from the saved file | overwrites the original in place, no stripping option |
| **Languages** | Traditional Chinese, Simplified Chinese, English | all GNOME languages |

“Image Viewer” is what GNOME’s app menu calls Loupe (`org.gnome.Loupe.desktop`, `/usr/bin/loupe`) —
the preinstalled viewer on this machine.

Measured on CachyOS, 13th Gen Intel i9-13900F, Xvfb 1440×900 with software rendering
(`GSK_RENDERER=cairo`, `WEBKIT_DISABLE_COMPOSITING_MODE=1`), median of 4 runs;
“photo visible” = first captured frame (60 fps) showing the picture. On a GPU desktop both
apps start faster; the table compares like-for-like conditions.
Reproduce with [`scripts/bench-vs-loupe.sh`](scripts/bench-vs-loupe.sh) (Xvfb + Loupe installed).

Why the gap, and what was done about it: Loupe is a single GTK4 process with a native
decode/texture path; Lumen carries WebKit's process model (WebProcess ≈ 150 MB, NetworkProcess
≈ 33 MB, shell ≈ 98 MB) plus a JS boot. Measured improvements: the preview decodes at ~2× the
viewport (a full-resolution bitmap is swapped in when you zoom past it or export), the extra full
decode was removed, and the splash no longer fades when opening from the file manager — that took
time-to-photo from 490→447 ms (1400×934) and 508→469 ms (12 MP), and memory from 310→269 MB
on 12 MP.


Native architecture spikes live in [`spikes/`](spikes/README.md): a GTK3 viewer measures
**24 MB / 54 ms** for a 1.3 MP photo (12 MP: 95 MB / 104 ms), i.e. Loupe-class footprint without
the WebView — see [`docs/architecture.md`](docs/architecture.md) for the trade-offs of a migration.

**Honest summary:** Loupe crops, rotates and flips too — and it is faster to get the photo on
screen and uses less RAM. Lumen is a **4 MB single binary** that opens its window twice as fast,
and adds what Loupe does not do: freehand/shape/text **annotations**, **resize**, **compress to a
target KB**, an **80-step undo** and **EXIF/GPS stripping** on save.

## Install

**Download the binary** (Linux x86_64):

```bash
curl -L -o lumen https://github.com/limOvO/lumen/releases/latest/download/lumen
chmod +x lumen && ./lumen photo.jpg
```

Requires `gtk3` and `webkit2gtk-4.1` (Arch: `gtk3`, `webkit2gtk-4.1`).

**Build from source** (Rust 1.80+):

```bash
./scripts/build.sh                       # → dist/lumen
./scripts/install.sh                     # install to ~/.local (no sudo)
./scripts/install.sh --desktop-shortcut  # + desktop shortcut
./scripts/install.sh --set-default       # make it the default image viewer
./scripts/uninstall.sh                   # remove (only touches ~/.local)
```

Installs `~/.local/bin/lumen`, an app-menu entry and the icon set. More in
[packaging/INSTALL.md](packaging/INSTALL.md).

## Use

```bash
lumen                 # empty window: Ctrl+O or drag & drop
lumen photo.jpg       # open one image
lumen ~/Pictures      # open a folder
```

Lumen has two modes. It opens in **Preview** (clean picture, zoom, ← → between images,
`I` for the info card). Press **Edit** in the top-right corner — or `E` — to get the tool
rail, panels and annotation style bar; `Esc` goes back to Preview.

| Key | Action | Key | Action |
|---|---|---|---|
| `Ctrl+O` / `Ctrl+S` / `Ctrl+Shift+S` | open / save / save as | `Ctrl+Z` / `Ctrl+Shift+Z` | undo / redo |
| `←` `→` | previous / next image | `Home` `End` | first / last image |
| `+` `-` / `0` | zoom in / out / fit | wheel, `Ctrl+wheel` | zoom at cursor |
| `V` `B` `R` `O` `A` `L` `T` `C` | move / brush / rectangle / ellipse / arrow / line / text / crop | `Delete` | delete selected annotation |
| `Space` (hold) or middle-drag | pan canvas | `Esc` | deselect / back to Preview |
| `I` | info card | `F` / `?` | fullscreen / shortcut list |

**Formats:** JPEG, PNG, WebP, GIF (first frame), BMP, TIFF, AVIF, SVG, ICO.
Output: JPEG (white background), PNG, WebP.

**Language:** follows the OS — Traditional Chinese, Simplified Chinese, English for
everything else. Force it with `LUMEN_LANG=zh-Hans` (or `zh-Hant`, `en`).

## Rust + wgpu + egui build (parity-complete)

A second build of Lumen is being ported to **Rust + wgpu + egui** (no WebView) to reach
Loupe-class footprint with GPU rendering:

```bash
scripts/build-egui.sh            # → dist/lumen-egui (~47 MB with full CJK fallback faces)
scripts/install-egui.sh          # install to ~/.local (add --desktop-shortcut / --set-default)
WGPU_BACKEND=gl ./dist/lumen-egui photo.jpg    # GL fallback for headless/software GL
```

Parity-complete: full viewer + editor chrome with the web build's motion timings (rail/
inspector slide, dock rise, peek card, image switch, phase glide), annotation toolset
(brush/line/rect/ellipse/arrow/text, dashed, weight-600 text), crop with ratio presets,
export with JPEG/lossy-WebP target-KB compression, save/save-as, in-app folder browser with
thumbnails, single-instance forwarding, 80-step undo, three languages.

Verified by `python3 scripts/parity/verify.py`: **62 automated checks** (geometry, palette
delta 0 against a live WebView capture, chrome regions, all tools, undo/redo, compare, crop,
zoom/fit, animations, idle CPU, languages, single-instance, save/save-as, benchmark) plus
**121 unit tests**. Matrix: [docs/parity.md](docs/parity.md); benchmarks:
[docs/architecture.md](docs/architecture.md).

## License

MIT.

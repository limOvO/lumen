# Architecture: where the WebView cost comes from, and what a native shell buys

Current Lumen is a Tauri 2 app: a Rust shell plus a WebKitGTK WebView that runs the whole UI.
That is great for iterating on the interface, expensive at runtime:

- process tree: `lumen` 98 MB + `WebKitWebProcess` ~150 MB + `WebKitNetworkProcess` 33 MB
- time-to-photo on a 1.3 MP JPEG: ~447 ms (WebView boot ≈ 300 ms, JS boot ≈ 80 ms)
- memory is dominated by WebKit's per-process baseline, not by the image itself

All numbers below were measured on CachyOS / i9-13900F / Xvfb 1440×900 / software rendering,
median of 4 runs. "photo" = first frame in a 60 fps capture that shows the picture.

| implementation | window | photo (1.3 MP) | photo (12 MP) | PSS (1.3 MP) | PSS (12 MP) |
|---|---|---|---|---|---|
| current Lumen (Tauri + WebKit) | 81 ms | 447 ms | 469 ms | 263 MB | 269 MB |
| `spikes/gtk3-viewer` (GTK3 + cairo) | 54 ms | **54 ms** | **104 ms** | **24 MB** | **95 MB** |
| `spikes/egui-viewer` (wgpu + egui) | 71 ms | 71 ms | 129 ms | 116 MB | 155 MB |
| **`egui-app` (rewrite, full UI)** | 79 ms | **79 ms** | 237 ms | 125 MB | 148 MB |
| Loupe 51.2 (GTK4 / libadwaita) | 146 ms | 212 ms | 339 ms | 113 MB | 137 MB |

## Note on the egui numbers

The headless session has no Vulkan device, so every wgpu run above used the GL backend on
llvmpipe. ~57 MB of the egui app's PSS is `libLLVM` + `libgallium` + `libnvidia-gpucomp`,
i.e. *software rasteriser* costs a real GPU session does not pay; its own heap is ~35 MB
(390-glyph subset font, textures, egui state). On a GPU session it should land at or below
Loupe's 113 MB while keeping the sub-100 ms time-to-photo.

The rewrite now has full chrome (preview chips/zoom HUD/peek card, editor top bar, rail,
資訊/調整/裁切 inspector, style dock, shortcuts window), the annotation toolset
(brush/line/rect/ellipse/arrow/text + dashed), crop with ratio presets, selection with
two-point handles, 80-step undo, and a full-resolution export pipeline with target-KB
compression. The rewrite now also has: lossy WebP with target-KB bisection, weight-600 annotation text
on screen and in exports, folder-browser thumbnails matching the CSS row geometry,
single-instance forwarding (second launch updates the running window), and packaging
(scripts/build-egui.sh / install-egui.sh + desktop entry). Parity gate:
`scripts/parity/check.sh` (12 paired captures + geometry) and 118 unit tests.

## What the spikes show

- **GTK3 + cairo** is in a different class: 24 MB for a normal photo, 95 MB for 12 MP, and the
  window only appears once the picture is ready (54–104 ms). This is the Loupe architecture
  (GTK4 instead of GTK3), and it also beats Loupe in this environment because Loupe carries
  libadwaita + glycin + a heavier toolkit baseline.
- **wgpu + egui** starts fast and renders on the GPU, but a full-resolution RGBA upload plus the
  egui font atlas and winit/wgpu state land at 116–155 MB here — still ~2× better than the WebView,
  but ~4–6× heavier than GTK3. A tiled/texture-atlas upload (upload only what is on screen) and a
  preview-size texture would cut most of the 12 MP difference. A CJK font has to be embedded for
  the Chinese UI (adds ~10–30 MB resident).
- The headless session has no Vulkan device, so the egui numbers used the GL backend on llvmpipe;
  on a real GPU the frame costs change, the memory profile is roughly the same.

## Options

1. **Native GTK4 shell (recommended if footprint is the goal).** Rust + GTK4 + cairo for the
   canvas; rebuild the panels as GTK widgets, keep the state/annotation model in Rust.
   Expected: ~24–60 MB typical, ~100 ms to photo, no WebView processes at all.
   Cost: full UI rewrite (the current CSS design cannot be reused), IME/accessibility handled by GTK.
2. **wgpu + egui shell (recommended if the canvas stays custom).** One immediate-mode canvas is a
   very good fit for crop handles, annotation objects and zoom/pan; wgpu gives crisp GPU scaling.
   Expected after tiling + preview texture: ~70–110 MB, ~120 ms. Cost: UI rewrite in egui,
   custom theming to keep the current look, CJK font embedding, no native widgets.
3. **Keep Tauri/WebView.** Zero rewrite, but it will stay around 260 MB and ~450 ms on this machine.

A staged migration is possible: ship a native **viewer** binary (option 1) that handles preview,
folder navigation, EXIF and crop, and keep the WebView editor for annotation-heavy work.
The GTK3 spike in `spikes/gtk3-viewer` is already a working viewer skeleton for that path.

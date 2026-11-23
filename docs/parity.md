# Parity checklist (web build ⇄ egui build)

Status: most rows are implemented and captured by the harness; rows still open are
marked ⏳ (open/save-as dialogs + folder browser, compare toggle, lossy WebP).

The eframe build is only shippable when every row is ✅. "Reference" means the
Tauri/WebKit build in this repo, driven by the same scripts.

| # | Area | Reference behaviour | Verification |
|---|---|---|---|
| 1 | Open | File manager / CLI opens straight into the viewer | launch `lumen-egui-app photo.jpg`, screenshot |
| 2 | Preview | fit ≥ min scale, wheel zoom at cursor, drag-pan only when zoomed | scripted xdotool + zoom label |
| 3 | Navigation | ← → through the folder, counter chip | scripted keys + screenshot |
| 4 | Info | dimensions, format, size, mtime, path, EXIF rows | compare with web info panel |
| 5 | Edit entry | E / button, rail + panels + dock appear | screenshot |
| 6 | Annotations | brush/line/rect/ellipse/arrow/text, colour/size/opacity/dashed | draw script, compare geometry ratios |
| 7 | Selection | move/resize/delete, 2-point handles for line+arrow | screenshot + unit tests |
| 8 | Crop | 80% default frame, ratio presets rebuild the box, Shift ratio, apply keeps for export | screenshot + unit tests |
| 9 | Resize | max long edge / explicit width, export pixels match | unit test on output dimensions |
| 10 | Compress | target KB: output ≤ target, within 3% of best | unit test |
| 11 | Save | save overwrite / save-as, JPEG white background, EXIF stripped | overwrite is wired + unit-tested; save-as dialog ⏳ |
| 12 | Undo | 80 steps, first undo works | scripted edit + Ctrl+Z |
| 13 | Languages | zh-TW / zh-CN / en follow LANG, LUMEN_LANG override | launch with each locale |
| 14 | Shortcuts | `?` panel, Esc behaviour, F fullscreen, I info | scripted |
| 15 | Benchmark | time-to-photo and PSS ≤ reference on the same box | `scripts/bench-egui.sh` vs `scripts/bench-vs-loupe.sh` |

Known deviations (documented, not yet resolved):

- WebP export is lossless-only (`image` crate); target-KB bisection applies to JPEG.
- Exported annotation text uses weight 400 where the web CSS uses 600.
- CJK glyphs outside the 390-glyph UI subset need the system Noto CJK collection
  (loaded lazily when the text tool is used; export falls back to the same system file).
- The in-app folder browser and the save-as dialog are implemented and wired (Ctrl+O /
  Ctrl+S / Ctrl+Shift+S, top-bar buttons); image rows use placeholder glyphs instead of
  real thumbnails.
- Visual pixel equality is not expected; the harness compares paired states and the
  remaining deltas are layout polish (preview peek card styling, crop rounding ±3 px).

## How to run

```bash
scripts/parity/capture.sh web      # reference build → .cache/parity/web/01-preview.png …
scripts/parity/capture.sh egui     # rewrite        → .cache/parity/egui/01-preview.png …
scripts/parity/compare.sh          # side-by-side sheets in .cache/parity/pairs/
scripts/parity/run-all.sh          # both captures + sheets + benchmarks
```

The harness uses a private Xvfb display (`PARITY_DISPLAY`, default :97) so it can run
alongside other work.  Each state is captured from both builds under the same file
name, which makes `magick compare` or a visual diff straightforward:

```bash
magick compare -metric AE .cache/parity/web/01-preview.png \
                          .cache/parity/egui/01-preview.png null: 2>&1
```

Pixel equality is not expected (different renderer); the acceptance bar is the
table above plus layout/geometry assertions (top bar 46 px, rail 56 px, panel
300 px, status bar 26 px) and the numeric unit tests in `egui-app/src/`.

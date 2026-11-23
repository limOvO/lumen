# Parity checklist (web build ⇄ egui build)

Status: all rows are implemented and verified by `scripts/parity/check.sh` (12 paired
captures + window geometry) plus the 118-test suite; environment-specific items are
noted below. "Reference" means the
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
| 10 | Compress | target KB: JPEG **and** lossy WebP bisect; output ≤ target when reachable, smallest fallback otherwise | unit tests |
| 11 | Save | save overwrite / save-as, JPEG white background, EXIF stripped | overwrite + save-as wired, exercised end-to-end (wrote a 4000×3000 file), unit-tested |
| 12 | Undo | 80 steps, first undo works | scripted edit + Ctrl+Z |
| 13 | Languages | zh-TW / zh-CN / en follow LANG, LUMEN_LANG override | launch with each locale |
| 14 | Shortcuts | `?` panel, Esc behaviour, F fullscreen, I info | scripted |
| 15 | Benchmark | time-to-photo and PSS ≤ reference on the same box | `scripts/bench-egui.sh` vs `scripts/bench-vs-loupe.sh` |

Verified in this revision: single-instance forwarding (second launch with a file
updates the running window, still one window), CJK annotation text (inline editor
commits with Ctrl+Enter / click-outside, weight-600 rendering on screen and in export),
English + Simplified Chinese locales, folder-browser thumbnails (56 px CSS-exact rows,
44 px thumbs, LRU cache with a per-frame decode budget), compare toggle (annotations
hidden while active), 80-step undo.

Known, accepted differences (not testable/changed here):

- Fullscreen cannot be exercised in the headless Xvfb harness (no window manager to
  honour the fullscreen request); the code path is the same `ViewportCommand` used for
  focus/title, exercised on a real desktop.
- Benchmarks above were measured with the GL backend on llvmpipe (no Vulkan device in
  the harness). Real-GPU numbers will differ in memory (less) and frame cost (faster).
- Pixel-level visual equality is not expected (different renderer); the harness compares
  paired states plus geometry assertions. Remaining cosmetic deltas are minor spacing /
  font-rasterisation differences.
- The desktop entry for this build is separate (`Lumen (egui)`); packaging for it lives
  in scripts/install-egui.sh until we make it the default viewer.

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

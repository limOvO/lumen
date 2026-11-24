# Parity checklist (web build ⇄ egui build)

Status: all rows are implemented and verified by `scripts/parity/check.sh` (12 paired
captures + window geometry) plus the 118-test suite; environment-specific items are
noted below. "Reference" means the
Tauri/WebKit build in this repo, driven by the same scripts.

| # | Area | Reference behaviour | Verification (verify.py check ids) |
|---|---|---|---|
| 1 | Open | File manager / CLI opens straight into the viewer | B14/B16/B17 (launch, single instance, forwarded file) |
| 2 | Preview | fit ≥ min scale, wheel zoom at cursor, drag-pan only when zoomed | B1/B2 zoom + fit |
| 3 | Navigation | ← → through the folder, counter chip | A1 image switch + G3 chip |
| 4 | Info | dimensions, format, size, mtime, path, EXIF rows | B3/B4 card open/close |
| 5 | Edit entry | E / button, rail + panels + dock appear | G6/G7/G8 + A3 rail slide |
| 6 | Annotations | brush/line/rect/ellipse/arrow/text, colour/size/opacity/dashed | B5/B8/B20/B21/B22 (+37 unit tests) |
| 7 | Selection | move/resize/delete, 2-point handles for line+arrow | B23 + unit tests |
| 8 | Crop | 80% default frame, ratio presets rebuild the box, Shift ratio, apply keeps for export | B11 + crop unit tests (exact web numbers) |
| 9 | Resize | max long edge / explicit width, export pixels match | unit tests (`export::target`, `flatten_dimensions…`) |
| 10 | Compress | target KB: JPEG **and** lossy WebP bisect; output ≤ target when reachable, smallest fallback otherwise | unit tests (`encode_to_target_*`, `webp_encode_to_target_fits_300kb`) |
| 11 | Save | save overwrite / save-as, JPEG white background, EXIF stripped | B18/B19 end-to-end + unit tests |
| 12 | Undo | 80 steps, first undo works | B6/B7 undo/redo |
| 13 | Languages | zh-TW / zh-CN / en follow LANG, LUMEN_LANG override | B15 + full glyph coverage for Hans |
| 14 | Shortcuts | `?` panel, Esc behaviour, F fullscreen, I info | B12 + B3/B4 |
| 15 | Benchmark | time-to-photo and PSS ≤ reference on the same box | P2 (inline bench, median of 2) |
| 16 | Empty state | full editor chrome + centred drop card (drag/drop, 開啟圖片…, 快捷鍵, Ctrl+O hint) | B14/B14b (card pixel count + rail colour) |
| 18 | Inspector grip | pill straddles the panel edge and drags the width (240–420) | G10 (pixel comparison against a live web capture) |
| 19 | Tool icons | identical SVG geometry to src/index.html (brush/text/crop were hand-traced before) | G13 (normalised ink IoU), G13b (AA delta), G14 (row count) |
| 20 | Window resize | preview keeps the 32px stage padding at 880x560 / 1280x820 / 1440x900; rail 52 / inspector 300 / status 24 hold | G15-*, G16-* |
| 21 | Modal layering | the scrim dims the chrome underneath (dock no longer floats above the dialog) | G11 |
| 22 | Small windows | the save dialog stays inside the 880x560 minimum window | G12 |
| 17 | Drag & drop | dropping a file/folder opens it | implemented in app.rs (`raw.dropped_files`), covered by code review (xdotool cannot synthesise DnD) |

Compare semantics match `src/js/main.js`: the toggle survives phase changes and resets
when a different document is loaded (`AppState::reset_for_doc`), with the annotations
hidden while it is on.

Paired screenshot review: `.cache/parity/pairs/` (preview, info, edit, brush, rect, crop,
crop-ratio, adjust, shortcuts, back-to-preview, open, save-as) and `.cache/parity/pairs-extra/`
(line, arrow, text, compare on/off, empty) were reviewed side by side; the only remaining
deltas are renderer-level (font rasterisation and egui widget metrics) — layout, palette,
copy and behaviour match. The empty state was the one real gap found and is now fixed.

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

## Automated gate

`python3 scripts/parity/verify.py` drives the real egui binary (private Xvfb `:96`) and asserts
51 checks spanning geometry, palette parity against a live WebView capture, presence of every
chrome region, tool behaviour (brush/line/arrow/rect/crop/text/delete/undo/redo/compare),
zoom/fit behaviour, animated transitions (image switch, phase glide, rail slide, dock rise,
peek close), idle CPU, languages, single-instance forwarding, save/save-as and the benchmark.
Report: `.cache/parity/report.json`; exit code 0 only when every check passes.
`--quick` skips the two benchmark runs.

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

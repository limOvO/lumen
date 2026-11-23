# egui-app module interfaces

Owner of this file: Lead. Read it before editing anything.

## Ground rules

- **Only edit the files listed under your task's write scope.** Build artifacts
  (`target/`) are ignored; never commit them.
- Do not touch `Cargo.toml`, `src/main.rs`, `src/app.rs`, `src/state.rs`,
  `src/theme.rs`, `src/doc.rs`, `src/i18n*.rs` — ask the Lead for changes there.
- All user-visible text goes through `state.locale.tr("繁中原文")` (or `trf` with
  `{0}` placeholders). The Traditional Chinese string is the dictionary key; if a
  string is missing, add it to `scripts/i18n/keys.json` + `scripts/i18n/en.json`
  and tell the Lead (the Lead regenerates `i18n_data.rs` and the font subset).
- Coordinates for annotations/crop are **image pixels** (the source image), not
  screen pixels. Use `CanvasView::to_screen` / `to_image` for conversion.
- Colours/sizes come from `src/theme.rs`.
- Every module must keep `cargo build --release --manifest-path egui-app/Cargo.toml`
  green, and add unit tests for the maths it implements.

## state.rs (Lead-owned)

```rust
pub struct AppState {
    pub locale: Locale,
    pub doc: Option<Doc>,
    pub error: Option<String>,
    pub view: View,            // scale, offset, fit
    pub phase: Phase,          // Preview | Edit
    pub tool: Tool,            // Move, Brush, Line, Rect, Ellipse, Arrow, Text, Crop
    pub style: Style,          // annotate::Style
    pub objects: Vec<Annot>,   // annotate::Annot, image px
    pub selected: Option<u64>,
    pub crop: Option<CropRect>,
    pub crop_committed: bool,
    pub output: OutputSettings,
    pub show_info: bool,
    pub show_shortcuts: bool,
    pub dirty: bool,
    pub fullscreen: bool,
    pub status: Option<StatusMessage>,
    pub siblings: Vec<PathBuf>,
    pub index: usize,
    // private undo/redo history
}
impl AppState {
    fn commit(&mut self);              // call before a mutation
    fn undo(&mut self) -> bool;
    fn redo(&mut self) -> bool;
    fn set_tool(&mut self, tool: Tool);
    fn add_annot(&mut self, a: Annot); // commits + selects
    fn delete_selected(&mut self);
    fn selected_object(&self) -> Option<&Annot>;
    fn selected_object_mut(&mut self) -> Option<&mut Annot>;
    fn status(&mut self, text, kind, now);
}
```

## annotate.rs (annotations teammate)

```rust
pub enum AnnotKind { Brush, Line, Rect, Ellipse, Arrow, Text }
pub struct Style { color, size, opacity, dashed, fill, font_size }
pub struct Annot { id, kind, points: Vec<Pos2> /* image px */, text, style }
impl Annot {
    fn new(kind, points, style) -> Self;
    fn bounds(&self) -> Rect;                       // image px
    fn handles(&self) -> Vec<(Handle, Pos2)>;       // Start/End for line+arrow, 8 box handles otherwise
    fn hit(&self, pt: Pos2, tol: f32) -> bool;      // tol in image px
    fn move_by(&mut self, delta: Vec2);
    fn resize(&mut self, handle: Handle, pt: Pos2, keep_ratio: bool);
    fn draw(&self, painter, to_screen: &dyn Fn(Pos2)->Pos2, zoom: f32);
}
pub fn hit_topmost(objects: &[Annot], pt: Pos2, tol: f32) -> Option<usize>;
```
Parity reference: `src/js/core/annotate.js` (dash pattern `size*2.4 / size*1.6`,
arrow head `max(size*3.2, size+6)`, brush width from speed).

## crop.rs (crop/export teammate)

```rust
pub struct CropRect { x, y, w, h, ratio: Option<f32> }
impl CropRect {
    fn centered(doc_w, doc_h, ratio) -> Self;             // 80% centred
    fn with_ratio(self, doc_w, doc_h, ratio) -> Self;     // keep centre, rebuild size
    fn handles(self) -> [(CropHandle, Pos2); 8];
    fn resize(self, handle, pt, doc_w, doc_h) -> Self;
    fn draw(&self, painter, to_screen, zoom);
}
```
Parity reference: `src/js/ui/panel-crop.js`, `src/js/core/interaction.js` (crop gestures),
ratio presets: 自由/原始/1:1/4:3/3:2/16:9.

## export.rs / save.rs (crop/export teammate)

```rust
fn flatten(doc: &Doc, objects: &[Annot], crop: Option<CropRect>,
           max_dim: Option<u32>, scale: Option<f32>) -> Flattened;
fn encode(image: &RgbaImage, format: OutputFormat, quality: u8) -> Vec<u8>;
fn encode_to_target(image: &RgbaImage, format, target_kb) -> Vec<u8>;
fn suggested_name(doc, format) -> PathBuf;
fn write(path, bytes) -> Result<(), String>;
```
Parity: `src/js/core/export.js` (JPEG white background, target-KB bisection
tolerance ≤ 3% or smallest candidate).

## interaction.rs (annotations teammate)

```rust
pub struct CanvasView { rect: Rect, to_screen: Box<dyn Fn(Pos2)->Pos2>,
                        to_image: Box<dyn Fn(Pos2)->Pos2>, zoom: f32 }
pub fn handle_canvas(ui: &egui::Ui, state: &mut AppState, view: &CanvasView);
```
Must implement, matching `src/js/core/interaction.js`:
preview: wheel zoom at cursor (min = fit); drag pan only when zoomed;
edit: pan anywhere (no clamping), Space/middle pan, tools:
- Brush: freehand, speed→width, incremental preview.
- Line/Arrow: two-click or drag; Esc cancels; `Shift` snaps 45°.
- Rect/Ellipse: drag from corner; Shift keeps square/circle.
- Text: click → inline editor → text object.
- Crop: 8 handles + inside drag; `Shift` keeps ratio.
- Selection: move/delete/resize handles; handles are 2 points for line/arrow.

## ui/*.rs (UI teammate)

`pub fn show(ctx: &egui::Context, state: &mut AppState)` per region:
`topbar.rs`, `rail.rs`, `info.rs`, `status.rs`, `dock.rs`, `shortcuts.rs`.
Parity target: `src/index.html` + `src/css/app.css` (46px top bar, 56px rail,
300px panel, 26px status bar, dark palette from tokens.css), preview chrome
(chips dock: counter / info ⓘ / edit ✎), peek info card, style dock visible only
for drawing tools including dashed toggle hidden for text.

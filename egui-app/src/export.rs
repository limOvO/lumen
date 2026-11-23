// Non-destructive export: flatten (crop + resize + annotations) then encode,
// with an optional "compress to a target KB" bisection.
// Owner: crop/export teammate.
//
// Parity source: src/js/core/export.js — targetDims/renderToCanvas (crop clamp,
// extra scale, max long edge, annotations re-drawn at the same ratio),
// white-backed JPEG, encodeToTarget bisection (8 probes, <= target, smallest
// candidate when even the lowest quality is too big), estimateSize.
//
// Rasterisation: tiny-skia (paths/dashes/AA) + ab_glyph (CJK text outlines from
// the bundled Noto Sans CJK subset OTFs) so we never need a GPU or a WebView.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use ab_glyph::{Font, FontRef, OutlineCurve, Point as GlyphPoint, ScaleFont};
use egui::Pos2;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::imageops::FilterType;
use image::{ExtendedColorType, ImageEncoder, RgbaImage};
use tiny_skia::{
    FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, StrokeDash, Transform,
};

use crate::annotate::{Annot, AnnotKind};
use crate::crop::CropRect;
use crate::doc::Doc;
use crate::state::{OutputFormat, OutputSettings};

// Noto Sans CJK subsets shared with the egui font setup (src/fonts.rs).
// The Bold (weight 600) pair is export-only: the web canvas renders annotation
// text with `font-weight: 600`, while the egui screen keeps the regular face.
use crate::fonts::{CJK_SC, CJK_SC_BOLD, CJK_TC, CJK_TC_BOLD};

pub struct Flattened {
    pub image: RgbaImage,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Estimated {
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
}

/// Resolved render target: the clamped source rectangle (image px) plus the
/// output pixel size. Mirrors `targetDims` in export.js.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Target {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub out_w: u32,
    pub out_h: u32,
}

fn clamp(v: f32, lo: f32, hi: f32) -> f32 {
    // NB: not f32::clamp — it panics when lo > hi (possible for 1px images).
    lo.max(hi.min(v))
}

impl Target {
    /// image px -> output px transform for annotations, given the actual render
    /// width (export.js uses the width ratio for both axes so annotation
    /// geometry stays uniform; a half-size preview just gets a smaller width).
    pub fn ann_transform_for_width(self, out_w: u32) -> Transform {
        let s = out_w as f32 / self.w.max(0.001);
        Transform::from_row(s, 0.0, 0.0, s, -self.x * s, -self.y * s)
    }

    /// Integer source crop, clamped to the decoded bitmap.
    pub fn source_rect(self, base_w: u32, base_h: u32) -> (u32, u32, u32, u32) {
        let bw = base_w.max(1) as f32;
        let bh = base_h.max(1) as f32;
        let sx = self.x.round().clamp(0.0, bw - 1.0) as u32;
        let sy = self.y.round().clamp(0.0, bh - 1.0) as u32;
        let sw = self.w.round().max(1.0).min(bw - sx as f32) as u32;
        let sh = self.h.round().max(1.0).min(bh - sy as f32) as u32;
        (sx, sy, sw.max(1), sh.max(1))
    }
}

/// Clamp the crop to the image and resolve the output pixel dimensions
/// (export.js `targetDims`): crop first, then `scale`, then `max_dim` on the
/// long edge.
pub fn target(
    base_w: u32,
    base_h: u32,
    crop: Option<CropRect>,
    max_dim: Option<u32>,
    scale: Option<f32>,
) -> Target {
    let bw = base_w.max(1) as f32;
    let bh = base_h.max(1) as f32;
    let (mut x, mut y, mut w, mut h) = (0.0f32, 0.0f32, bw, bh);
    if let Some(c) = crop {
        if c.w > 0.0 && c.h > 0.0 {
            let x0 = clamp(c.x, 0.0, bw);
            let y0 = clamp(c.y, 0.0, bh);
            let x1 = clamp(c.x + c.w, 0.0, bw);
            let y1 = clamp(c.y + c.h, 0.0, bh);
            x = x0;
            y = y0;
            w = (x1 - x0).max(1.0);
            h = (y1 - y0).max(1.0);
        }
    }
    let extra = scale.filter(|s| s.is_finite() && *s > 0.0).unwrap_or(1.0);
    let mut raw_w = w * extra;
    let mut raw_h = h * extra;
    if let Some(max) = max_dim.filter(|m| *m > 0) {
        let max = max as f32;
        let long = raw_w.max(raw_h);
        if long > max {
            let f = max / long;
            raw_w *= f;
            raw_h *= f;
        }
    }
    let out_w = (raw_w.round() as i64).max(1) as u32;
    let out_h = (raw_h.round() as i64).max(1) as u32;
    Target { x, y, w, h, out_w, out_h }
}

// ---------------------------------------------------------------------------
// Full-resolution decode
// ---------------------------------------------------------------------------

struct Decoded {
    path: PathBuf,
    len: u64,
    modified: Option<std::time::SystemTime>,
    image: Arc<RgbaImage>,
}

/// The document keeps only the GPU texture, so exporting decodes the source
/// file again at full resolution. One-entry cache keeps live size estimates
/// cheap without retaining a second copy of the pixels.
static DECODE_CACHE: OnceLock<Mutex<Option<Decoded>>> = OnceLock::new();

fn cache() -> &'static Mutex<Option<Decoded>> {
    DECODE_CACHE.get_or_init(|| Mutex::new(None))
}

fn decode_full(doc: &Doc) -> Option<Arc<RgbaImage>> {
    let meta = std::fs::metadata(&doc.path).ok();
    let len = meta.as_ref().map(|m| m.len()).unwrap_or(0);
    let modified = meta.and_then(|m| m.modified().ok());
    {
        let guard = cache().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(c) = guard.as_ref() {
            if c.path == doc.path && c.len == len && c.modified == modified {
                return Some(c.image.clone());
            }
        }
    }
    let bytes = std::fs::read(&doc.path).ok()?;
    let image = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let image = Arc::new(image);
    let mut guard = cache().lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some(Decoded {
        path: doc.path.clone(),
        len,
        modified,
        image: image.clone(),
    });
    Some(image)
}

// ---------------------------------------------------------------------------
// Annotation rasterisation (tiny-skia)
// ---------------------------------------------------------------------------

fn color_rgba8(color: egui::Color32, opacity: f32) -> [u8; 4] {
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    let alpha = (a as f32 * clamp(opacity, 0.0, 1.0)).round() as u8;
    [r, g, b, alpha]
}

fn solid(rgba: [u8; 4]) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]);
    paint.anti_alias = true;
    paint
}

fn stroke_for(size: f32, dashed: bool) -> Stroke {
    let width = size.max(0.1);
    let mut stroke = Stroke {
        width,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Default::default()
    };
    if dashed {
        // annotate.js `applyDash`: [w * 2.4, w * 1.6] in image px.
        let w = width.max(1.0);
        stroke.dash = StrokeDash::new(vec![w * 2.4, w * 1.6], 0.0);
    }
    stroke
}

fn mid(a: Pos2, b: Pos2) -> Pos2 {
    Pos2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0)
}

fn draw_brush(pix: &mut Pixmap, obj: &Annot, tf: Transform) {
    let rgba = color_rgba8(obj.style.color, obj.style.opacity);
    let paint = solid(rgba);
    let pts = &obj.points;
    if pts.is_empty() {
        return;
    }
    if pts.len() == 1 {
        let r = (obj.style.size / 2.0).max(0.7);
        if let Some(path) = PathBuilder::from_circle(pts[0].x, pts[0].y, r) {
            pix.fill_path(&path, &paint, FillRule::Winding, tf, None);
        }
        return;
    }
    // Same midpoint-quadratic smoothing as annotate.js drawBrush, built as one
    // continuous path (round caps/joins make the per-segment version identical).
    let mut b = PathBuilder::new();
    b.move_to(pts[0].x, pts[0].y);
    for i in 1..pts.len() {
        let p1 = mid(pts[i - 1], pts[i]);
        b.quad_to(pts[i - 1].x, pts[i - 1].y, p1.x, p1.y);
    }
    if let Some(path) = b.finish() {
        let stroke = stroke_for(obj.style.size.max(0.6), obj.style.dashed);
        pix.stroke_path(&path, &paint, &stroke, tf, None);
    }
}

fn draw_line(pix: &mut Pixmap, obj: &Annot, tf: Transform) {
    if obj.points.len() < 2 {
        return;
    }
    let (a, b) = (obj.points[0], obj.points[1]);
    if (b - a).length() < 0.01 {
        return;
    }
    let paint = solid(color_rgba8(obj.style.color, obj.style.opacity));
    let mut pb = PathBuilder::new();
    pb.move_to(a.x, a.y);
    pb.line_to(b.x, b.y);
    if let Some(path) = pb.finish() {
        pix.stroke_path(&path, &paint, &stroke_for(obj.style.size, obj.style.dashed), tf, None);
    }
}

fn draw_arrow(pix: &mut Pixmap, obj: &Annot, tf: Transform) {
    if obj.points.len() < 2 {
        return;
    }
    let (a, b) = (obj.points[0], obj.points[1]);
    let d = b - a;
    let len = d.length();
    if len < 0.01 {
        return;
    }
    let (ux, uy) = (d.x / len, d.y / len);
    let size = obj.style.size;
    let head = (size * 3.2).max(size + 6.0);
    let neck = (len * 0.6).min(head * 0.85);
    let rgba = color_rgba8(obj.style.color, obj.style.opacity);
    let paint = solid(rgba);

    // shaft (stops at the arrow neck, like annotate.js)
    let mut pb = PathBuilder::new();
    pb.move_to(a.x, a.y);
    pb.line_to(b.x - ux * neck, b.y - uy * neck);
    if let Some(path) = pb.finish() {
        pix.stroke_path(&path, &paint, &stroke_for(size, obj.style.dashed), tf, None);
    }

    // head: filled triangle, half-width head * 0.42
    let half = head * 0.42;
    let mut hb = PathBuilder::new();
    hb.move_to(b.x, b.y);
    hb.line_to(b.x - ux * head + uy * half, b.y - uy * head - ux * half);
    hb.line_to(b.x - ux * head - uy * half, b.y - uy * head + ux * half);
    hb.close();
    if let Some(path) = hb.finish() {
        pix.fill_path(&path, &paint, FillRule::Winding, tf, None);
    }
}

fn draw_rect(pix: &mut Pixmap, obj: &Annot, tf: Transform) {
    if obj.points.len() < 2 {
        return;
    }
    let a = obj.points[0];
    let b = obj.points[1];
    let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
    let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
    let Some(rect) = tiny_skia::Rect::from_ltrb(x0, y0, x1, y1) else { return };
    let paint = solid(color_rgba8(obj.style.color, obj.style.opacity));
    if obj.style.fill {
        pix.fill_rect(rect, &paint, tf, None);
    }
    let mut pb = PathBuilder::new();
    pb.push_rect(rect);
    if let Some(path) = pb.finish() {
        pix.stroke_path(
            &path,
            &paint,
            &stroke_for(obj.style.size, obj.style.dashed),
            tf,
            None,
        );
    }
}

fn draw_ellipse(pix: &mut Pixmap, obj: &Annot, tf: Transform) {
    if obj.points.len() < 2 {
        return;
    }
    let a = obj.points[0];
    let b = obj.points[1];
    let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
    let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
    let Some(rect) = tiny_skia::Rect::from_ltrb(x0, y0, x1, y1) else { return };
    let mut pb = PathBuilder::new();
    pb.push_oval(rect);
    let Some(path) = pb.finish() else { return };
    let paint = solid(color_rgba8(obj.style.color, obj.style.opacity));
    if obj.style.fill {
        pix.fill_path(&path, &paint, FillRule::Winding, tf, None);
    }
    pix.stroke_path(&path, &paint, &stroke_for(obj.style.size, obj.style.dashed), tf, None);
}

// --- text ------------------------------------------------------------------

/// Embedded subsets (same files `fonts.rs` installs into egui): the Bold pair
/// carries `font-weight: 600` for export, the Regular pair is the fallback.
struct FontSet {
    tc_bold: FontRef<'static>,
    sc_bold: FontRef<'static>,
    tc: FontRef<'static>,
    sc: FontRef<'static>,
}

fn fonts() -> Option<&'static FontSet> {
    static FONTS: OnceLock<Option<FontSet>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let tc_bold = FontRef::try_from_slice(CJK_TC_BOLD).ok()?;
            let sc_bold = FontRef::try_from_slice(CJK_SC_BOLD).ok()?;
            let tc = FontRef::try_from_slice(CJK_TC).ok()?;
            let sc = FontRef::try_from_slice(CJK_SC).ok()?;
            Some(FontSet { tc_bold, sc_bold, tc, sc })
        })
        .as_ref()
}

/// Annotation text is free-form, so characters outside the UI subsets fall back
/// to the system Noto Sans CJK collection (the Bold face first, so the exported
/// weight stays 600). Loaded lazily and only when a glyph is missing; export
/// still works without it (missing glyphs advance half an em).
const SYSTEM_TTC_BOLD: &str = "/usr/share/fonts/noto-cjk/NotoSansCJK-Bold.ttc";
const SYSTEM_TTC_REGULAR: &str = "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc";
const SYSTEM_TTC_TC_INDEX: u32 = 3; // Noto Sans CJK TC inside the collection

fn system_face(path: &'static str) -> Option<FontRef<'static>> {
    let bytes = std::fs::read(path).ok()?;
    let leaked: &'static [u8] = Box::leak(bytes.into_boxed_slice());
    FontRef::try_from_slice_and_index(leaked, SYSTEM_TTC_TC_INDEX).ok()
}

fn full_font(bold: bool) -> Option<&'static FontRef<'static>> {
    static BOLD: OnceLock<Option<FontRef<'static>>> = OnceLock::new();
    static REGULAR: OnceLock<Option<FontRef<'static>>> = OnceLock::new();
    let (path, slot) = if bold {
        (SYSTEM_TTC_BOLD, &BOLD)
    } else {
        (SYSTEM_TTC_REGULAR, &REGULAR)
    };
    slot.get_or_init(|| system_face(path)).as_ref()
}

/// First font that has a glyph for `ch` at the requested weight: embedded
/// subsets first, then the system family.
fn glyph_for(ch: char, bold: bool) -> Option<(&'static FontRef<'static>, ab_glyph::GlyphId)> {
    let set = fonts()?;
    let candidates = if bold {
        [&set.tc_bold, &set.sc_bold, &set.tc, &set.sc]
    } else {
        [&set.tc, &set.sc, &set.tc, &set.sc]
    };
    for font in candidates {
        let id = font.glyph_id(ch);
        if id.0 != 0 {
            return Some((font, id));
        }
    }
    let full = full_font(bold)?;
    let id = full.glyph_id(ch);
    if id.0 != 0 {
        return Some((full, id));
    }
    None
}

fn move_to_if_needed(b: &mut PathBuilder, last: &mut Option<(f32, f32)>, p: (f32, f32)) {
    let same = matches!(*last, Some(l) if (l.0 - p.0).abs() < 0.01 && (l.1 - p.1).abs() < 0.01);
    if !same {
        b.move_to(p.0, p.1);
    }
}

/// Append one glyph outline (y-up font units) as a tiny-skia path in image px,
/// so the annotation transform scales it with everything else.
fn append_outline(
    b: &mut PathBuilder,
    curves: &[OutlineCurve],
    pen_x: f32,
    baseline: f32,
    hs: f32,
    vs: f32,
) {
    let map = |p: GlyphPoint| (pen_x + p.x * hs, baseline - p.y * vs);
    let mut last: Option<(f32, f32)> = None;
    for curve in curves {
        match curve {
            OutlineCurve::Line(p0, p1) => {
                let a = map(*p0);
                let z = map(*p1);
                move_to_if_needed(b, &mut last, a);
                b.line_to(z.0, z.1);
                last = Some(z);
            }
            OutlineCurve::Quad(p0, p1, p2) => {
                let a = map(*p0);
                let c = map(*p1);
                let z = map(*p2);
                move_to_if_needed(b, &mut last, a);
                b.quad_to(c.0, c.1, z.0, z.1);
                last = Some(z);
            }
            OutlineCurve::Cubic(p0, p1, p2, p3) => {
                let a = map(*p0);
                let c1 = map(*p1);
                let c2 = map(*p2);
                let z = map(*p3);
                move_to_if_needed(b, &mut last, a);
                b.cubic_to(c1.0, c1.1, c2.0, c2.1, z.0, z.1);
                last = Some(z);
            }
        }
    }
}

fn draw_text(pix: &mut Pixmap, obj: &Annot, tf: Transform) {
    draw_text_weighted(pix, obj, tf, true);
}

/// `weight_600` picks the Bold subset (what the web canvas renders) or the
/// Regular one; tests use the toggle to prove the exported text is heavier.
fn draw_text_weighted(pix: &mut Pixmap, obj: &Annot, tf: Transform, weight_600: bool) {
    if obj.text.is_empty() {
        return;
    }
    let Some(origin) = obj.points.first().copied() else { return };
    let Some(set) = fonts() else { return };
    let size = obj.style.font_size.max(1.0);
    let paint = solid(color_rgba8(obj.style.color, obj.style.opacity));
    // Canvas textBaseline = 'top': the first line's baseline sits one ascent down.
    let metrics = if weight_600 { &set.tc_bold } else { &set.tc };
    let mut baseline = origin.y + metrics.as_scaled(size).ascent();
    let line_height = size * 1.28; // annotate.js measureText lineHeight
    for line in obj.text.split('\n') {
        let mut b = PathBuilder::new();
        let mut pen = origin.x;
        let mut prev: Option<ab_glyph::GlyphId> = None;
        for ch in line.chars() {
            let Some((font, gid)) = glyph_for(ch, weight_600) else {
                // No font on this host covers the character: advance half an em.
                pen += size * 0.5;
                prev = None;
                continue;
            };
            let sf = font.as_scaled(size);
            if let Some(outline) = font.outline(gid) {
                append_outline(
                    &mut b,
                    &outline.curves,
                    pen,
                    baseline,
                    sf.h_scale_factor(),
                    sf.v_scale_factor(),
                );
            }
            if let Some(p) = prev {
                pen += sf.kern(p, gid);
            }
            pen += sf.h_advance(gid);
            prev = Some(gid);
        }
        if let Some(path) = b.finish() {
            pix.fill_path(&path, &paint, FillRule::Winding, tf, None);
        }
        baseline += line_height;
    }
}

/// Rasterise all annotations into a transparent layer in output pixel space.
fn annotation_layer(objects: &[Annot], tf: Transform, out_w: u32, out_h: u32) -> Option<Pixmap> {
    if objects.is_empty() {
        return None;
    }
    let mut pix = Pixmap::new(out_w.max(1), out_h.max(1))?;
    for obj in objects {
        match obj.kind {
            AnnotKind::Brush => draw_brush(&mut pix, obj, tf),
            AnnotKind::Line => draw_line(&mut pix, obj, tf),
            AnnotKind::Arrow => draw_arrow(&mut pix, obj, tf),
            AnnotKind::Rect => draw_rect(&mut pix, obj, tf),
            AnnotKind::Ellipse => draw_ellipse(&mut pix, obj, tf),
            AnnotKind::Text => draw_text(&mut pix, obj, tf),
        }
    }
    Some(pix)
}

/// Composite a premultiplied annotation layer over the (straight-alpha) base.
fn composite_over(base: &mut RgbaImage, layer: &Pixmap) {
    let (w, h) = (base.width().min(layer.width()), base.height().min(layer.height()));
    for y in 0..h {
        for x in 0..w {
            let Some(src) = layer.pixel(x, y) else { continue };
            let src_a = src.alpha();
            if src_a == 0 {
                continue;
            }
            let sa = src_a as f32 / 255.0;
            let src_d = src.demultiply();
            let sr = src_d.red() as f32 / 255.0;
            let sg = src_d.green() as f32 / 255.0;
            let sb = src_d.blue() as f32 / 255.0;
            let dst = base.get_pixel_mut(x, y);
            let da = dst[3] as f32 / 255.0;
            let out_a = sa + da * (1.0 - sa);
            if out_a <= 0.0 {
                *dst = image::Rgba([0, 0, 0, 0]);
                continue;
            }
            for (i, s) in [sr, sg, sb].into_iter().enumerate() {
                let d = dst[i] as f32 / 255.0;
                let v = (s * sa + d * da * (1.0 - sa)) / out_a;
                dst[i] = (v * 255.0).round().clamp(0.0, 255.0) as u8;
            }
            dst[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
}

// ---------------------------------------------------------------------------
// Flatten
// ---------------------------------------------------------------------------

fn render(
    base: &RgbaImage,
    objects: &[Annot],
    t: &Target,
    out_w: u32,
    out_h: u32,
) -> RgbaImage {
    let (bw, bh) = base.dimensions();
    let (sx, sy, sw, sh) = t.source_rect(bw, bh);
    let cropped = image::imageops::crop_imm(base, sx, sy, sw, sh).to_image();
    let mut image = if cropped.dimensions() == (out_w, out_h) {
        cropped
    } else {
        image::imageops::resize(&cropped, out_w, out_h, FilterType::CatmullRom)
    };
    if let Some(layer) = annotation_layer(objects, t.ann_transform_for_width(out_w), out_w, out_h) {
        composite_over(&mut image, &layer);
    }
    image
}

/// Flatten already-decoded pixels (unit-testable core of `flatten`).
pub fn flatten_pixels(
    base: &RgbaImage,
    objects: &[Annot],
    crop: Option<CropRect>,
    max_dim: Option<u32>,
    scale: Option<f32>,
) -> Flattened {
    let (bw, bh) = base.dimensions();
    let t = target(bw, bh, crop, max_dim, scale);
    let image = render(base, objects, &t, t.out_w, t.out_h);
    Flattened { image, width: t.out_w, height: t.out_h }
}

/// Flattened export of a document (crop + resize + annotations), decoding the
/// source file at full resolution first.
pub fn flatten(
    doc: &Doc,
    objects: &[Annot],
    crop: Option<CropRect>,
    max_dim: Option<u32>,
    scale: Option<f32>,
) -> Flattened {
    flatten_try(doc, objects, crop, max_dim, scale).unwrap_or_else(|_| {
        // Decode failure: still return a correctly sized (transparent) canvas so
        // callers get usable dimensions instead of a panic.
        let t = target(doc.width.max(1), doc.height.max(1), crop, max_dim, scale);
        let image = RgbaImage::new(t.out_w, t.out_h);
        Flattened { image, width: t.out_w, height: t.out_h }
    })
}

pub fn flatten_try(
    doc: &Doc,
    objects: &[Annot],
    crop: Option<CropRect>,
    max_dim: Option<u32>,
    scale: Option<f32>,
) -> Result<Flattened, String> {
    let base = decode_full(doc)
        .ok_or_else(|| format!("無法解碼圖片以匯出：{}", doc.name))?;
    Ok(flatten_pixels(&base, objects, crop, max_dim, scale))
}

// ---------------------------------------------------------------------------
// Encoding
// ---------------------------------------------------------------------------

/// JPEG has no alpha channel: composite over an opaque white sheet first
/// (export.js `whiteBacked`).
fn white_backed_rgb(image: &RgbaImage) -> Vec<u8> {
    let mut out = Vec::with_capacity((image.width() as usize) * (image.height() as usize) * 3);
    for p in image.pixels() {
        let a = p[3] as u32;
        let inv = 255 - a;
        out.push(((p[0] as u32 * a + 255 * inv + 127) / 255) as u8);
        out.push(((p[1] as u32 * a + 255 * inv + 127) / 255) as u8);
        out.push(((p[2] as u32 * a + 255 * inv + 127) / 255) as u8);
    }
    out
}

/// Encode the flattened image. JPEG and WebP honour `quality` (0-100,
/// clamped); PNG is lossless and ignores it.
pub fn encode(image: &RgbaImage, format: OutputFormat, quality: u8) -> Vec<u8> {
    try_encode(image, format, quality).unwrap_or_default()
}

/// Lossy WebP through libwebp (`webp` crate). Alpha is kept in the ALPH chunk
/// (libwebp `alpha_compression`), matching what the web canvas produces.
fn encode_webp_lossy(image: &RgbaImage, quality: u8) -> Result<Vec<u8>, String> {
    let q = quality.clamp(1, 100) as f32;
    let encoder = webp::Encoder::from_rgba(image.as_raw(), image.width(), image.height());
    let memory = encoder
        .encode_simple(false, q)
        .map_err(|e| format!("WEBP 編碼失敗：{e:?}"))?;
    Ok(memory.to_vec())
}

pub fn try_encode(
    image: &RgbaImage,
    format: OutputFormat,
    quality: u8,
) -> Result<Vec<u8>, String> {
    let w = image.width().max(1);
    let h = image.height().max(1);
    if w == 0 || h == 0 {
        return Err("沒有可編碼的圖片".into());
    }
    let mut out = Vec::new();
    match format {
        OutputFormat::Auto => {
            return Err("輸出格式尚未解析（Auto）".into());
        }
        OutputFormat::Jpeg => {
            let q = quality.clamp(1, 100);
            let rgb = white_backed_rgb(image);
            JpegEncoder::new_with_quality(&mut out, q)
                .write_image(&rgb, w, h, ExtendedColorType::Rgb8)
                .map_err(|e| format!("JPEG 編碼失敗：{e}"))?;
        }
        OutputFormat::Png => {
            PngEncoder::new(&mut out)
                .write_image(image.as_raw(), w, h, ExtendedColorType::Rgba8)
                .map_err(|e| format!("PNG 編碼失敗：{e}"))?;
        }
        OutputFormat::Webp => {
            out = encode_webp_lossy(image, quality)?;
        }
    }
    Ok(out)
}

/// Binary-search the highest quality whose payload still fits `target_kb`
/// (export.js `encodeToTarget`): 8 probes, stop once the bracket is < 0.02.
/// Applies to JPEG and lossy WebP; PNG gets a single lossless encode.
/// Returns the smallest candidate when even the lowest quality overflows.
pub fn encode_to_target_with_quality(
    image: &RgbaImage,
    format: OutputFormat,
    target_kb: u32,
) -> (Vec<u8>, u8) {
    let target = (target_kb as usize).saturating_mul(1024).max(1024);
    let first = encode(image, format, 90);
    // JPEG and lossy WebP trade quality for size; PNG is lossless (one probe).
    if !matches!(format, OutputFormat::Jpeg | OutputFormat::Webp)
        || first.len() <= target
        || first.is_empty()
    {
        return (first, 90);
    }
    let mut lo = 0.1f32;
    let mut hi = 0.95f32;
    let mut best: Option<(Vec<u8>, u8)> = None;
    let mut smallest: Option<(Vec<u8>, u8)> = None;
    for _ in 0..8 {
        let q = (lo + hi) / 2.0;
        let q_u8 = ((q * 100.0).round() as u8).clamp(1, 100);
        let bytes = encode(image, format, q_u8);
        if smallest.as_ref().map_or(true, |(b, _)| bytes.len() <= b.len()) {
            smallest = Some((bytes.clone(), q_u8));
        }
        if bytes.len() <= target {
            best = Some((bytes, q_u8));
            lo = q;
        } else {
            hi = q;
        }
        if hi - lo < 0.02 {
            break;
        }
    }
    if let Some(b) = best {
        return b;
    }
    smallest.unwrap_or((first, 90))
}

pub fn encode_to_target(image: &RgbaImage, format: OutputFormat, target_kb: u32) -> Vec<u8> {
    encode_to_target_with_quality(image, format, target_kb).0
}

/// Fast size estimate (export.js `estimateSize`): render/encode at half size
/// and scale the byte count by the pixel ratio.
pub fn estimate_size(
    doc: &Doc,
    objects: &[Annot],
    output: &OutputSettings,
    crop: Option<CropRect>,
) -> Estimated {
    let t = target(doc.width, doc.height, crop, output.max_dim, output.scale);
    let format = output.format.resolve(doc);
    let Some(base) = decode_full(doc) else {
        return Estimated { bytes: 0, width: t.out_w, height: t.out_h };
    };
    let preview_w = (t.out_w / 2).max(1);
    let preview_h = (t.out_h / 2).max(1);
    let preview = render(&base, objects, &t, preview_w, preview_h);
    let bytes = encode(&preview, format, output.quality).len() as u64;
    let ratio = (t.out_w as u64 * t.out_h as u64) / (preview_w as u64 * preview_h as u64).max(1);
    Estimated { bytes: (bytes * ratio).max(1), width: t.out_w, height: t.out_h }
}

/// Drop the cached full-resolution decode (call when the document changes).
pub fn clear_decode_cache() {
    *cache().lock().unwrap_or_else(|e| e.into_inner()) = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotate::{Annot, AnnotKind, Style};
    use crate::doc::{Doc, LoadOptions};
    use image::{Rgba, RgbaImage};
    use std::path::PathBuf;

    fn solid_image(w: u32, h: u32, rgba: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba(rgba))
    }

    fn gradient_image(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| {
            Rgba([
                ((x * 7 + y * 3) % 256) as u8,
                ((x * 3 + y * 11) % 256) as u8,
                ((x * 5 + y * 5) % 256) as u8,
                255,
            ])
        })
    }

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lumen-export-tests-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir.join(name)
    }

    #[test]
    fn target_dimensions_match_export_js() {
        // full image, no options
        let t = target(4000, 3000, None, None, None);
        assert_eq!((t.out_w, t.out_h), (4000, 3000));
        assert_eq!((t.x, t.y, t.w, t.h), (0.0, 0.0, 4000.0, 3000.0));

        // max long edge
        let t = target(4000, 3000, None, Some(1000), None);
        assert_eq!((t.out_w, t.out_h), (1000, 750));

        // extra scale before max_dim
        let t = target(4000, 3000, None, None, Some(0.5));
        assert_eq!((t.out_w, t.out_h), (2000, 1500));
        let t = target(4000, 3000, None, Some(1000), Some(0.5));
        assert_eq!((t.out_w, t.out_h), (1000, 750));
        let t = target(4000, 3000, None, None, Some(2.0));
        assert_eq!((t.out_w, t.out_h), (8000, 6000));

        // portrait: long edge is the height
        let t = target(500, 1000, None, Some(400), None);
        assert_eq!((t.out_w, t.out_h), (200, 400));
    }

    #[test]
    fn target_crop_is_clamped_to_the_image() {
        let crop = CropRect::new(3900.0, 2900.0, 800.0, 800.0, None);
        let t = target(4000, 3000, Some(crop), None, None);
        assert_eq!((t.w, t.h), (100.0, 100.0));
        assert_eq!((t.out_w, t.out_h), (100, 100));

        // crop with negative origin
        let crop = CropRect::new(-40.0, -20.0, 200.0, 120.0, None);
        let t = target(1000, 800, Some(crop), None, None);
        assert_eq!((t.x, t.y, t.w, t.h), (0.0, 0.0, 160.0, 100.0));

        // degenerate crop (zero size) falls back to the whole image
        let crop = CropRect::new(10.0, 10.0, 0.0, 0.0, None);
        let t = target(1000, 800, Some(crop), None, None);
        assert_eq!((t.out_w, t.out_h), (1000, 800));

        // crop + max_dim + scale compose
        let crop = CropRect::new(100.0, 50.0, 800.0, 600.0, Some(4.0 / 3.0));
        let t = target(4000, 3000, Some(crop), Some(400), None);
        assert_eq!((t.out_w, t.out_h), (400, 300));
        let t = target(4000, 3000, Some(crop), None, Some(2.0));
        assert_eq!((t.out_w, t.out_h), (1600, 1200));
        // 1px floor
        let t = target(4000, 3000, None, Some(1), None);
        assert_eq!((t.out_w, t.out_h), (1, 1));
    }

    #[test]
    fn flatten_dimensions_for_crop_scale_max_dim() {
        let base = gradient_image(800, 600);
        let f = flatten_pixels(&base, &[], None, None, None);
        assert_eq!((f.image.width(), f.image.height()), (800, 600));
        assert_eq!((f.width, f.height), (800, 600));

        let crop = CropRect::new(100.0, 50.0, 400.0, 300.0, None);
        let f = flatten_pixels(&base, &[], Some(crop), None, None);
        assert_eq!((f.width, f.height), (400, 300));
        assert_eq!(f.image.dimensions(), (400, 300));

        let f = flatten_pixels(&base, &[], Some(crop), Some(200), None);
        assert_eq!((f.width, f.height), (200, 150));

        let f = flatten_pixels(&base, &[], Some(crop), None, Some(0.5));
        assert_eq!((f.width, f.height), (200, 150));

        let f = flatten_pixels(&base, &[], None, Some(400), None);
        assert_eq!((f.width, f.height), (400, 300));

        let f = flatten_pixels(&base, &[], None, None, Some(1.5));
        assert_eq!((f.width, f.height), (1200, 900));
    }

    #[test]
    fn flatten_matches_web_pixel_content_for_crop_and_scale() {
        // Crop the top-left quarter of a 2x2 quad image: the output must be the
        // red pixel even after a scale-up.
        let mut base = RgbaImage::new(2, 2);
        base.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        base.put_pixel(1, 0, Rgba([0, 255, 0, 255]));
        base.put_pixel(0, 1, Rgba([0, 0, 255, 255]));
        base.put_pixel(1, 1, Rgba([255, 255, 0, 255]));
        let crop = CropRect::new(0.0, 0.0, 1.0, 1.0, None);
        let f = flatten_pixels(&base, &[], Some(crop), None, Some(10.0));
        assert_eq!(f.image.dimensions(), (10, 10));
        let p = f.image.get_pixel(5, 5);
        assert_eq!((p[0], p[1], p[2]), (255, 0, 0), "crop picked the wrong pixel: {p:?}");
    }

    #[test]
    fn flatten_draws_annotations_relative_to_the_crop() {
        let base = solid_image(200, 200, [255, 255, 255, 255]);
        let style = Style { color: egui::Color32::from_rgb(255, 0, 0), size: 12.0, ..Default::default() };
        let annot = Annot::new(
            AnnotKind::Rect,
            vec![Pos2::new(20.0, 20.0), Pos2::new(80.0, 80.0)],
            style,
        );
        // Whole-image export: red stroke around (20,20)-(80,80).
        let f = flatten_pixels(&base, std::slice::from_ref(&annot), None, None, None);
        let p = f.image.get_pixel(50, 20); // top edge of the rect
        assert!(p[0] > 180 && p[1] < 120, "rect stroke missing at top edge: {p:?}");
        let p = f.image.get_pixel(5, 5);
        assert!(p[0] > 240 && p[1] > 240 && p[2] > 240, "outside must stay white: {p:?}");

        // Crop far away: the annotation is clipped out (no red anywhere).
        let crop = CropRect::new(120.0, 120.0, 60.0, 60.0, None);
        let f = flatten_pixels(&base, std::slice::from_ref(&annot), Some(crop), None, None);
        let red = f.image.pixels().any(|p| p[0] > 180 && p[1] < 120);
        assert!(!red, "annotation outside the crop must not paint");

        // Crop containing the annotation: it is re-drawn shifted into the crop.
        let crop = CropRect::new(10.0, 10.0, 100.0, 100.0, None);
        let f = flatten_pixels(&base, std::slice::from_ref(&annot), Some(crop), None, None);
        let p = f.image.get_pixel(40, 10); // (50,20) in image px -> (40,10) in crop px
        assert!(p[0] > 180 && p[1] < 120, "rect stroke not shifted with the crop: {p:?}");
    }

    #[test]
    fn flatten_draws_every_kind_without_panicking() {
        let base = solid_image(240, 180, [255, 255, 255, 255]);
        let style = Style { color: egui::Color32::from_rgb(0, 80, 255), size: 6.0, ..Default::default() };
        let mut objects = Vec::new();
        objects.push(Annot::new(
            AnnotKind::Line,
            vec![Pos2::new(10.0, 10.0), Pos2::new(100.0, 10.0)],
            Style { dashed: true, ..style },
        ));
        objects.push(Annot::new(
            AnnotKind::Arrow,
            vec![Pos2::new(10.0, 40.0), Pos2::new(120.0, 70.0)],
            style,
        ));
        objects.push(Annot::new(
            AnnotKind::Ellipse,
            vec![Pos2::new(140.0, 30.0), Pos2::new(220.0, 110.0)],
            Style { fill: true, ..style },
        ));
        objects.push(Annot::new(
            AnnotKind::Brush,
            vec![
                Pos2::new(20.0, 140.0),
                Pos2::new(50.0, 150.0),
                Pos2::new(80.0, 135.0),
                Pos2::new(110.0, 155.0),
            ],
            style,
        ));
        let f = flatten_pixels(&base, &objects, None, None, None);
        assert_eq!(f.image.dimensions(), (240, 180));
        let painted = f
            .image
            .pixels()
            .filter(|p| p[0] < 200 && p[2] > 180)
            .count();
        assert!(painted > 300, "expected painted pixels, got {painted}");
    }

    #[test]
    fn flatten_draws_cjk_text() {
        let base = solid_image(300, 120, [255, 255, 255, 255]);
        let style = Style {
            color: egui::Color32::from_rgb(0, 0, 0),
            font_size: 40.0,
            ..Default::default()
        };
        // Chars that are definitely in the UI subset font.
        let mut annot =
            Annot::new(AnnotKind::Text, vec![Pos2::new(10.0, 10.0)], style);
        annot.text = "圖片".to_string();
        let f = flatten_pixels(&base, std::slice::from_ref(&annot), None, None, None);
        let dark = f.image.pixels().filter(|p| p[0] < 100).count();
        assert!(dark > 200, "CJK text did not rasterise ({dark} dark px)");
    }

    #[test]
    fn export_text_uses_the_weight_600_face() {
        // The web canvas draws annotation text with font-weight 600; the export
        // must use the Bold subset, not the Regular one.
        let Some(_set) = fonts() else { return };

        fn ink(weight_600: bool) -> usize {
            let mut pix = tiny_skia::Pixmap::new(320, 140).expect("pixmap");
            let mut annot = Annot::new(
                AnnotKind::Text,
                vec![Pos2::new(8.0, 8.0)],
                Style {
                    color: egui::Color32::from_rgb(0, 0, 0),
                    font_size: 44.0,
                    ..Default::default()
                },
            );
            annot.text = "圖片 Lumen".to_string();
            draw_text_weighted(&mut pix, &annot, Transform::identity(), weight_600);
            pix.pixels().iter().map(|p| p.alpha() as usize).sum()
        }

        let bold = ink(true);
        let regular = ink(false);
        assert!(bold > 0 && regular > 0, "text failed to rasterise");
        assert!(
            bold as f32 > regular as f32 * 1.06,
            "export text is not weight 600: bold ink {bold} vs regular {regular}"
        );
    }

    #[test]
    fn text_falls_back_to_the_system_cjk_font() {
        // The UI subsets only carry the ~390 dictionary characters, so free-form
        // annotation text relies on the lazily loaded system Noto Sans CJK.
        let Some(set) = fonts() else { return };
        let ch = '測';
        if set.tc.glyph_id(ch).0 != 0 || set.sc.glyph_id(ch).0 != 0 {
            return; // subset happens to cover it: fallback not exercised here
        }
        if full_font(true).is_none() {
            return; // host without /usr/share/fonts/noto-cjk: nothing to assert
        }
        assert!(
            glyph_for(ch, true).is_some(),
            "system fallback font did not provide '測'"
        );
        let base = solid_image(260, 120, [255, 255, 255, 255]);
        let mut annot = Annot::new(
            AnnotKind::Text,
            vec![Pos2::new(10.0, 10.0)],
            Style {
                color: egui::Color32::from_rgb(0, 0, 0),
                font_size: 48.0,
                ..Default::default()
            },
        );
        annot.text = "測試".to_string();
        let f = flatten_pixels(&base, std::slice::from_ref(&annot), None, None, None);
        let dark = f.image.pixels().filter(|p| p[0] < 100).count();
        assert!(dark > 200, "fallback text did not rasterise ({dark} dark px)");
    }

    #[test]
    fn no_exif_app1_marker_in_jpeg_output() {
        let img = gradient_image(320, 240);
        let bytes = try_encode(&img, OutputFormat::Jpeg, 90).expect("jpeg encode");
        assert_eq!(&bytes[..2], &[0xFF, 0xD8], "JPEG must start with SOI");
        assert!(
            !bytes.windows(6).any(|w| w == b"Exif\0\0"),
            "fresh JPEG must not contain an Exif payload"
        );
        // Walk the marker segments: APP1 (0xE1) would be where EXIF lives.
        let mut i = 2;
        while i + 4 <= bytes.len() {
            if bytes[i] != 0xFF {
                break;
            }
            let marker = bytes[i + 1];
            if marker == 0xDA || marker == 0xD9 {
                break; // start of scan / end of image
            }
            let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
            assert_ne!(marker, 0xE1, "JPEG output must not carry an APP1 (Exif) segment");
            if len < 2 {
                break;
            }
            i += 2 + len;
        }
    }

    #[test]
    fn png_output_keeps_alpha_and_size() {
        let img = RgbaImage::from_pixel(40, 30, Rgba([10, 20, 30, 128]));
        let bytes = try_encode(&img, OutputFormat::Png, 90).expect("png encode");
        assert_eq!(&bytes[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        let decoded = image::load_from_memory(&bytes).expect("decode png").to_rgba8();
        assert_eq!(decoded.dimensions(), (40, 30));
        assert_eq!(decoded.get_pixel(0, 0)[3], 128);
    }

    #[test]
    fn webp_output_is_lossy_and_round_trips() {
        let img = gradient_image(64, 48);
        let bytes = try_encode(&img, OutputFormat::Webp, 90).expect("webp encode");
        assert!(bytes.len() > 12);
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WEBP");
        // Lossy -> "VP8 " (or "VP8X" when alpha is present), never "VP8L".
        let chunk = &bytes[12..16];
        assert!(
            chunk == b"VP8 " || chunk == b"VP8X",
            "expected lossy WEBP chunk, got {chunk:?}"
        );
        let decoded = image::load_from_memory(&bytes)
            .expect("decode lossy webp")
            .to_rgba8();
        assert_eq!(decoded.dimensions(), (64, 48));

        // Quality must actually change the payload size (i.e. it is lossy).
        let low = try_encode(&img, OutputFormat::Webp, 10).expect("webp q10");
        let high = try_encode(&img, OutputFormat::Webp, 95).expect("webp q95");
        assert!(
            high.len() > low.len(),
            "webp quality has no effect: q95 {} vs q10 {}",
            high.len(),
            low.len()
        );
    }

    #[test]
    fn webp_keeps_alpha() {
        let mut img = RgbaImage::from_pixel(48, 40, Rgba([200, 40, 40, 255]));
        for x in 0..24 {
            for y in 0..40 {
                img.put_pixel(x, y, Rgba([200, 40, 40, 0]));
            }
        }
        let bytes = try_encode(&img, OutputFormat::Webp, 90).expect("webp alpha");
        let decoded = image::load_from_memory(&bytes)
            .expect("decode webp alpha")
            .to_rgba8();
        assert!(decoded.get_pixel(4, 20)[3] < 16, "transparent area lost its alpha");
        assert!(decoded.get_pixel(40, 20)[3] > 240, "opaque area lost its alpha");
    }

    /// Deterministic pseudo-random detail: fine-grained noise that compresses
    /// like a busy photograph (lossy WebP at q95 ≈ 600 KB at 1200x900).
    fn noise_image(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| {
            let base_r = (x * 255 / w.max(1)) as i32;
            let base_g = (y * 255 / h.max(1)) as i32;
            let base_b = ((x + y) * 255 / (w + h).max(1)) as i32;
            let mut v = x
                .wrapping_mul(1_103_515_245)
                .wrapping_add(y.wrapping_mul(12_345));
            v ^= v >> 13;
            v = v.wrapping_mul(2_654_435_761);
            let n = ((v >> 16) & 0x3F) as i32 - 32; // +-32 of detail
            Rgba([
                (base_r + n).clamp(0, 255) as u8,
                (base_g + n).clamp(0, 255) as u8,
                (base_b + n).clamp(0, 255) as u8,
                255,
            ])
        })
    }

    #[test]
    fn webp_encode_to_target_fits_300kb() {
        let img = noise_image(1200, 900);
        let high = try_encode(&img, OutputFormat::Webp, 95).expect("webp q95");
        assert!(
            high.len() > 300 * 1024,
            "test fixture too compressible at q95 ({} bytes), cannot exercise the target",
            high.len()
        );
        let (bytes, quality) = encode_to_target_with_quality(&img, OutputFormat::Webp, 300);
        assert!(
            bytes.len() <= 300 * 1024,
            "webp payload {} > target {} bytes (quality {quality})",
            bytes.len(),
            300 * 1024
        );
        assert_eq!(&bytes[..4], b"RIFF");
        assert!((1..=100).contains(&quality));
        // and the result still decodes
        let decoded = image::load_from_memory(&bytes).expect("decode target webp");
        assert_eq!((decoded.width(), decoded.height()), (1200, 900));
    }

    #[test]
    fn webp_target_unreachable_returns_the_smallest_fallback() {
        // Pure noise cannot reach 300 KB at any probed quality: the contract is
        // the same as the web build (and JPEG) — return the smallest candidate,
        // never panic, and stay below the q90 probe.
        let img = RgbaImage::from_fn(1600, 1200, |x, y| {
            let mut v = x
                .wrapping_mul(1_103_515_245)
                .wrapping_add(y.wrapping_mul(12_345));
            v ^= v >> 13;
            v = v.wrapping_mul(2_654_435_761);
            Rgba([
                (v & 0xFF) as u8,
                ((v >> 8) & 0xFF) as u8,
                ((v >> 16) & 0xFF) as u8,
                255,
            ])
        });
        let q90 = try_encode(&img, OutputFormat::Webp, 90).expect("q90");
        assert!(q90.len() > 300 * 1024);
        let (bytes, quality) = encode_to_target_with_quality(&img, OutputFormat::Webp, 300);
        assert!(!bytes.is_empty());
        assert_eq!(&bytes[..4], b"RIFF");
        assert!(bytes.len() <= q90.len(), "should return the smallest candidate");
        assert!((1..=100).contains(&quality));
    }

    #[test]
    fn jpeg_white_background_flattens_alpha() {
        // Fully transparent pixels must come out white, not black.
        let img = RgbaImage::from_pixel(16, 16, Rgba([0, 0, 0, 0]));
        let bytes = try_encode(&img, OutputFormat::Jpeg, 95).expect("jpeg");
        let decoded = image::load_from_memory(&bytes).expect("decode jpeg").to_rgb8();
        let p = decoded.get_pixel(8, 8);
        assert!(p[0] > 240 && p[1] > 240 && p[2] > 240, "transparent -> white expected: {p:?}");
    }

    #[test]
    fn encode_to_target_fits_the_requested_kb() {
        let img = gradient_image(512, 384);
        // Pick a target a little above a known-small quality so a fit exists.
        let baseline = try_encode(&img, OutputFormat::Jpeg, 15).expect("jpeg").len();
        let target_kb = ((baseline + 4096) as u64).div_ceil(1024) as u32;
        let (bytes, quality) = encode_to_target_with_quality(&img, OutputFormat::Jpeg, target_kb);
        assert!(
            bytes.len() <= target_kb as usize * 1024,
            "payload {} > target {} KB (quality {quality})",
            bytes.len(),
            target_kb
        );
        assert!(quality >= 1 && quality <= 100);
        // The result must still be a valid JPEG.
        assert_eq!(&bytes[..2], &[0xFF, 0xD8]);
    }

    #[test]
    fn encode_to_target_returns_smallest_when_impossible() {
        let img = gradient_image(400, 300);
        // 1 KB is impossible for a 400x300 photo; the web build returns the
        // smallest candidate instead of failing.
        let bytes = encode_to_target(&img, OutputFormat::Jpeg, 1);
        assert!(!bytes.is_empty());
        assert_eq!(&bytes[..2], &[0xFF, 0xD8]);
    }

    #[test]
    fn flatten_reads_the_full_resolution_file() {
        let dir = temp_path("doc.png");
        let img = gradient_image(120, 90);
        img.save(&dir).expect("write test png");
        let doc = Doc::load(&dir, LoadOptions { max_pixels: 64 }).expect("load doc");
        // Preview decode is tiny, but flatten must still use 120x90.
        let f = flatten(&doc, &[], None, None, None);
        assert_eq!((f.width, f.height), (120, 90));
        assert_eq!(f.image.dimensions(), (120, 90));
        // and honour crop + max_dim on the full-res pixels
        let crop = CropRect::new(10.0, 10.0, 100.0, 70.0, None);
        let f = flatten(&doc, &[], Some(crop), Some(50), None);
        assert_eq!((f.width, f.height), (50, 35));
        let _ = std::fs::remove_file(&dir);
    }

    #[test]
    fn estimate_size_is_cheap_and_plausible() {
        let dir = temp_path("estimate.png");
        let img = gradient_image(200, 150);
        img.save(&dir).expect("write test png");
        let doc = Doc::load(&dir, LoadOptions { max_pixels: 10_000 }).expect("load doc");
        let out = OutputSettings {
            format: OutputFormat::Jpeg,
            quality: 90,
            max_dim: Some(100),
            scale: None,
            target_kb: None,
        };
        let est = estimate_size(&doc, &[], &out, None);
        assert_eq!((est.width, est.height), (100, 75));
        assert!(est.bytes > 0);
        let real = flatten(&doc, &[], None, Some(100), None);
        let real_bytes = encode(&real.image, OutputFormat::Jpeg, 90).len() as u64;
        // half-res extrapolation is only an estimate: stay within 4x either way
        assert!(
            est.bytes * 4 >= real_bytes && est.bytes <= real_bytes * 4,
            "estimate {} vs real {}",
            est.bytes,
            real_bytes
        );
        let _ = std::fs::remove_file(&dir);
    }
}

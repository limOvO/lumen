// Annotation object model, rasterisation helpers and hit testing.
// Coordinate system: image pixels (the source image), independent of zoom.
// Owner: annotations teammate. Keep this file self-contained.
//
// Parity reference: src/js/core/annotate.js. Geometry constants match it:
//   dash pattern        = size * 2.4 / size * 1.6
//   arrow head length   = max(size * 3.2, size + 6)
//   arrow neck          = min(len * 0.6, head * 0.85)
//   brush start width   = max(0.65 * size / 8, size * 0.82)
//   brush width ramp    = prev + (target - prev) * 0.38

use egui::{Color32, Painter, Pos2, Rect, Stroke, Vec2};

/// Two endpoints of a line/arrow may never be closer than this (image px).
pub const MIN_ARROW_LEN: f32 = 2.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnnotKind {
    Brush,
    Line,
    Rect,
    Ellipse,
    Arrow,
    Text,
}

impl AnnotKind {
    pub fn is_linear(self) -> bool {
        matches!(self, AnnotKind::Line | AnnotKind::Arrow)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Style {
    pub color: Color32,
    pub size: f32,
    pub opacity: f32,
    pub dashed: bool,
    pub fill: bool,
    pub font_size: f32,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            color: Color32::from_rgb(255, 92, 92),
            size: 8.0,
            opacity: 1.0,
            dashed: false,
            fill: false,
            font_size: 36.0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Handle {
    Start,
    End,
    Nw,
    N,
    Ne,
    E,
    Se,
    S,
    Sw,
    W,
}

#[derive(Clone, Debug)]
pub struct Annot {
    pub id: u64,
    pub kind: AnnotKind,
    /// Image-space points: two endpoints for line/arrow, the full stroke for
    /// brush, `[min, max]` for rect/ellipse, `[top-left]` for text.
    pub points: Vec<Pos2>,
    /// Per-point brush width in image px (parallel to `points`; empty for the
    /// other kinds, where `style.size` is the width).
    pub widths: Vec<f32>,
    pub text: String,
    pub style: Style,
}

fn next_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static IDS: AtomicU64 = AtomicU64::new(1);
    IDS.fetch_add(1, Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Pure geometry constants/helpers (shared with interaction.rs and tests).
// ---------------------------------------------------------------------------

/// Dash / gap lengths in image px for a stroke of `size`.
pub fn dash_pattern(size: f32) -> (f32, f32) {
    let w = size.max(1.0);
    (w * 2.4, w * 1.6)
}

/// Arrow head length in image px.
pub fn arrow_head_length(size: f32) -> f32 {
    (size * 3.2).max(size + 6.0)
}

/// How much of the shaft to hide behind the arrow head (image px).
pub fn arrow_neck(length: f32, head: f32) -> f32 {
    (length * 0.6).min(head * 0.85)
}

/// Width of the very first brush point.
pub fn initial_brush_width(size: f32) -> f32 {
    ((0.65 * size) / 8.0).max(size * 0.82)
}

/// Next brush width from pointer velocity (screen px / ms): fast = thin,
/// slow = thick, smoothed 38% towards the target so the taper is gradual.
pub fn brush_width(prev: f32, size: f32, velocity: f32) -> f32 {
    let target = size * (1.0 - (velocity / 1.5).min(1.0) * 0.82);
    ((0.65 * size) / 8.0).max(prev + (target - prev) * 0.38)
}

/// Keep an arrow endpoint at least `MIN_ARROW_LEN` away from the other end so
/// dragging one end past the other cannot flip/flap.
pub fn clamp_arrow_endpoint(fixed: Pos2, target: Pos2) -> Pos2 {
    let d = target - fixed;
    let len = d.length();
    if len >= MIN_ARROW_LEN {
        return target;
    }
    if len < 1e-6 {
        return Pos2::new(fixed.x + MIN_ARROW_LEN, fixed.y);
    }
    fixed + d / len * MIN_ARROW_LEN
}

pub fn point_segment_distance(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_sq();
    if len2 <= f32::EPSILON {
        return (p - a).length();
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
}

/// Approximate per-character advance as a fraction of the font size.
/// (egui cannot measure text from a free function without a `Context`; this
/// table is deterministic so geometry/hit testing/export can share it.)
fn char_width_ratio(c: char) -> f32 {
    match c {
        ' ' => 0.30,
        '\t' => 0.60,
        '\u{3000}' => 1.0,
        c if c.is_ascii_uppercase() => 0.66,
        c if c.is_ascii_lowercase() => 0.53,
        c if c.is_ascii_digit() => 0.58,
        c if c.is_ascii_punctuation() => 0.42,
        c if is_wide(c) => 1.0,
        _ => 0.62,
    }
}

fn is_wide(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x115F
        | 0x2E80..=0x303E
        | 0x3041..=0x33FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xA000..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE4F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1FAFF)
}

pub fn text_line_width(font_size: f32, line: &str) -> f32 {
    line.chars().map(|c| char_width_ratio(c) * font_size).sum()
}

/// Measured text box in image px (multi-line aware, line height = 1.28 * font).
pub fn measure_text(font_size: f32, text: &str) -> Vec2 {
    let size = font_size.max(0.0);
    let mut width = 0.0f32;
    let mut lines = 0usize;
    for line in text.split('\n') {
        width = width.max(text_line_width(size, line));
        lines += 1;
    }
    let lines = lines.max(1) as f32;
    Vec2::new(width.max(size * 0.5), (lines * size * 1.28).max(size))
}

// ---------------------------------------------------------------------------
// Constructors
// ---------------------------------------------------------------------------

impl Annot {
    pub fn new(kind: AnnotKind, points: Vec<Pos2>, style: Style) -> Self {
        let mut annot =
            Self { id: next_id(), kind, points, widths: Vec::new(), text: String::new(), style };
        if kind == AnnotKind::Brush {
            let w = initial_brush_width(annot.style.size);
            annot.widths = vec![w; annot.points.len()];
        }
        annot
    }

    /// Start a new brush stroke at `start` (image px).
    pub fn brush(start: Pos2, style: Style) -> Self {
        Self::new(AnnotKind::Brush, vec![start], style)
    }

    pub fn text(start: Pos2, text: impl Into<String>, style: Style) -> Self {
        let mut a = Self::new(AnnotKind::Text, vec![start], style);
        a.text = text.into();
        a
    }

    /// Append a brush sample with an explicit width (image px).
    pub fn push_brush_point(&mut self, p: Pos2, width: f32) {
        if self.kind != AnnotKind::Brush {
            return;
        }
        self.points.push(p);
        let w = width.max(0.1);
        while self.widths.len() + 1 < self.points.len() {
            self.widths.push(w);
        }
        self.widths.push(w);
    }

    /// Width of brush point `i` (falls back to `style.size`).
    pub fn point_width(&self, i: usize) -> f32 {
        self.widths.get(i).copied().unwrap_or(self.style.size).max(0.1)
    }

    fn text_size(&self) -> Vec2 {
        measure_text(self.style.font_size, &self.text)
    }

    pub fn bounds(&self) -> Rect {
        match self.kind {
            AnnotKind::Brush => {
                if self.points.is_empty() {
                    return Rect::NOTHING;
                }
                let mut min = Pos2::new(f32::INFINITY, f32::INFINITY);
                let mut max = Pos2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
                for (i, p) in self.points.iter().enumerate() {
                    let half = (self.point_width(i) * 0.5).max(0.35);
                    min.x = min.x.min(p.x - half);
                    min.y = min.y.min(p.y - half);
                    max.x = max.x.max(p.x + half);
                    max.y = max.y.max(p.y + half);
                }
                let w = (max.x - min.x).max(1.0);
                let h = (max.y - min.y).max(1.0);
                Rect::from_min_size(min, Vec2::new(w, h))
            }
            AnnotKind::Text => {
                let p = self.points.first().copied().unwrap_or(Pos2::ZERO);
                Rect::from_min_size(p, self.text_size())
            }
            AnnotKind::Line | AnnotKind::Arrow => {
                if self.points.len() < 2 {
                    return Rect::NOTHING;
                }
                let a = self.points[0];
                let b = self.points[1];
                Rect::from_min_size(
                    Pos2::new(a.x.min(b.x), a.y.min(b.y)),
                    Vec2::new((b.x - a.x).abs().max(1.0), (b.y - a.y).abs().max(1.0)),
                )
            }
            AnnotKind::Rect | AnnotKind::Ellipse => {
                if self.points.len() < 2 {
                    return Rect::NOTHING;
                }
                Rect::from_two_pos(self.points[0], self.points[1])
            }
        }
    }

    pub fn handles(&self) -> Vec<(Handle, Pos2)> {
        if self.kind.is_linear() {
            if self.points.len() >= 2 {
                return vec![(Handle::Start, self.points[0]), (Handle::End, self.points[1])];
            }
            return Vec::new();
        }
        let b = self.bounds();
        vec![
            (Handle::Nw, b.left_top()),
            (Handle::N, b.center_top()),
            (Handle::Ne, b.right_top()),
            (Handle::E, b.right_center()),
            (Handle::Se, b.right_bottom()),
            (Handle::S, b.center_bottom()),
            (Handle::Sw, b.left_bottom()),
            (Handle::W, b.left_center()),
        ]
    }

    /// Hit test in image space; `tol` is already converted to image px.
    pub fn hit(&self, pt: Pos2, tol: f32) -> bool {
        let st = self.style;
        match self.kind {
            AnnotKind::Brush => {
                if self.points.is_empty() {
                    return false;
                }
                if self.points.len() == 1 {
                    return (pt - self.points[0]).length() <= tol + self.point_width(0) / 2.0;
                }
                for i in 1..self.points.len() {
                    let a = self.points[i - 1];
                    let b = self.points[i];
                    let r = tol + self.point_width(i - 1).max(self.point_width(i)) / 2.0;
                    if point_segment_distance(pt, a, b) <= r {
                        return true;
                    }
                }
                false
            }
            AnnotKind::Text => {
                let b = self.bounds();
                pt.x >= b.left() - tol
                    && pt.x <= b.right() + tol
                    && pt.y >= b.top() - tol
                    && pt.y <= b.bottom() + tol
            }
            AnnotKind::Rect => {
                let b = self.bounds();
                let pad = tol + st.size / 2.0;
                let inside_x = pt.x >= b.left() - pad && pt.x <= b.right() + pad;
                let inside_y = pt.y >= b.top() - pad && pt.y <= b.bottom() + pad;
                if !inside_x || !inside_y {
                    return false;
                }
                if st.fill {
                    return true;
                }
                let core_x = pt.x >= b.left() + pad && pt.x <= b.right() - pad;
                let core_y = pt.y >= b.top() + pad && pt.y <= b.bottom() - pad;
                !(core_x && core_y)
            }
            AnnotKind::Ellipse => {
                let b = self.bounds();
                let rx = (b.width() / 2.0).max(0.5);
                let ry = (b.height() / 2.0).max(0.5);
                let c = b.center();
                let nx = (pt.x - c.x) / rx;
                let ny = (pt.y - c.y) / ry;
                let r = (nx * nx + ny * ny).sqrt();
                if r <= 1.0 && st.fill {
                    return true;
                }
                let band = (tol + st.size / 2.0) / rx.min(ry).max(0.5);
                (r - 1.0).abs() <= band
            }
            AnnotKind::Line | AnnotKind::Arrow => {
                if self.points.len() < 2 {
                    return false;
                }
                let a = self.points[0];
                let b = self.points[1];
                if point_segment_distance(pt, a, b) <= tol + st.size / 2.0 {
                    return true;
                }
                if self.kind != AnnotKind::Arrow {
                    return false;
                }
                (pt - b).length() <= arrow_head_length(st.size) * 0.55
            }
        }
    }

    pub fn move_by(&mut self, delta: Vec2) {
        for p in &mut self.points {
            *p += delta;
        }
    }

    /// Resize by dragging `handle` to `pt` (image px). `keep_ratio` locks the
    /// original aspect ratio (corner handles follow the larger relative change,
    /// edge handles derive the other axis), anchored at the opposite edge.
    pub fn resize(&mut self, handle: Handle, pt: Pos2, keep_ratio: bool) {
        if self.kind.is_linear() && matches!(handle, Handle::Start | Handle::End) {
            // Arrows/lines only have two control points: move just that end.
            if self.points.len() >= 2 {
                let i = if handle == Handle::Start { 0 } else { 1 };
                let fixed = self.points[1 - i];
                self.points[i] = clamp_arrow_endpoint(fixed, pt);
            }
            return;
        }
        let b = self.bounds();
        let base_w = b.width().max(1.0);
        let base_h = b.height().max(1.0);
        let dw = matches!(handle, Handle::Nw | Handle::W | Handle::Sw);
        let de = matches!(handle, Handle::Ne | Handle::E | Handle::Se);
        let dn = matches!(handle, Handle::Nw | Handle::N | Handle::Ne);
        let ds = matches!(handle, Handle::Sw | Handle::S | Handle::Se);

        let mut left = b.left();
        let mut top = b.top();
        let mut width = base_w;
        let mut height = base_h;

        if dw {
            let right = b.left() + base_w;
            left = pt.x.min(right - 1.0);
            width = right - left;
        } else if de {
            width = (pt.x - b.left()).max(1.0);
        }
        if dn {
            let bottom = b.top() + base_h;
            top = pt.y.min(bottom - 1.0);
            height = bottom - top;
        } else if ds {
            height = (pt.y - b.top()).max(1.0);
        }

        if keep_ratio {
            let ratio = base_w / base_h;
            let is_corner = (dw || de) && (dn || ds);
            if is_corner {
                let s = (width / base_w).max(height / base_h);
                width = base_w * s;
                height = base_h * s;
                if dw {
                    left = b.left() + base_w - width;
                }
                if dn {
                    top = b.top() + base_h - height;
                }
            } else if dn || ds {
                width = height * ratio;
                left = if dw { b.left() + base_w - width } else { b.left() };
            } else if dw || de {
                height = width / ratio;
                top = if dn { b.top() + base_h - height } else { b.top() };
            }
        }

        width = width.max(1.0);
        height = height.max(1.0);

        match self.kind {
            AnnotKind::Brush => {
                let sx = width / base_w;
                let sy = height / base_h;
                let sw = (sx + sy) / 2.0;
                for (i, p) in self.points.iter_mut().enumerate() {
                    *p = Pos2::new(left + (p.x - b.left()) * sx, top + (p.y - b.top()) * sy);
                    if let Some(wd) = self.widths.get_mut(i) {
                        *wd = (*wd * sw).max(0.65);
                    }
                }
            }
            AnnotKind::Text => {
                self.style.font_size = (self.style.font_size * (height / base_h)).max(4.0);
                self.points = vec![Pos2::new(left, top)];
                while self.widths.len() > self.points.len() {
                    self.widths.pop();
                }
            }
            _ => {
                self.points = vec![Pos2::new(left, top), Pos2::new(left + width, top + height)];
                self.widths.clear();
            }
        }
    }

    /// Draw in image space; `to_screen` maps image px to screen points and
    /// `zoom` is screen px per image px.
    pub fn draw(&self, painter: &Painter, to_screen: &dyn Fn(Pos2) -> Pos2, zoom: f32) {
        if self.points.is_empty() {
            return;
        }
        let st = self.style;
        let color = st.color.gamma_multiply(st.opacity.clamp(0.0, 1.0));
        let stroke_w = (st.size * zoom).max(0.6);
        let stroke = Stroke::new(stroke_w, color);
        let (dash, gap) = dash_pattern(st.size);
        let (dash, gap) = (dash * zoom, gap * zoom);

        match self.kind {
            AnnotKind::Brush => {
                if st.dashed {
                    let path: Vec<Pos2> = self.points.iter().map(|p| to_screen(*p)).collect();
                    let avg = self.widths.iter().sum::<f32>()
                        / self.widths.len().max(1) as f32;
                    painter.extend(egui::Shape::dashed_line(
                        &path,
                        Stroke::new((avg * zoom).max(0.6), color),
                        dash.max(1.0),
                        gap.max(1.0),
                    ));
                    return;
                }
                for (i, p) in self.points.iter().enumerate() {
                    let sp = to_screen(*p);
                    let w = (self.point_width(i) * zoom).max(0.6);
                    painter.circle_filled(sp, (w / 2.0).max(0.25), color);
                    if i > 0 {
                        let prev = to_screen(self.points[i - 1]);
                        let pw = (self.point_width(i - 1) * zoom).max(0.6);
                        painter.line_segment([prev, sp], Stroke::new((pw + w) / 2.0, color));
                    }
                }
            }
            AnnotKind::Line => {
                if self.points.len() < 2 {
                    return;
                }
                let a = to_screen(self.points[0]);
                let b = to_screen(self.points[1]);
                if (b - a).length() < 0.01 {
                    return;
                }
                if st.dashed {
                    painter.extend(egui::Shape::dashed_line(
                        &[a, b],
                        stroke,
                        dash.max(1.0),
                        gap.max(1.0),
                    ));
                } else {
                    painter.line_segment([a, b], stroke);
                }
            }
            AnnotKind::Arrow => {
                if self.points.len() < 2 {
                    return;
                }
                let a = to_screen(self.points[0]);
                let b = to_screen(self.points[1]);
                let v = b - a;
                let len = v.length();
                if len < 0.01 {
                    return;
                }
                let dir = v / len;
                let normal = Vec2::new(-dir.y, dir.x);
                let head = arrow_head_length(st.size) * zoom;
                let neck = arrow_neck(len, head);
                let shaft_end = b - dir * neck;
                if st.dashed {
                    painter.extend(egui::Shape::dashed_line(
                        &[a, shaft_end],
                        stroke,
                        dash.max(1.0),
                        gap.max(1.0),
                    ));
                } else {
                    painter.line_segment([a, shaft_end], stroke);
                }
                let half = head * 0.42;
                let back = b - dir * head;
                painter.add(egui::Shape::convex_polygon(
                    vec![b, back + normal * half, back - normal * half],
                    color,
                    Stroke::NONE,
                ));
            }
            AnnotKind::Rect => {
                if self.points.len() < 2 {
                    return;
                }
                let r = Rect::from_two_pos(to_screen(self.points[0]), to_screen(self.points[1]));
                if st.fill {
                    painter.rect_filled(r, 0.0, color);
                }
                if st.dashed {
                    let pts = vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
                    painter.extend(egui::Shape::dashed_line(
                        &pts,
                        stroke,
                        dash.max(1.0),
                        gap.max(1.0),
                    ));
                } else {
                    painter.rect_stroke(r, 0.0, stroke, egui::StrokeKind::Middle);
                }
            }
            AnnotKind::Ellipse => {
                if self.points.len() < 2 {
                    return;
                }
                let r = Rect::from_two_pos(to_screen(self.points[0]), to_screen(self.points[1]));
                let mut pts = Vec::with_capacity(65);
                for i in 0..64 {
                    let t = i as f32 / 64.0 * std::f32::consts::TAU;
                    pts.push(Pos2::new(
                        r.center().x + r.width() / 2.0 * t.cos(),
                        r.center().y + r.height() / 2.0 * t.sin(),
                    ));
                }
                if st.fill {
                    painter.add(egui::Shape::convex_polygon(pts.clone(), color, Stroke::NONE));
                }
                if st.dashed {
                    let mut path = pts;
                    path.push(path[0]);
                    painter.extend(egui::Shape::dashed_line(
                        &path,
                        stroke,
                        dash.max(1.0),
                        gap.max(1.0),
                    ));
                } else {
                    painter.add(egui::Shape::closed_line(pts, stroke));
                }
            }
            AnnotKind::Text => {
                let Some(p) = self.points.first() else { return };
                let pos = to_screen(*p);
                let font_size = (st.font_size * zoom).max(1.0);
                let font = egui::FontId::proportional(font_size);
                let line_h = font_size * 1.28;
                for (i, line) in self.text.split('\n').enumerate() {
                    if line.is_empty() {
                        continue;
                    }
                    painter.text(
                        Pos2::new(pos.x, pos.y + i as f32 * line_h),
                        egui::Align2::LEFT_TOP,
                        line,
                        font.clone(),
                        color,
                    );
                }
            }
        }
    }
}

/// Topmost object under an image-space point (`tol` in image px).
pub fn hit_topmost(objects: &[Annot], pt: Pos2, tol: f32) -> Option<usize> {
    objects.iter().enumerate().rev().find(|(_, o)| o.hit(pt, tol)).map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) {
        assert!((a - b).abs() < 1e-4, "{a} != {b}");
    }

    fn style(size: f32) -> Style {
        Style { size, ..Style::default() }
    }

    // --- point / segment ---------------------------------------------------

    #[test]
    fn point_segment_distance_perpendicular() {
        let d = point_segment_distance(Pos2::new(50.0, 5.0), Pos2::ZERO, Pos2::new(100.0, 0.0));
        approx(d, 5.0);
    }

    #[test]
    fn point_segment_distance_clamps_to_endpoints() {
        let a = Pos2::new(0.0, 0.0);
        let b = Pos2::new(10.0, 0.0);
        approx(point_segment_distance(Pos2::new(-3.0, 4.0), a, b), 5.0);
        approx(point_segment_distance(Pos2::new(13.0, 4.0), a, b), 5.0);
        // degenerate segment behaves like a point
        approx(point_segment_distance(Pos2::new(3.0, 4.0), a, a), 5.0);
    }

    // --- constants ---------------------------------------------------------

    #[test]
    fn arrow_head_length_matches_web() {
        // max(size*3.2, size+6)
        approx(arrow_head_length(8.0), 25.6);
        approx(arrow_head_length(2.0), 8.0);
        approx(arrow_head_length(1.0), 7.0);
    }

    #[test]
    fn arrow_neck_is_the_shorter_of_the_two_limits() {
        approx(arrow_neck(100.0, 25.6), 21.76);
        approx(arrow_neck(10.0, 25.6), 6.0);
    }

    #[test]
    fn dash_pattern_is_proportional_to_size() {
        let (d, g) = dash_pattern(8.0);
        approx(d, 19.2);
        approx(g, 12.8);
        let (d, g) = dash_pattern(0.0);
        approx(d, 2.4);
        approx(g, 1.6);
    }

    #[test]
    fn brush_width_ramps_fast_thin_slow_thick() {
        let size = 8.0;
        let start = initial_brush_width(size);
        approx(start, 6.56);
        // slow pointer: target 8, 38% smoothing
        let slow = brush_width(start, size, 0.0);
        approx(slow, start + (size - start) * 0.38);
        // fast pointer saturates the taper: target size*(1-0.82)
        let fast = brush_width(start, size, 2.0);
        approx(fast, start + (size * 0.18 - start) * 0.38);
        assert!(fast < slow);
        // clamped at the minimum
        let floor = brush_width(0.0, size, 9.0);
        assert!(floor >= (0.65 * size) / 8.0 - 1e-5);
    }

    #[test]
    fn arrow_endpoint_is_clamped_to_min_length() {
        let fixed = Pos2::new(10.0, 10.0);
        let p = clamp_arrow_endpoint(fixed, Pos2::new(10.5, 10.5));
        approx((p - fixed).length(), MIN_ARROW_LEN);
        let far = Pos2::new(40.0, 10.0);
        assert_eq!(clamp_arrow_endpoint(fixed, far), far);
    }

    // --- handles -----------------------------------------------------------

    #[test]
    fn linear_objects_have_two_handles_shapes_have_eight() {
        let line = Annot::new(
            AnnotKind::Line,
            vec![Pos2::new(0.0, 0.0), Pos2::new(50.0, 50.0)],
            style(8.0),
        );
        assert_eq!(line.handles().len(), 2);
        let arrow = Annot::new(
            AnnotKind::Arrow,
            vec![Pos2::new(0.0, 0.0), Pos2::new(50.0, 50.0)],
            style(8.0),
        );
        assert_eq!(arrow.handles().len(), 2);
        for kind in [AnnotKind::Rect, AnnotKind::Ellipse] {
            let obj = Annot::new(kind, vec![Pos2::new(0.0, 0.0), Pos2::new(50.0, 50.0)], style(8.0));
            assert_eq!(obj.handles().len(), 8, "{kind:?}");
        }
        let brush = Annot::new(
            AnnotKind::Brush,
            vec![Pos2::new(10.0, 10.0), Pos2::new(40.0, 20.0)],
            style(8.0),
        );
        assert_eq!(brush.handles().len(), 8);
        let text = Annot::text(Pos2::new(10.0, 10.0), "hello", Style::default());
        assert_eq!(text.handles().len(), 8);
    }

    // --- bounds / hit testing ---------------------------------------------

    #[test]
    fn bounds_cover_line_and_brush_stroke() {
        let line = Annot::new(
            AnnotKind::Line,
            vec![Pos2::new(10.0, 20.0), Pos2::new(110.0, 20.0)],
            style(8.0),
        );
        let b = line.bounds();
        approx(b.left(), 10.0);
        approx(b.right(), 110.0);
        approx(b.top(), 20.0);
        approx(b.height(), 1.0);

        // brush bounds include half the per-point width
        let brush = Annot::new(
            AnnotKind::Brush,
            vec![Pos2::new(10.0, 10.0), Pos2::new(30.0, 10.0)],
            style(8.0),
        );
        let b = brush.bounds();
        approx(b.left(), 10.0 - initial_brush_width(8.0) / 2.0);
        approx(b.right(), 30.0 + initial_brush_width(8.0) / 2.0);
    }

    #[test]
    fn line_hit_uses_tolerance_plus_half_width() {
        let line = Annot::new(
            AnnotKind::Line,
            vec![Pos2::new(0.0, 0.0), Pos2::new(100.0, 0.0)],
            style(8.0),
        );
        // tol 2 + size/2 4 => 6
        assert!(line.hit(Pos2::new(50.0, 5.0), 2.0));
        assert!(!line.hit(Pos2::new(50.0, 7.0), 2.0));
        assert!(!line.hit(Pos2::new(-20.0, 0.0), 2.0));
    }

    #[test]
    fn arrow_tip_hit_extends_into_the_head() {
        let arrow = Annot::new(
            AnnotKind::Arrow,
            vec![Pos2::new(0.0, 0.0), Pos2::new(100.0, 0.0)],
            style(8.0),
        );
        let head = arrow_head_length(8.0); // 25.6
        assert!(arrow.hit(Pos2::new(100.0 + head * 0.5, 0.0), 0.0));
        assert!(!arrow.hit(Pos2::new(100.0 + head * 0.7, 0.0), 0.0));
    }

    #[test]
    fn rect_hit_is_hollow_until_filled() {
        let mut rect = Annot::new(
            AnnotKind::Rect,
            vec![Pos2::new(10.0, 10.0), Pos2::new(110.0, 60.0)],
            style(8.0),
        );
        // pad = tol(2) + size/2(4) = 6
        assert!(rect.hit(Pos2::new(10.0, 35.0), 2.0)); // on the left edge
        assert!(!rect.hit(Pos2::new(60.0, 35.0), 2.0)); // hollow middle
        rect.style.fill = true;
        assert!(rect.hit(Pos2::new(60.0, 35.0), 2.0));
        assert!(!rect.hit(Pos2::new(2.0, 35.0), 2.0)); // outside even with pad
    }

    #[test]
    fn ellipse_hit_tracks_the_rim_band() {
        let ellipse = Annot::new(
            AnnotKind::Ellipse,
            vec![Pos2::new(10.0, 10.0), Pos2::new(110.0, 60.0)],
            style(8.0),
        );
        // rx=50 ry=25, centre (60,35): top of the rim
        assert!(ellipse.hit(Pos2::new(60.0, 10.0), 2.0));
        assert!(!ellipse.hit(Pos2::new(60.0, 35.0), 2.0));
        assert!(ellipse.hit(Pos2::new(60.0, 10.0 + 3.0), 2.0));
    }

    #[test]
    fn text_bounds_follow_font_size_and_wide_chars() {
        let small = Annot::text(Pos2::ZERO, "Hi", style(8.0));
        let big = Annot::text(Pos2::ZERO, "Hi", Style { font_size: 72.0, ..Style::default() });
        assert!(big.bounds().width() > small.bounds().width());
        let cjk = Annot::text(Pos2::ZERO, "文字", Style { font_size: 36.0, ..Style::default() });
        approx(cjk.bounds().width(), 72.0);
        let two_lines = Annot::text(Pos2::ZERO, "a\nb", Style { font_size: 36.0, ..Style::default() });
        approx(two_lines.bounds().height(), 36.0 * 1.28 * 2.0);
    }

    #[test]
    fn hit_topmost_prefers_the_last_object() {
        let a = Annot::new(AnnotKind::Rect, vec![Pos2::new(0.0, 0.0), Pos2::new(100.0, 100.0)], style(4.0));
        let b = Annot::new(AnnotKind::Rect, vec![Pos2::new(0.0, 0.0), Pos2::new(100.0, 100.0)], style(4.0));
        let objects = vec![a, b];
        let idx = hit_topmost(&objects, Pos2::new(0.0, 50.0), 1.0);
        assert_eq!(idx, Some(1));
        assert_eq!(hit_topmost(&objects, Pos2::new(400.0, 400.0), 1.0), None);
    }

    // --- move / resize anchoring ------------------------------------------

    #[test]
    fn move_by_offsets_all_points() {
        let mut line = Annot::new(
            AnnotKind::Line,
            vec![Pos2::new(0.0, 0.0), Pos2::new(10.0, 10.0)],
            style(8.0),
        );
        line.move_by(Vec2::new(5.0, -3.0));
        assert_eq!(line.points[0], Pos2::new(5.0, -3.0));
        assert_eq!(line.points[1], Pos2::new(15.0, 7.0));
    }

    #[test]
    fn resize_se_moves_only_the_bottom_right_corner() {
        let mut rect = Annot::new(
            AnnotKind::Rect,
            vec![Pos2::new(10.0, 20.0), Pos2::new(110.0, 70.0)],
            style(8.0),
        );
        rect.resize(Handle::Se, Pos2::new(200.0, 100.0), false);
        let b = rect.bounds();
        approx(b.left(), 10.0);
        approx(b.top(), 20.0);
        approx(b.right(), 200.0);
        approx(b.bottom(), 100.0);
    }

    #[test]
    fn resize_nw_anchors_the_bottom_right_corner() {
        let mut rect = Annot::new(
            AnnotKind::Rect,
            vec![Pos2::new(10.0, 20.0), Pos2::new(110.0, 70.0)],
            style(8.0),
        );
        rect.resize(Handle::Nw, Pos2::new(50.0, 40.0), false);
        let b = rect.bounds();
        approx(b.left(), 50.0);
        approx(b.top(), 40.0);
        approx(b.right(), 110.0);
        approx(b.bottom(), 70.0);
    }

    #[test]
    fn resize_west_handle_keeps_the_right_edge() {
        let mut rect = Annot::new(
            AnnotKind::Rect,
            vec![Pos2::new(10.0, 20.0), Pos2::new(110.0, 70.0)],
            style(8.0),
        );
        rect.resize(Handle::W, Pos2::new(0.0, 999.0), false);
        let b = rect.bounds();
        approx(b.left(), 0.0);
        approx(b.right(), 110.0);
        approx(b.top(), 20.0);
        approx(b.bottom(), 70.0);
    }

    #[test]
    fn resize_keep_ratio_corner_follows_larger_change() {
        let mut rect = Annot::new(
            AnnotKind::Rect,
            vec![Pos2::new(0.0, 0.0), Pos2::new(100.0, 50.0)],
            style(8.0),
        );
        // pointer at (200,100): relative change 2.0 both ways -> 200x100
        rect.resize(Handle::Se, Pos2::new(200.0, 100.0), true);
        let b = rect.bounds();
        approx(b.width(), 200.0);
        approx(b.height(), 100.0);
        approx(b.left(), 0.0);
        approx(b.top(), 0.0);
    }

    #[test]
    fn resize_keep_ratio_edge_derives_the_other_axis() {
        let mut rect = Annot::new(
            AnnotKind::Rect,
            vec![Pos2::new(0.0, 0.0), Pos2::new(100.0, 50.0)],
            style(8.0),
        );
        rect.resize(Handle::E, Pos2::new(200.0, 999.0), true);
        let b = rect.bounds();
        approx(b.width(), 200.0);
        approx(b.height(), 100.0);
    }

    #[test]
    fn brush_resize_scales_points_and_widths() {
        let mut brush = Annot::new(
            AnnotKind::Brush,
            vec![Pos2::new(0.0, 0.0), Pos2::new(10.0, 10.0)],
            style(8.0),
        );
        let b0 = brush.bounds();
        // Double the bounds on both axes via the SE corner.
        let target = Pos2::new(b0.right() + b0.width(), b0.bottom() + b0.height());
        let w0 = brush.point_width(0);
        brush.resize(Handle::Se, target, false);
        let b1 = brush.bounds();
        assert!((b1.width() / b0.width() - 2.0).abs() < 0.05);
        assert!((b1.height() / b0.height() - 2.0).abs() < 0.05);
        assert!(brush.point_width(0) > w0);
    }

    #[test]
    fn text_resize_scales_font_size() {
        let mut text =
            Annot::text(Pos2::new(10.0, 10.0), "hello", Style { font_size: 36.0, ..Style::default() });
        let h0 = text.bounds().height();
        text.resize(Handle::Se, Pos2::new(500.0, 10.0 + h0 * 2.0), false);
        approx(text.style.font_size, 72.0);
        approx(text.points[0].x, 10.0);
        approx(text.points[0].y, 10.0);
    }

    #[test]
    fn arrow_resize_moves_a_single_endpoint() {
        let mut arrow = Annot::new(
            AnnotKind::Arrow,
            vec![Pos2::new(0.0, 0.0), Pos2::new(100.0, 0.0)],
            style(8.0),
        );
        arrow.resize(Handle::End, Pos2::new(100.0, 80.0), false);
        assert_eq!(arrow.points[0], Pos2::new(0.0, 0.0));
        assert_eq!(arrow.points[1], Pos2::new(100.0, 80.0));
        // too close to the fixed end: clamped to the minimum length
        arrow.resize(Handle::End, Pos2::new(0.1, 0.0), false);
        approx((arrow.points[1] - arrow.points[0]).length(), MIN_ARROW_LEN);
    }
}

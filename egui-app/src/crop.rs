// Crop rectangle model, ratio presets and overlay geometry.
// Image-space coordinates. Owner: crop/export teammate.
//
// Parity source: src/js/ui/panel-crop.js (centred 80% default, ratio presets,
// "keep the crop centre" rebuild, apply-time clamping) and the crop gestures in
// src/js/core/interaction.js (eight handles, drag-to-create, move, resize with
// a locked ratio, outside dimming + rule-of-thirds overlay).

use egui::{Color32, Painter, Pos2, Rect, Vec2};

use crate::theme;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CropRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub ratio: Option<f32>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CropHandle {
    Nw,
    N,
    Ne,
    E,
    Se,
    S,
    Sw,
    W,
}

/// Ratio chips shown in the crop panel, in web order (自由/原始/1:1/4:3/3:2/16:9).
pub const RATIO_LABELS: [&str; 6] = ["自由", "原始", "1:1", "4:3", "3:2", "16:9"];

/// Minimum crop size in image px (the web build rejects drags smaller than
/// 8 screen px; this is the lower bound of an accepted rectangle).
pub const MIN_SIZE: f32 = 2.0;

/// Matches panel-crop.js: |a - b| < 0.012 counts as the same ratio.
pub const RATIO_EPS: f32 = 0.012;

fn clamp(v: f32, lo: f32, hi: f32) -> f32 {
    // NB: not f32::clamp — it panics when lo > hi (possible for 1px images).
    lo.max(hi.min(v))
}

fn round(v: f32) -> f32 {
    // JS Math.round: halves go up (0.5 -> 1), unlike Rust's round-away-from-zero
    // for negatives which we never hit here because rectangles are >= 0.
    (v + 0.5).floor()
}

/// Resolve a ratio chip label to a concrete ratio (None = 自由).
pub fn ratio_for(label: &str, doc_w: u32, doc_h: u32) -> Option<f32> {
    match label {
        "自由" | "free" => None,
        "原始" | "orig" | "original" => {
            if doc_w > 0 && doc_h > 0 {
                Some(doc_w as f32 / doc_h as f32)
            } else {
                None
            }
        }
        "1:1" => Some(1.0),
        "4:3" => Some(4.0 / 3.0),
        "3:2" => Some(3.0 / 2.0),
        "16:9" => Some(16.0 / 9.0),
        other => {
            // "a:b" fallback for any future chip.
            let mut parts = other.split(':');
            let a: Option<f32> = parts.next().and_then(|s| s.trim().parse().ok());
            let b: Option<f32> = parts.next().and_then(|s| s.trim().parse().ok());
            match (a, b) {
                (Some(a), Some(b)) if a > 0.0 && b > 0.0 => Some(a / b),
                _ => None,
            }
        }
    }
}

/// Which chip is active for a given crop ratio (panel-crop.js `render`).
pub fn ratio_label(ratio: Option<f32>, doc_w: u32, doc_h: u32) -> &'static str {
    let Some(value) = ratio else { return RATIO_LABELS[0] };
    if !value.is_finite() {
        return RATIO_LABELS[0];
    }
    for label in RATIO_LABELS.iter().skip(1) {
        if let Some(target) = ratio_for(label, doc_w, doc_h) {
            if (value - target).abs() < RATIO_EPS {
                return label;
            }
        }
    }
    RATIO_LABELS[0]
}

impl Default for CropRect {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0, w: 0.0, h: 0.0, ratio: None }
    }
}

impl CropRect {
    pub fn new(x: f32, y: f32, w: f32, h: f32, ratio: Option<f32>) -> Self {
        Self { x, y, w, h, ratio }
    }

    pub fn to_rect(self) -> Rect {
        Rect::from_min_size(Pos2::new(self.x, self.y), Vec2::new(self.w, self.h))
    }

    pub fn is_valid(self) -> bool {
        self.w >= 1.0 && self.h >= 1.0
    }

    pub fn center(self) -> Pos2 {
        Pos2::new(self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    pub fn contains(self, pt: Pos2) -> bool {
        self.is_valid()
            && pt.x >= self.x
            && pt.x <= self.x + self.w
            && pt.y >= self.y
            && pt.y <= self.y + self.h
    }

    /// 80% centred default (panel-crop.js `centeredRect`): free ratio keeps
    /// 10% margins, a locked ratio shrinks to fit 80% of the doc on both axes.
    pub fn centered(doc_w: u32, doc_h: u32, ratio: Option<f32>) -> Self {
        let (dw, dh) = (doc_w.max(1) as f32, doc_h.max(1) as f32);
        let out = if let Some(r) = ratio.filter(|r| r.is_finite() && *r > 0.0) {
            let mut w = dw * 0.8;
            let mut h = w / r;
            if h > dh * 0.8 {
                h = dh * 0.8;
                w = h * r;
            }
            Self {
                x: round((dw - w) / 2.0),
                y: round((dh - h) / 2.0),
                w: round(w),
                h: round(h),
                ratio: Some(r),
            }
        } else {
            Self {
                x: round(dw * 0.1),
                y: round(dh * 0.1),
                w: round(dw * 0.8),
                h: round(dh * 0.8),
                ratio: None,
            }
        };
        out.clamp_inside(doc_w, doc_h)
    }

    /// Rebuild around the current centre while matching a new ratio
    /// (panel-crop.js chip handler; 自由 only drops the lock and keeps w/h).
    pub fn with_ratio(self, doc_w: u32, doc_h: u32, ratio: Option<f32>) -> Self {
        let (dw, dh) = (doc_w.max(1) as f32, doc_h.max(1) as f32);
        let Some(r) = ratio.filter(|r| r.is_finite() && *r > 0.0) else {
            return Self { ratio: None, ..self };
        };
        let c = self.center();
        let mut w = dw.min(self.w.max(16.0));
        let mut h = w / r;
        if h > dh {
            h = dh;
            w = h * r;
        }
        let x = round(clamp(c.x - w / 2.0, 0.0, (dw - w).max(0.0)));
        let y = round(clamp(c.y - h / 2.0, 0.0, (dh - h).max(0.0)));
        Self { x, y, w: round(w), h: round(h), ratio: Some(r) }.clamp_inside(doc_w, doc_h)
    }

    /// Keep the rectangle inside the image and at least 1 px in both axes.
    pub fn clamp_inside(self, doc_w: u32, doc_h: u32) -> Self {
        let (dw, dh) = (doc_w.max(1) as f32, doc_h.max(1) as f32);
        let w = self.w.clamp(1.0, dw);
        let h = self.h.clamp(1.0, dh);
        let x = clamp(self.x, 0.0, (dw - w).max(0.0));
        let y = clamp(self.y, 0.0, (dh - h).max(0.0));
        Self { x, y, w, h, ratio: self.ratio }
    }

    /// 套用裁切: round + clamp exactly like panel-crop.js before the crop is
    /// committed and handed to the export pipeline.
    pub fn normalized(self, doc_w: u32, doc_h: u32) -> Self {
        let (dw, dh) = (doc_w.max(1) as f32, doc_h.max(1) as f32);
        let x = round(clamp(self.x, 0.0, dw - 1.0));
        let y = round(clamp(self.y, 0.0, dh - 1.0));
        let w = round(clamp(self.w, 1.0, dw - x).max(1.0));
        let h = round(clamp(self.h, 1.0, dh - y).max(1.0));
        Self { x, y, w, h, ratio: self.ratio }
    }

    pub fn handles(self) -> [(CropHandle, Pos2); 8] {
        let r = self.to_rect();
        [
            (CropHandle::Nw, r.left_top()),
            (CropHandle::N, r.center_top()),
            (CropHandle::Ne, r.right_top()),
            (CropHandle::E, r.right_center()),
            (CropHandle::Se, r.right_bottom()),
            (CropHandle::S, r.center_bottom()),
            (CropHandle::Sw, r.left_bottom()),
            (CropHandle::W, r.left_center()),
        ]
    }

    pub fn handle_name(handle: CropHandle) -> &'static str {
        match handle {
            CropHandle::Nw => "nw",
            CropHandle::N => "n",
            CropHandle::Ne => "ne",
            CropHandle::E => "e",
            CropHandle::Se => "se",
            CropHandle::S => "s",
            CropHandle::Sw => "sw",
            CropHandle::W => "w",
        }
    }

    /// Closest handle within `tol` image px (interaction.js `cropHandleAt`).
    pub fn hit_handle(self, pt: Pos2, tol: f32) -> Option<CropHandle> {
        if !self.is_valid() {
            return None;
        }
        let mut best: Option<(CropHandle, f32)> = None;
        for (handle, p) in self.handles() {
            let d = (pt - p).length();
            if d <= tol && best.map_or(true, |(_, bd)| d < bd) {
                best = Some((handle, d));
            }
        }
        best.map(|(h, _)| h)
    }

    /// Resize one handle with the rectangle's own locked ratio (if any).
    pub fn resize(self, handle: CropHandle, pt: Pos2, doc_w: u32, doc_h: u32) -> Self {
        self.resize_with_ratio(handle, pt, doc_w, doc_h, self.ratio)
    }

    /// `resizeCropRect` from interaction.js. `ratio` is passed explicitly so the
    /// gesture can honour a transient Shift lock without mutating the model.
    pub fn resize_with_ratio(
        self,
        handle: CropHandle,
        pt: Pos2,
        doc_w: u32,
        doc_h: u32,
        ratio: Option<f32>,
    ) -> Self {
        let (dw, dh) = (doc_w.max(1) as f32, doc_h.max(1) as f32);
        let base = self;
        let (dw_side, de_side, dn_side, ds_side) = match handle {
            CropHandle::Nw => (true, false, true, false),
            CropHandle::N => (false, false, true, false),
            CropHandle::Ne => (false, true, true, false),
            CropHandle::E => (false, true, false, false),
            CropHandle::Se => (false, true, false, true),
            CropHandle::S => (false, false, false, true),
            CropHandle::Sw => (true, false, false, true),
            CropHandle::W => (true, false, false, false),
        };
        let mut left = base.x;
        let mut top = base.y;
        let mut width = base.w;
        let mut height = base.h;

        if dw_side {
            let right = base.x + base.w;
            left = clamp(pt.x, 0.0, right - 1.0);
            width = right - left;
        } else if de_side {
            width = (clamp(pt.x, 0.0, dw) - base.x).max(1.0);
        }
        if dn_side {
            let bottom = base.y + base.h;
            top = clamp(pt.y, 0.0, bottom - 1.0);
            height = bottom - top;
        } else if ds_side {
            height = (clamp(pt.y, 0.0, dh) - base.y).max(1.0);
        }

        if let Some(r) = ratio.filter(|r| r.is_finite() && *r > 0.0) {
            if dw_side || de_side {
                height = width / r;
                if dn_side {
                    top = base.y + base.h - height;
                } else if !ds_side {
                    top = base.y;
                }
            } else if dn_side || ds_side {
                width = height * r;
                if dw_side {
                    left = base.x + base.w - width;
                } else if !de_side {
                    left = base.x;
                }
            }
        }

        width = width.min(dw).max(1.0);
        height = height.min(dh).max(1.0);
        left = clamp(left, 0.0, (dw - width).max(0.0));
        top = clamp(top, 0.0, (dh - height).max(0.0));
        Self { x: left, y: top, w: width, h: height, ratio }
    }

    /// Drag-to-create (interaction.js `crop-new`): `start` is the anchor, `end`
    /// the current pointer, both clamped to the image; an optional ratio keeps
    /// the box proportional (Shift passes 1.0).
    pub fn from_drag(
        start: Pos2,
        end: Pos2,
        ratio: Option<f32>,
        doc_w: u32,
        doc_h: u32,
    ) -> Self {
        let (dw, dh) = (doc_w.max(1) as f32, doc_h.max(1) as f32);
        let x0 = clamp(start.x, 0.0, dw);
        let y0 = clamp(start.y, 0.0, dh);
        let mut x1 = clamp(end.x, 0.0, dw);
        let mut y1 = clamp(end.y, 0.0, dh);
        if let Some(r) = ratio.filter(|r| r.is_finite() && *r > 0.0) {
            let dx = x1 - x0;
            let dy = y1 - y0;
            if dx.abs() >= dy.abs() * r {
                y1 = clamp(y0 + if dy < 0.0 { -1.0 } else { 1.0 } * (dx.abs() / r), 0.0, dh);
            } else {
                x1 = clamp(x0 + if dx < 0.0 { -1.0 } else { 1.0 } * (dy.abs() * r), 0.0, dw);
            }
        }
        Self {
            x: x0.min(x1),
            y: y0.min(y1),
            w: (x1 - x0).abs().max(1.0),
            h: (y1 - y0).abs().max(1.0),
            ratio,
        }
        .clamp_inside(doc_w, doc_h)
    }

    /// Drag the whole rectangle (interaction.js `crop-move`), clamped inside.
    pub fn moved(self, delta: Vec2, doc_w: u32, doc_h: u32) -> Self {
        let (dw, dh) = (doc_w.max(1) as f32, doc_h.max(1) as f32);
        Self {
            x: clamp(self.x + delta.x, 0.0, (dw - self.w).max(0.0)),
            y: clamp(self.y + delta.y, 0.0, (dh - self.h).max(0.0)),
            ..self
        }
    }

    /// Crop overlay: dim everything outside, rule-of-thirds guides, white frame
    /// and the eight handle squares (interaction.js `paintCrop`).
    /// `canvas` is the screen-space region of the image, used for the dim mask.
    pub fn draw_in(
        &self,
        painter: &Painter,
        to_screen: &dyn Fn(Pos2) -> Pos2,
        zoom: f32,
        canvas: Rect,
    ) {
        let a = to_screen(Pos2::new(self.x, self.y));
        let b = to_screen(Pos2::new(self.x + self.w, self.y + self.h));
        let frame = Rect::from_two_pos(a, b);

        // 框外壓暗 (rgba(0,0,0,0.55))
        let dim = Color32::from_rgba_premultiplied(0, 0, 0, 140);
        if frame.top() > canvas.top() {
            painter.rect_filled(
                Rect::from_min_max(canvas.min, Pos2::new(canvas.right(), frame.top())),
                0.0,
                dim,
            );
        }
        if frame.bottom() < canvas.bottom() {
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(canvas.left(), frame.bottom()), canvas.max),
                0.0,
                dim,
            );
        }
        let band_top = frame.top().max(canvas.top());
        let band_bottom = frame.bottom().min(canvas.bottom());
        if frame.left() > canvas.left() {
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(canvas.left(), band_top),
                    Pos2::new(frame.left(), band_bottom),
                ),
                0.0,
                dim,
            );
        }
        if frame.right() < canvas.right() {
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(frame.right(), band_top),
                    Pos2::new(canvas.right(), band_bottom),
                ),
                0.0,
                dim,
            );
        }

        // 三分線 (rgba(255,255,255,0.34) premultiplied)
        let guide = egui::Stroke::new(1.0, Color32::from_rgba_premultiplied(87, 87, 87, 87));
        for i in 1..3 {
            let t = i as f32 / 3.0;
            let x = frame.left() + frame.width() * t;
            let y = frame.top() + frame.height() * t;
            painter.line_segment(
                [Pos2::new(x, frame.top()), Pos2::new(x, frame.bottom())],
                guide,
            );
            painter.line_segment(
                [Pos2::new(frame.left(), y), Pos2::new(frame.right(), y)],
                guide,
            );
        }

        // 邊框 (rgba(255,255,255,0.92)) + 把手
        let border = egui::Stroke::new(
            1.0,
            Color32::from_rgba_premultiplied(235, 235, 235, 235),
        );
        painter.rect_stroke(frame, 0.0, border, egui::StrokeKind::Middle);

        // Handles keep a constant screen size (web: HANDLE_SIZE + 2 px), with a
        // slight zoom response so they stay tappable on small images.
        let hw = (10.0 * zoom.clamp(0.6, 1.6)).clamp(7.0, 16.0);
        let accent = theme::ACCENT;
        for (_, p) in self.handles() {
            let sp = to_screen(p);
            let r = Rect::from_center_size(sp, Vec2::splat(hw));
            painter.rect_filled(r, 0.0, Color32::WHITE);
            painter.rect_stroke(
                r,
                0.0,
                egui::Stroke::new(1.0, accent),
                egui::StrokeKind::Middle,
            );
        }
    }

    /// Convenience for callers that only have the painter's clip rect.
    pub fn draw(&self, painter: &Painter, to_screen: &dyn Fn(Pos2) -> Pos2, zoom: f32) {
        self.draw_in(painter, to_screen, zoom, painter.clip_rect());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inside(c: CropRect, w: u32, h: u32) -> bool {
        c.x >= -0.001
            && c.y >= -0.001
            && c.w >= 1.0
            && c.h >= 1.0
            && c.x + c.w <= w as f32 + 0.001
            && c.y + c.h <= h as f32 + 0.001
    }

    #[test]
    fn centered_free_is_eighty_percent_and_inside() {
        let c = CropRect::centered(1000, 600, None);
        assert_eq!((c.x, c.y), (100.0, 60.0));
        assert_eq!((c.w, c.h), (800.0, 480.0));
        assert!(inside(c, 1000, 600));
        // tiny docs must still yield a valid 1px+ rect inside the image
        for (w, h) in [(1u32, 1u32), (2, 3), (7, 5), (17, 13)] {
            let c = CropRect::centered(w, h, None);
            assert!(inside(c, w, h), "centered({w},{h}) = {c:?}");
        }
    }

    #[test]
    fn centered_ratio_is_inside_and_proportional() {
        for doc in [(1000u32, 600u32), (600, 1000), (4032, 3024), (37, 91)] {
            for label in RATIO_LABELS.iter().skip(1) {
                let r = ratio_for(label, doc.0, doc.1).unwrap();
                let c = CropRect::centered(doc.0, doc.1, Some(r));
                assert!(inside(c, doc.0, doc.1), "{label} on {:?} => {c:?}", doc);
                // rounding may move the ratio by a pixel; keep it within ~3%
                let got = c.w / c.h;
                assert!(
                    (got - r).abs() / r < 0.03,
                    "{label} ratio drifted: {got} vs {r} ({c:?})"
                );
                assert_eq!(c.ratio, Some(r));
            }
        }
    }

    #[test]
    fn ratio_rebuild_keeps_centre_and_stays_inside() {
        let doc = (1600u32, 900u32);
        let c0 = CropRect::new(300.0, 150.0, 640.0, 480.0, None);
        let centre = c0.center();
        for label in ["1:1", "4:3", "3:2", "16:9", "原始"] {
            let r = ratio_for(label, doc.0, doc.1).unwrap();
            let c = c0.with_ratio(doc.0, doc.1, Some(r));
            assert!(inside(c, doc.0, doc.1), "{label}: {c:?}");
            assert_eq!(c.ratio, Some(r));
            // centre is preserved unless the rect had to be clamped at an edge
            assert!(
                (c.center().x - centre.x).abs() <= 1.0
                    && (c.center().y - centre.y).abs() <= 1.0,
                "{label}: centre moved {:?} -> {:?}",
                centre,
                c.center()
            );
            let got = c.w / c.h;
            assert!((got - r).abs() / r < 0.02, "{label}: ratio {got} vs {r}");
        }
        // free ratio keeps w/h, only drops the lock
        let c = c0.with_ratio(doc.0, doc.1, None);
        assert_eq!((c.w, c.h), (c0.w, c0.h));
        assert_eq!(c.ratio, None);
    }

    /// Regression for the 1400x934 parity capture: the web rebuild ends at
    /// exactly 934x934 for a 1:1 lock (not 937), 1120x630 for 16:9 and
    /// 1120x840 for 4:3, starting from the 80% default crop (1120x747).
    #[test]
    fn ratio_rebuild_matches_the_web_capture_numbers() {
        let (dw, dh) = (1400u32, 934u32);
        let base = CropRect::centered(dw, dh, None);
        assert_eq!((base.x, base.y), (140.0, 93.0));
        assert_eq!((base.w, base.h), (1120.0, 747.0));
        assert_eq!(base.normalized(dw, dh), base);

        let square = base.with_ratio(dw, dh, Some(1.0));
        assert_eq!((square.w, square.h), (934.0, 934.0));
        assert_eq!((square.x, square.y), (233.0, 0.0));
        assert!(square.to_rect().max.x <= dw as f32);
        assert!(square.to_rect().max.y <= dh as f32);

        let wide = base.with_ratio(dw, dh, Some(16.0 / 9.0));
        assert_eq!((wide.w, wide.h), (1120.0, 630.0));
        assert_eq!((wide.x, wide.y), (140.0, 152.0));

        let four3 = base.with_ratio(dw, dh, Some(4.0 / 3.0));
        assert_eq!((four3.w, four3.h), (1120.0, 840.0));
        assert_eq!((four3.x, four3.y), (140.0, 47.0));

        // The size shown in the inspector is these rounded values, and the same
        // numbers must survive the 套用裁切 normalise step used for export.
        for c in [square, wide, four3] {
            let applied = c.normalized(dw, dh);
            assert_eq!((applied.w, applied.h), (c.w, c.h));
            assert_eq!((applied.x, applied.y), (c.x, c.y));
        }
    }

    #[test]
    fn ratio_rebuild_from_edge_stays_inside() {
        let doc = (800u32, 600u32);
        // a crop hugging the top-left corner may not keep the centre exactly,
        // but must never leave the image
        let c = CropRect::new(0.0, 0.0, 700.0, 500.0, None);
        let out = c.with_ratio(doc.0, doc.1, Some(16.0 / 9.0));
        assert!(inside(out, doc.0, doc.1), "{out:?}");
        let c = CropRect::new(700.0, 500.0, 100.0, 100.0, None);
        let out = c.with_ratio(doc.0, doc.1, Some(1.0));
        assert!(inside(out, doc.0, doc.1), "{out:?}");
    }

    #[test]
    fn resize_keeps_locked_ratio_and_bounds() {
        let doc = (1200u32, 800u32);
        let base = CropRect::new(200.0, 100.0, 400.0, 300.0, Some(4.0 / 3.0));
        // Drags that stay inside the image must keep the locked ratio exactly.
        for (handle, pt) in [
            (CropHandle::Se, Pos2::new(700.0, 400.0)),
            (CropHandle::Nw, Pos2::new(0.0, 0.0)),
            (CropHandle::Ne, Pos2::new(700.0, 0.0)),
            (CropHandle::Sw, Pos2::new(0.0, 799.0)),
            (CropHandle::N, Pos2::new(300.0, 0.0)),
            (CropHandle::E, Pos2::new(500.0, 400.0)),
            (CropHandle::S, Pos2::new(400.0, 799.0)),
            (CropHandle::W, Pos2::new(0.0, 400.0)),
        ] {
            let out = base.resize(handle, pt, doc.0, doc.1);
            assert!(inside(out, doc.0, doc.1), "{handle:?} {pt:?} => {out:?}");
            let got = out.w / out.h;
            let want = 4.0 / 3.0;
            assert!(
                (got - want).abs() / want < 0.02,
                "{handle:?} ratio drifted: {got} vs {want} ({out:?})"
            );
            assert_eq!(out.ratio, Some(want));
        }
    }

    #[test]
    fn resize_stays_inside_even_when_ratio_must_yield() {
        // Dragging a corner far outside the image: the web build clamps w/h to
        // the doc, which breaks the locked ratio — bounds win, never overflow.
        let doc = (1200u32, 800u32);
        let base = CropRect::new(200.0, 100.0, 400.0, 300.0, Some(4.0 / 3.0));
        for (handle, pt) in [
            (CropHandle::Se, Pos2::new(9000.0, 9000.0)),
            (CropHandle::Ne, Pos2::new(9000.0, -9000.0)),
            (CropHandle::Sw, Pos2::new(-9000.0, 9000.0)),
            (CropHandle::E, Pos2::new(1199.0, 400.0)),
            (CropHandle::S, Pos2::new(400.0, 799.0)),
            (CropHandle::Se, Pos2::new(100.0, 50.0)), // inverted drag
        ] {
            let out = base.resize(handle, pt, doc.0, doc.1);
            assert!(inside(out, doc.0, doc.1), "{handle:?} {pt:?} => {out:?}");
        }
    }

    #[test]
    fn resize_without_ratio_uses_free_edges() {
        let base = CropRect::new(200.0, 100.0, 400.0, 300.0, None);
        let out = base.resize(CropHandle::Se, Pos2::new(700.0, 500.0), 1200, 800);
        assert_eq!((out.x, out.y, out.w, out.h), (200.0, 100.0, 500.0, 400.0));
        let out = base.resize(CropHandle::Nw, Pos2::new(100.0, 50.0), 1200, 800);
        assert_eq!((out.x, out.y), (100.0, 50.0));
        assert_eq!((out.w, out.h), (500.0, 350.0));
    }

    #[test]
    fn resize_shift_ratio_override() {
        let base = CropRect::new(100.0, 100.0, 400.0, 100.0, None);
        // Shift while dragging E: width changes, height follows to stay square
        let out = base.resize_with_ratio(CropHandle::E, Pos2::new(500.0, 0.0), 1000, 1000, Some(1.0));
        assert!(inside(out, 1000, 1000));
        assert!((out.w - out.h).abs() < 0.01, "{out:?}");
        // the model's own lock is not modified by the transient override
        assert_eq!(out.ratio, Some(1.0));
    }

    #[test]
    fn drag_create_normalises_and_clamps() {
        let c = CropRect::from_drag(Pos2::new(500.0, 400.0), Pos2::new(100.0, 50.0), None, 800, 600);
        assert_eq!((c.x, c.y, c.w, c.h), (100.0, 50.0, 400.0, 350.0));
        // ratio drags stay proportional and inside
        let c = CropRect::from_drag(Pos2::new(10.0, 10.0), Pos2::new(300.0, 50.0), Some(1.0), 800, 600);
        assert!(inside(c, 800, 600), "{c:?}");
        assert!((c.w - c.h).abs() < 0.01, "{c:?}");
        // outside the image: clamped
        let c = CropRect::from_drag(Pos2::new(-50.0, -50.0), Pos2::new(5000.0, 5000.0), None, 800, 600);
        assert!(inside(c, 800, 600), "{c:?}");
    }

    #[test]
    fn move_clamps_inside_image() {
        let c = CropRect::new(10.0, 10.0, 100.0, 100.0, None);
        let out = c.moved(Vec2::new(-500.0, -500.0), 800, 600);
        assert_eq!((out.x, out.y), (0.0, 0.0));
        let out = c.moved(Vec2::new(5000.0, 5000.0), 800, 600);
        assert_eq!((out.x, out.y), (700.0, 500.0));
    }

    #[test]
    fn handles_and_hit_testing() {
        let c = CropRect::new(100.0, 100.0, 400.0, 200.0, None);
        let hs = c.handles();
        assert_eq!(hs.len(), 8);
        assert_eq!(hs[0], (CropHandle::Nw, Pos2::new(100.0, 100.0)));
        assert_eq!(hs[4], (CropHandle::Se, Pos2::new(500.0, 300.0)));
        assert_eq!(hs[3], (CropHandle::E, Pos2::new(500.0, 200.0)));
        assert_eq!(c.hit_handle(Pos2::new(501.0, 201.0), 4.0), Some(CropHandle::E));
        assert_eq!(c.hit_handle(Pos2::new(300.0, 200.0), 4.0), None);
        assert!(c.contains(Pos2::new(300.0, 200.0)));
        assert!(!c.contains(Pos2::new(50.0, 200.0)));
    }

    #[test]
    fn ratio_labels_round_trip() {
        // 900x700 keeps 原始 distinct from every fixed preset.
        let doc = (900u32, 700u32);
        for label in RATIO_LABELS.iter().skip(1) {
            let r = ratio_for(label, doc.0, doc.1).unwrap();
            assert_eq!(ratio_label(Some(r), doc.0, doc.1), *label);
        }
        // when 原始 equals a preset, the first matching chip wins (web order)
        assert_eq!(ratio_label(Some(16.0 / 9.0), 1600, 900), "原始");
        assert_eq!(ratio_label(None, doc.0, doc.1), "自由");
        assert_eq!(ratio_label(Some(2.17), doc.0, doc.1), "自由");
        assert_eq!(ratio_for("自由", doc.0, doc.1), None);
        let orig = ratio_for("原始", 1600, 900).unwrap();
        assert!((orig - 16.0 / 9.0).abs() < 1e-6);
    }

    #[test]
    fn normalized_rounds_and_clamps() {
        let c = CropRect::new(-3.4, 5.5, 2000.0, 4.0, None);
        let n = c.normalized(800, 600);
        assert_eq!((n.x, n.y), (0.0, 6.0));
        assert_eq!(n.w, 800.0);
        assert_eq!(n.h, 4.0);
        assert!(inside(n, 800, 600));
    }
}

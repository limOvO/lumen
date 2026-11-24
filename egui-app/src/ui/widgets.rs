// Shared painting toolkit for the chrome: buttons, chips, swatches, sliders,
// numeric fields and the line-icon set used across the top bar / rail / panels.
// Everything here is pure painting on top of egui primitives so the result can
// match `src/css/app.css` closely (30px hit targets, 8px radii, near-black
// surfaces, near-white primary buttons).

use egui::{
    Align, Align2, Color32, CornerRadius, CursorIcon, FontId, Id, Painter, Pos2, Rect, Response,
    Sense, Shape, Stroke, StrokeKind, Ui, Vec2,
};

use crate::theme;

use super::motion;

pub const RADIUS: u8 = 8;
pub const BTN_H: f32 = 30.0;

// ---------------------------------------------------------------------------
// Palette (derived from tokens.css where theme.rs has no direct constant).
// ---------------------------------------------------------------------------

/// `--surface` = 6% white over the panel colour.
pub const SURFACE: Color32 = Color32::from_rgb(48, 48, 49);
/// `--surface-2` = 10% white over the panel colour.
pub const SURFACE_2: Color32 = Color32::from_rgb(57, 57, 58);
/// `--rail` / `--status` (#1c1c1e).
pub const RAIL: Color32 = Color32::from_rgb(28, 28, 30);
/// Floating pill background: rgba(21, 21, 23, 0.86) over the canvas.
pub const CHIP_BG: Color32 = Color32::from_rgb(21, 21, 23);
/// Tool semantics red (--tool).
pub const TOOL: Color32 = Color32::from_rgb(255, 92, 92);

pub fn brand_soft() -> Color32 {
    Color32::from_rgba_unmultiplied(249, 250, 251, 26) // web --brand-soft (10%)
}

pub fn white(a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(255, 255, 255, a)
}

/// Dim text faded for disabled controls (~28% of --text-dim).
pub fn disabled(color: Color32) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 92)
}

pub fn is_enabled(ui: &Ui, enabled: bool) -> bool {
    enabled && ui.is_enabled()
}

/// Disabled controls must not be clickable at all (egui custom widgets keep
/// their sense otherwise, so a "disabled" 儲存 button would still emit clicks).
fn sense(enabled: bool, desired: Sense) -> Sense {
    if enabled {
        desired
    } else {
        Sense::hover()
    }
}

// ---------------------------------------------------------------------------
// Icons: 24×24 stroke drawings matched to the SVGs in src/index.html.
// ---------------------------------------------------------------------------

#[allow(dead_code)] // Shared icon vocabulary; some glyphs are only used by other chrome.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Brand,
    Move,
    Brush,
    Rect,
    Ellipse,
    Arrow,
    Line,
    Text,
    Crop,
    Eye,
    Pencil,
    Info,
    Plus,
    Minus,
    Fit,
    Folder,
    Save,
    SaveAs,
    Undo,
    Redo,
    Panel,
    Help,
    Close,
    Lock,
    Prev,
    Next,
    Dashed,
    Copy,
}

fn svg_name(icon: Icon) -> Option<&'static str> {
    Some(match icon {
        Icon::Brand => "tb-brand",
        Icon::Move => "move",
        Icon::Brush => "brush",
        Icon::Rect => "rect",
        Icon::Ellipse => "ellipse",
        Icon::Arrow => "arrow",
        Icon::Line => "line",
        Icon::Text => "text",
        Icon::Crop => "crop",
        Icon::Eye => "btn-preview",
        Icon::Pencil => "preview-edit",
        Icon::Info => "preview-info",
        Icon::Plus => "zoom-in",
        Icon::Minus => "zoom-out",
        Icon::Fit => "zoom-fit",
        Icon::Folder => "btn-open",
        Icon::Save => "btn-save",
        Icon::SaveAs => "btn-saveas",
        Icon::Undo => "btn-undo",
        Icon::Redo => "btn-redo",
        Icon::Panel => "btn-insp-collapse",
        Icon::Help => "btn-shortcuts",
        Icon::Close => "browser-title",
        Icon::Lock => "adj-lock",
        Icon::Prev => "preview-nav-prev",
        Icon::Next => "preview-nav-next",
        Icon::Dashed => "style-dashed",
        Icon::Copy => "info-copy-path",
    })
}

/// Draw one of the web build's inline SVGs (24x24, 1.8 stroke, round caps).
fn paint_svg_icon(painter: &Painter, rect: Rect, icon: &super::svg_icons::ToolIcon, color: Color32) {
    let d = rect.width().min(rect.height()).max(1.0);
    let scale = d / 24.0;
    let stroke = Stroke::new((1.8 * scale).max(0.8), color);
    let origin = rect.center() - Vec2::splat(d / 2.0);
    for path in icon.paths {
        if path.len() < 2 {
            continue;
        }
        let pts: Vec<Pos2> = path
            .iter()
            .map(|(x, y)| origin + Vec2::new(x * scale, y * scale))
            .collect();
        painter.add(Shape::line(pts.clone(), stroke));
        for p in [pts[0], *pts.last().unwrap()] {
            painter.circle_filled(p, stroke.width * 0.5, color);
        }
    }
}

pub fn paint_icon(painter: &Painter, rect: Rect, icon: Icon, color: Color32, px: f32) {
    if let Some(name) = svg_name(icon) {
        if let Some(g) = super::svg_icons::ui(name).or_else(|| super::svg_icons::rail(name)) {
            paint_svg_icon(painter, rect, g, color, );
            return;
        }
    }
    let d = rect.width().min(rect.height()).max(1.0);
    let s = d / 24.0;
    let origin = rect.center() - Vec2::splat(d / 2.0);
    let p = |x: f32, y: f32| origin + Vec2::new(x * s, y * s);
    let stroke = Stroke::new((px * s / 0.75).clamp(0.9, 2.2), color);

    let poly = |pts: Vec<Pos2>| {
        if pts.len() > 1 {
            painter.add(Shape::line(pts, stroke));
        }
    };
    let seg = |a: (f32, f32), b: (f32, f32)| {
        painter.line_segment([p(a.0, a.1), p(b.0, b.1)], stroke);
        ()
    };
    let circle = |c: (f32, f32), r: f32| {
        painter.circle_stroke(p(c.0, c.1), r * s, stroke);
        ()
    };
    let dot = |c: (f32, f32), r: f32| {
        painter.circle_filled(p(c.0, c.1), (r * s).max(0.9), color);
        ()
    };
    let rounded = |min: (f32, f32), max: (f32, f32), r: f32| {
        painter.rect_stroke(
            Rect::from_min_max(p(min.0, min.1), p(max.0, max.1)),
            CornerRadius::same((r * s).round() as u8),
            stroke,
            StrokeKind::Middle,
        );
        ()
    };
    let arc = |cx: f32, cy: f32, r: f32, a0: f32, a1: f32| -> Vec<Pos2> {
        let n = 18;
        (0..=n)
            .map(|i| {
                let t = a0 + (a1 - a0) * (i as f32 / n as f32);
                p(cx + r * t.cos(), cy + r * t.sin())
            })
            .collect()
    };

    match icon {
        Icon::Brand => {
            rounded((3.5, 4.5), (20.5, 19.5), 3.0);
            circle((9.0, 10.0), 1.7);
            poly(vec![p(4.5, 17.5), p(9.5, 13.0), p(12.9, 16.1), p(15.9, 13.7), p(19.5, 17.0)]);
        }
        Icon::Move => {
            seg((12.0, 3.5), (12.0, 20.5));
            seg((3.5, 12.0), (20.5, 12.0));
            poly(vec![p(9.0, 6.0), p(12.0, 3.5), p(15.0, 6.0)]);
            poly(vec![p(9.0, 18.0), p(12.0, 20.5), p(15.0, 18.0)]);
            poly(vec![p(6.0, 9.0), p(3.5, 12.0), p(6.0, 15.0)]);
            poly(vec![p(18.0, 9.0), p(20.5, 12.0), p(18.0, 15.0)]);
        }
        Icon::Brush => {
            circle((9.4, 16.9), 2.9);
            poly(vec![p(11.6, 15.2), p(19.8, 6.0)]);
            poly(vec![p(12.9, 16.4), p(20.2, 7.6)]);
        }
        Icon::Rect => rounded((4.0, 5.5), (20.0, 18.5), 1.6),
        Icon::Ellipse => {
            let pts: Vec<Pos2> = (0..=48)
                .map(|i| {
                    let a = i as f32 / 48.0 * std::f32::consts::TAU;
                    p(12.0 + 8.0 * a.cos(), 12.0 + 6.2 * a.sin())
                })
                .collect();
            painter.add(Shape::line(pts, stroke));
        }
        Icon::Arrow => {
            seg((4.5, 19.5), (19.0, 5.0));
            poly(vec![p(11.5, 5.0), p(19.0, 5.0), p(19.0, 12.5)]);
        }
        Icon::Line => seg((5.0, 19.0), (19.0, 5.0)),
        Icon::Text => {
            seg((5.5, 5.5), (18.5, 5.5));
            seg((12.0, 5.5), (12.0, 18.5));
            seg((8.5, 18.5), (15.5, 18.5));
        }
        Icon::Crop => {
            poly(vec![p(7.0, 3.5), p(7.0, 17.0), p(20.5, 17.0)]);
            poly(vec![p(3.5, 7.0), p(17.0, 7.0), p(17.0, 20.5)]);
        }
        Icon::Eye => {
            let pts: Vec<Pos2> = (0..=48)
                .map(|i| {
                    let a = i as f32 / 48.0 * std::f32::consts::TAU;
                    p(12.0 + 9.0 * a.cos(), 12.0 + 5.6 * a.sin())
                })
                .collect();
            painter.add(Shape::line(pts, stroke));
            circle((12.0, 12.0), 2.6);
        }
        Icon::Pencil => {
            poly(vec![
                p(4.5, 19.5),
                p(8.2, 19.5),
                p(17.9, 9.8),
                p(13.9, 5.8),
                p(4.5, 15.2),
                p(4.5, 19.5),
            ]);
            poly(vec![p(13.9, 5.8), p(17.9, 9.8)]);
            poly(vec![p(12.9, 6.8), p(16.9, 10.8)]);
        }
        Icon::Info => {
            circle((12.0, 12.0), 8.4);
            seg((12.0, 10.7), (12.0, 15.9));
            dot((12.0, 8.2), 1.0);
        }
        Icon::Plus => {
            seg((5.0, 12.0), (19.0, 12.0));
            seg((12.0, 5.0), (12.0, 19.0));
        }
        Icon::Minus => seg((5.0, 12.0), (19.0, 12.0)),
        Icon::Fit => {
            poly(vec![p(9.0, 4.5), p(4.5, 4.5), p(4.5, 9.0)]);
            poly(vec![p(15.0, 4.5), p(19.5, 4.5), p(19.5, 9.0)]);
            poly(vec![p(9.0, 19.5), p(4.5, 19.5), p(4.5, 15.0)]);
            poly(vec![p(15.0, 19.5), p(19.5, 19.5), p(19.5, 15.0)]);
        }
        Icon::Folder => poly(vec![
            p(3.5, 8.3),
            p(3.5, 17.2),
            p(5.2, 18.9),
            p(18.8, 18.9),
            p(20.5, 17.2),
            p(20.5, 9.6),
            p(11.8, 9.6),
            p(9.8, 7.6),
            p(5.2, 7.6),
            p(3.5, 8.3),
        ]),
        Icon::Save => {
            poly(vec![
                p(6.0, 4.0),
                p(4.5, 5.6),
                p(4.5, 18.4),
                p(6.1, 20.0),
                p(17.9, 20.0),
                p(19.5, 18.4),
                p(19.5, 7.8),
                p(15.7, 4.0),
                p(6.0, 4.0),
            ]);
            poly(vec![p(8.0, 4.0), p(8.0, 9.2), p(15.0, 9.2), p(15.0, 4.0)]);
            poly(vec![p(8.0, 20.0), p(8.0, 14.6), p(16.0, 14.6), p(16.0, 20.0)]);
        }
        Icon::SaveAs => {
            poly(vec![p(4.5, 5.6), p(6.1, 4.0), p(13.4, 4.0), p(15.4, 6.0)]);
            poly(vec![p(13.0, 20.0), p(6.1, 20.0), p(4.5, 18.4), p(4.5, 5.6)]);
            poly(vec![p(8.0, 4.0), p(8.0, 9.2), p(13.4, 9.2), p(13.4, 4.0)]);
            poly(vec![
                p(15.4, 20.0),
                p(15.8, 17.4),
                p(20.2, 13.0),
                p(18.2, 11.0),
                p(13.8, 15.4),
                p(13.4, 17.8),
                p(15.4, 20.0),
            ]);
        }
        Icon::Undo => {
            poly(vec![p(9.0, 6.0), p(4.5, 10.4), p(9.0, 14.8)]);
            let mut path = vec![p(4.8, 10.4), p(13.2, 10.4)];
            path.extend(arc(13.2, 15.8, 5.4, -std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2));
            path.push(p(10.0, 21.2));
            poly(path);
        }
        Icon::Redo => {
            poly(vec![p(15.0, 6.0), p(19.5, 10.4), p(15.0, 14.8)]);
            let mut path = vec![p(19.2, 10.4), p(10.8, 10.4)];
            path.extend(arc(10.8, 15.8, 5.4, std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2 * 3.0));
            path.push(p(14.0, 21.2));
            poly(path);
        }
        Icon::Panel => {
            rounded((3.5, 4.5), (20.5, 19.5), 2.5);
            seg((14.5, 4.5), (14.5, 19.5));
        }
        Icon::Help => {
            circle((12.0, 12.0), 8.4);
            let mut path = vec![p(9.6, 9.4)];
            path.extend(arc(12.0, 9.4, 2.5, std::f32::consts::PI, -0.35));
            path.push(p(12.0, 11.7));
            path.push(p(12.0, 13.2));
            poly(path);
            dot((12.0, 16.6), 0.9);
        }
        Icon::Close => {
            seg((6.0, 6.0), (18.0, 18.0));
            seg((18.0, 6.0), (6.0, 18.0));
        }
        Icon::Lock => {
            rounded((5.5, 10.5), (18.5, 19.5), 2.0);
            let mut path = vec![p(8.5, 10.5), p(8.5, 8.0)];
            path.extend(arc(12.0, 8.0, 3.5, std::f32::consts::PI, std::f32::consts::TAU));
            path.push(p(15.5, 10.5));
            poly(path);
        }
        Icon::Prev => poly(vec![p(14.5, 5.5), p(8.0, 12.0), p(14.5, 18.5)]),
        Icon::Next => poly(vec![p(9.5, 5.5), p(16.0, 12.0), p(9.5, 18.5)]),
        Icon::Copy => {
            rounded((8.5, 8.5), (19.5, 19.5), 2.0);
            poly(vec![
                p(15.5, 8.5),
                p(15.5, 6.5),
                p(13.5, 4.5),
                p(6.5, 4.5),
                p(4.5, 6.5),
                p(4.5, 13.5),
                p(6.5, 15.5),
                p(8.5, 15.5),
            ]);
        }
        Icon::Dashed => {
            seg((3.5, 12.0), (7.1, 12.0));
            seg((10.2, 12.0), (13.8, 12.0));
            seg((16.9, 12.0), (20.5, 12.0));
        }
    }
}

// ---------------------------------------------------------------------------
// Text + layout helpers
// ---------------------------------------------------------------------------

pub fn text(ui: &mut Ui, s: impl Into<String>, size: f32, color: Color32) {
    ui.label(egui::RichText::new(s.into()).size(size).color(color));
}

pub fn field_label(ui: &mut Ui, s: impl Into<String>) {
    text(ui, s, 11.0, theme::DIM);
}

pub fn hint(ui: &mut Ui, s: impl Into<String>) {
    text(ui, s, 10.5, white(120));
}

pub fn section_head(ui: &mut Ui, left: &str, right: Option<&str>) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::hover());
    let painter = ui.painter();
    let y = rect.center().y;
    painter.text(
        Pos2::new(rect.left(), y),
        Align2::LEFT_CENTER,
        left,
        FontId::proportional(10.5),
        theme::DIM,
    );
    if let Some(right) = right {
        painter.text(
            Pos2::new(rect.right(), y),
            Align2::RIGHT_CENTER,
            right,
            FontId::proportional(10.5),
            white(110),
        );
    }
    painter.hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0_f32, white(22)),
    );
}

/// A two-column information row: 72px key, flexible value, faint rule below.
pub fn kv_row(ui: &mut Ui, key: &str, value: &str, mono: bool) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::hover());
    let painter = ui.painter();
    let y = rect.center().y;
    painter.text(
        Pos2::new(rect.left(), y),
        Align2::LEFT_CENTER,
        key,
        FontId::proportional(11.0),
        theme::DIM,
    );
    let font = if mono { FontId::monospace(11.0) } else { FontId::proportional(12.0) };
    // Values are right-aligned like `.kv-v` in app.css.
    painter.text(
        Pos2::new(rect.right(), y),
        Align2::RIGHT_CENTER,
        value,
        font,
        theme::TEXT,
    );
    painter.hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0_f32, white(18)),
    );
}

// ---------------------------------------------------------------------------
// Buttons
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
pub struct Btn {
    pub icon: Option<Icon>,
    pub active: bool,
    pub primary: bool,
    /// Red tool accent (the compare toggle).
    pub accent: bool,
    pub block: bool,
}

fn paint_button_base(
    ui: &mut Ui,
    rect: Rect,
    resp: &Response,
    enabled: bool,
    style: Btn,
    radius: CornerRadius,
) -> Color32 {
    let hovered = enabled && resp.hovered();
    let fill = if style.primary {
        if !enabled {
            // Web `.tb-btn.tb-primary:disabled`: 12% white fill at 50% opacity.
            white(15)
        } else if hovered {
            Color32::WHITE
        } else {
            theme::TEXT
        }
    } else if style.accent {
        Color32::from_rgba_unmultiplied(255, 92, 92, 40)
    } else if hovered {
        white(15)
    } else if style.active {
        white(22)
    } else {
        Color32::TRANSPARENT
    };
    let painter = ui.painter();
    painter.rect_filled(rect, radius, fill);
    if style.accent {
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 92, 92, 40)),
            StrokeKind::Inside,
        );
    }
    if style.primary {
        if enabled {
            theme::BG
        } else {
            white(100)
        }
    } else if style.accent {
        TOOL
    } else if !enabled {
        disabled(theme::DIM)
    } else if style.active || hovered {
        theme::TEXT
    } else {
        theme::DIM
    }
}

/// A top-bar style button: optional leading icon, label, 30px high.
pub fn bar_button(
    ui: &mut Ui,
    label: &str,
    style: Btn,
    enabled: bool,
) -> Response {
    let enabled = is_enabled(ui, enabled);
    let font = FontId::proportional(11.5);
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font.clone(), theme::TEXT);
    let icon_w = if style.icon.is_some() { 16.0 } else { 0.0 };
    let gap = if style.icon.is_some() { 5.0 } else { 0.0 };
    let w = if style.block {
        ui.available_width()
    } else {
        (galley.size().x + icon_w + gap + 18.0).max(30.0)
    };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, BTN_H), sense(enabled, Sense::click()));
    let color = paint_button_base(ui, rect, &resp, enabled, style, CornerRadius::same(RADIUS));
    let painter = ui.painter();
    let content_w = icon_w + gap + galley.size().x;
    let mut x = rect.center().x - content_w / 2.0;
    if let Some(icon) = style.icon {
        let irect = Rect::from_center_size(
            Pos2::new(x + icon_w / 2.0, rect.center().y),
            Vec2::splat(16.0),
        );
        paint_icon(painter, irect, icon, color, 1.8);
        x += icon_w + gap;
    }
    painter.text(
        Pos2::new(x, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        font,
        color,
    );
    finish(ui, rect, resp, enabled)
}

/// An icon-only square button (30×30) for the top bar.
pub fn icon_button(ui: &mut Ui, icon: Icon, tip: &str, enabled: bool) -> Response {
    let enabled = is_enabled(ui, enabled);
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(BTN_H), sense(enabled, Sense::click()));
    let mut color = paint_button_base(ui, rect, &resp, enabled, Btn::default(), CornerRadius::same(RADIUS));
    if !enabled {
        // Web: disabled icon buttons keep the glyph faint, with no hover fill.
        color = theme::DIM.gamma_multiply(0.4);
    }
    paint_icon(ui.painter(), Rect::from_center_size(rect.center(), Vec2::splat(16.0)), icon, color, 1.8);
    finish(ui, rect, resp, enabled).on_hover_text(tip)
}

/// A square icon button that shows an active (pressed) fill.
pub fn icon_button_active(ui: &mut Ui, icon: Icon, active: bool, tip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(BTN_H), Sense::click());
    let hovered = resp.hovered();
    let painter = ui.painter();
    if active || hovered {
        painter.rect_filled(
            rect,
            CornerRadius::same(RADIUS),
            if active { white(24) } else { white(15) },
        );
    }
    let color = if active || hovered { theme::TEXT } else { theme::DIM };
    paint_icon(
        painter,
        Rect::from_center_size(rect.center(), Vec2::splat(15.0)),
        icon,
        color,
        1.9,
    );
    finish(ui, rect, resp, true).on_hover_text(tip)
}

/// Rounded pill icon button used by the floating preview dock (30×30).
pub fn pill_icon_button(
    ui: &mut Ui,
    icon: Icon,
    tip: &str,
    active: bool,
    primary: bool,
    enabled: bool,
) -> Response {
    let enabled = is_enabled(ui, enabled);
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(BTN_H), sense(enabled, Sense::click()));
    let hovered = enabled && resp.hovered();
    let (fill, color) = if primary {
        (if hovered { Color32::WHITE } else { theme::TEXT }, theme::BG)
    } else if active {
        (white(30), theme::TEXT)
    } else if hovered {
        (white(20), theme::TEXT)
    } else {
        (Color32::TRANSPARENT, theme::DIM)
    };
    ui.painter().rect_filled(rect, CornerRadius::same(15), fill);
    paint_icon(ui.painter(), Rect::from_center_size(rect.center(), Vec2::splat(16.0)), icon, color, 1.8);
    finish(ui, rect, resp, enabled).on_hover_text(tip)
}

/// A full-width primary action (34px, near-white).
pub fn primary_block(ui: &mut Ui, label: &str, enabled: bool) -> Response {
    let enabled = is_enabled(ui, enabled);
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), 34.0),
        sense(enabled, Sense::click()),
    );
    let color = paint_button_base(ui, rect, &resp, enabled, Btn { primary: true, ..Default::default() }, CornerRadius::same(RADIUS));
    let painter = ui.painter();
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(11.5),
        color,
    );
    finish(ui, rect, resp, enabled)
}

/// A full-width ghost action (32px, outlined).
pub fn ghost_block(ui: &mut Ui, label: &str, enabled: bool) -> Response {
    let enabled = is_enabled(ui, enabled);
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), 32.0),
        sense(enabled, Sense::click()),
    );
    let hovered = enabled && resp.hovered();
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(RADIUS), if hovered { white(15) } else { Color32::TRANSPARENT });
    painter.rect_stroke(
        rect,
        CornerRadius::same(RADIUS),
        Stroke::new(1.0_f32, white(36)),
        StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(11.5),
        if !enabled { disabled(theme::DIM) } else if hovered { theme::TEXT } else { theme::DIM },
    );
    finish(ui, rect, resp, enabled)
}

/// A ghost action with an explicit width (used in dense rows).
pub fn ghost_block_sized(ui: &mut Ui, label: &str, width: f32, enabled: bool) -> Response {
    let enabled = is_enabled(ui, enabled);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 30.0), sense(enabled, Sense::click()));
    let hovered = enabled && resp.hovered();
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(RADIUS), if hovered { white(15) } else { Color32::TRANSPARENT });
    painter.rect_stroke(
        rect,
        CornerRadius::same(RADIUS),
        Stroke::new(1.0_f32, white(36)),
        StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(11.0),
        if !enabled { disabled(theme::DIM) } else if hovered { theme::TEXT } else { theme::DIM },
    );
    finish(ui, rect, resp, enabled)
}

/// A full-width ghost action with a leading icon.
pub fn ghost_block_icon(ui: &mut Ui, label: &str, icon: Icon, enabled: bool) -> Response {
    let enabled = is_enabled(ui, enabled);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 32.0), sense(enabled, Sense::click()));
    let hovered = enabled && resp.hovered();
    let color = if !enabled {
        disabled(theme::DIM)
    } else if hovered {
        theme::TEXT
    } else {
        theme::DIM
    };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(RADIUS), if hovered { white(15) } else { Color32::TRANSPARENT });
    painter.rect_stroke(
        rect,
        CornerRadius::same(RADIUS),
        Stroke::new(1.0_f32, white(36)),
        StrokeKind::Inside,
    );
    let galley = painter.layout_no_wrap(label.to_owned(), FontId::proportional(11.5), color);
    let icon_w = 14.0;
    let gap = 6.0;
    let total = icon_w + gap + galley.size().x;
    let start = rect.center().x - total / 2.0;
    paint_icon(
        painter,
        Rect::from_center_size(Pos2::new(start + icon_w / 2.0, rect.center().y), Vec2::splat(14.0)),
        icon,
        color,
        1.8,
    );
    painter.text(
        Pos2::new(start + icon_w + gap, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(11.5),
        color,
    );
    finish(ui, rect, resp, enabled)
}

/// A small frameless text link (清除 / 已套用 …).
pub fn link_button(ui: &mut Ui, label: &str, enabled: bool) -> Response {
    let enabled = is_enabled(ui, enabled);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), FontId::proportional(10.5), theme::TEXT);
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(galley.size().x + 6.0, 22.0),
        sense(enabled, Sense::click()),
    );
    let hovered = enabled && resp.hovered();
    ui.painter().text(
        Pos2::new(rect.left() + 3.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(10.5),
        if !enabled { disabled(theme::DIM) } else if hovered { theme::TEXT } else { theme::DIM },
    );
    finish(ui, rect, resp, enabled)
}

/// A pill chip (ratios, output format, percentages), 28px high.
pub fn chip(ui: &mut Ui, label: &str, active: bool, enabled: bool) -> Response {
    let enabled = is_enabled(ui, enabled);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), FontId::proportional(10.5), theme::TEXT);
    let w = galley.size().x + 22.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 28.0), sense(enabled, Sense::click()));
    let hovered = enabled && resp.hovered();
    let painter = ui.painter();
    let radius = CornerRadius::same(14);
    let fill = if active {
        white(26)
    } else if hovered {
        white(15)
    } else {
        Color32::TRANSPARENT
    };
    painter.rect_filled(rect, radius, fill);
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0_f32, if active { white(90) } else { white(34) }),
        StrokeKind::Inside,
    );
    let color = if !enabled {
        disabled(theme::DIM)
    } else if active || hovered {
        theme::TEXT
    } else {
        theme::DIM
    };
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(10.5),
        color,
    );
    finish(ui, rect, resp, enabled)
}

pub fn sep_v(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(9.0, 18.0), Sense::hover());
    ui.painter().vline(
        rect.center().x,
        rect.y_range(),
        Stroke::new(1.0_f32, white(26)),
    );
}

// ---------------------------------------------------------------------------
// Swatches
// ---------------------------------------------------------------------------

/// A colour swatch. The selection ring pops in (0.8 -> 1.12 -> 1 over 180 ms)
/// and fades out on the previously selected swatch.
pub fn swatch(ui: &mut Ui, anim_id: Id, color: Color32, active: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::click());
    let hover = motion::blend(ui.ctx(), anim_id.with("hover"), resp.hovered(), 0.12);
    let ring = motion::blend(ui.ctx(), anim_id, active, 0.12);
    let scale = 1.0 + 0.08 * hover;
    let center = rect.center();
    let painter = ui.painter();
    // web `.swatch`: 20px circle, `border: 1px solid var(--line)`,
    // `:hover { transform: scale(1.08) }`
    painter.circle_filled(center, 10.0 * scale, color);
    painter.circle_stroke(
        center,
        10.0 * scale - 0.5,
        Stroke::new(1.0_f32, white(36)),
    );
    // web `#style-dock .swatch.is-active { box-shadow: 0 0 0 2px var(--brand) }`
    if ring > 0.01 {
        painter.circle_stroke(
            center,
            11.0 * scale,
            Stroke::new(2.0_f32, theme::TEXT.gamma_multiply(ring)),
        );
    }
    finish(ui, rect, resp, true)
}

/// The dashed toggle lives in the swatch row as a round icon swatch.
pub fn toggle_swatch(ui: &mut Ui, icon: Icon, active: bool, tip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::click());
    let hovered = resp.hovered();
    let painter = ui.painter();
    let fill = if active {
        theme::TEXT
    } else if hovered {
        white(24)
    } else {
        SURFACE
    };
    painter.circle_filled(rect.center(), 10.0, fill);
    painter.circle_stroke(rect.center(), 10.0, Stroke::new(1.0_f32, white(36)));
    let color = if active {
        theme::BG
    } else if hovered {
        theme::TEXT
    } else {
        theme::DIM
    };
    // The web's dashed toggle is three short dashes (14px); drawn as round dots
    // so they stay distinct at this size (a stroked polyline merges visually).
    if matches!(icon, Icon::Dashed) {
        for dx in [-4.0_f32, 0.0, 4.0] {
            painter.circle_filled(rect.center() + Vec2::new(dx, 0.0), 1.1, color);
        }
    } else {
        paint_icon(
            painter,
            Rect::from_center_size(rect.center(), Vec2::splat(14.0)),
            icon,
            color,
            1.8,
        );
    }
    finish(ui, rect, resp, true).on_hover_text(tip)
}

// ---------------------------------------------------------------------------
// Slider (4px track, 13px near-white thumb)
// ---------------------------------------------------------------------------

pub fn slider(
    ui: &mut Ui,
    value: &mut f32,
    min: f32,
    max: f32,
    width: f32,
    enabled: bool,
) -> Response {
    let enabled = is_enabled(ui, enabled);
    let (rect, mut resp) = ui.allocate_exact_size(Vec2::new(width, 16.0), Sense::click_and_drag());
    let before = *value;
    let mut v = (*value).clamp(min, max);
    if enabled && (resp.dragged() || resp.clicked()) {
        if let Some(pos) = ui.ctx().input(|i| i.pointer.interact_pos()) {
            let t = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            v = min + (max - min) * t;
            *value = v;
        }
    }
    if (*value - before).abs() > f32::EPSILON {
        resp.mark_changed();
    }
    let t = ((v - min) / (max - min)).clamp(0.0, 1.0);
    let painter = ui.painter();
    let cy = rect.center().y;
    // web `.range::-webkit-slider-runnable-track`: height 4, radius 2,
    // background rgba(255,255,255,.16) — no filled progress segment.
    let track = Rect::from_min_max(
        Pos2::new(rect.left(), cy - 2.0),
        Pos2::new(rect.right(), cy + 2.0),
    );
    painter.rect_filled(track, CornerRadius::same(2), white(41));
    // web thumb: 13px total, `border: 2px solid var(--panel)`, background brand.
    let thumb = Pos2::new(rect.left() + rect.width() * t, cy);
    let brand = if enabled { theme::TEXT } else { white(90) };
    painter.circle_filled(thumb, 6.5, theme::PANEL);
    painter.circle_filled(thumb, 4.5, brand);
    finish(ui, rect, resp, enabled)
}

/// A `label … value` header above sliders/fields.
pub fn slider_label(ui: &mut Ui, label: &str, value: &str, width: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 12.0), Sense::hover());
    let painter = ui.painter();
    painter.text(
        Pos2::new(rect.left(), rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(9.5),
        theme::DIM,
    );
    painter.text(
        Pos2::new(rect.right(), rect.center().y),
        Align2::RIGHT_CENTER,
        value,
        FontId::monospace(9.5),
        theme::TEXT,
    );
}

// ---------------------------------------------------------------------------
// Numeric text field with a persistent buffer (so typing isn't overwritten by
// the state value every frame) and commit-on-change semantics.
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
struct FieldBuf {
    text: String,
    focused: bool,
}

fn field_id(key: impl std::hash::Hash) -> egui::Id {
    egui::Id::new("lumen.ui.field").with(key)
}

/// Draws a small dark numeric input. Returns `Some(value)` when the user typed
/// a valid positive number this frame.
pub fn number_field(
    ui: &mut Ui,
    key: impl std::hash::Hash + Clone,
    value: Option<u32>,
    placeholder: &str,
    enabled: bool,
    width: f32,
) -> Option<u32> {
    let enabled = is_enabled(ui, enabled);
    let id = field_id(key.clone());
    let mut buf: FieldBuf = ui.ctx().data(|d| d.get_temp(id)).unwrap_or_default();
    if !buf.focused {
        buf.text = value.map(|v| v.to_string()).unwrap_or_default();
    }
    if buf.text.is_empty() {
        buf.text = String::new();
    }
    let mut changed = None;
    let desired = width;
    let weak = ui.visuals().weak_text_color();
    ui.visuals_mut().weak_text_color = Some(white(110));
    let resp = ui.add_enabled(
        enabled,
        egui::TextEdit::singleline(&mut buf.text)
            .id(id)
            .hint_text(placeholder)
            .font(FontId::monospace(11.0))
            .text_color(theme::TEXT)
            .background_color(theme::BG)
            .margin(egui::Margin::symmetric(8, 6))
            // `desired` is the total widget width: TextEdit adds 8px padding per side.
            .desired_width((desired - 16.0).max(24.0))
            .min_size(Vec2::new(desired, 26.0))
            .vertical_align(Align::Center),
    );
    ui.visuals_mut().weak_text_color = Some(weak);
    if resp.changed() {
        let trimmed = buf.text.trim();
        if trimmed.is_empty() {
            return Some(0).filter(|_| false);
        }
        if let Ok(parsed) = trimmed.parse::<u32>() {
            if parsed > 0 {
                changed = Some(parsed);
            }
        }
    }
    // Draw the 1px outline the theme asks for (TextEdit's own frame is a fill
    // only in egui, so add the stroke ourselves).
    let rect = resp.rect.expand2(Vec2::new(1.0, 1.0));
    let focused = resp.has_focus();
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(7),
        Stroke::new(1.0_f32, if focused { theme::TEXT } else { white(38) }),
        StrokeKind::Outside,
    );
    buf.focused = focused;
    ui.ctx().data_mut(|d| d.insert_temp(id, buf));
    changed
}

// ---------------------------------------------------------------------------
// Finish: cursor + focus ring for custom-painted buttons.
// ---------------------------------------------------------------------------

fn finish(ui: &mut Ui, rect: Rect, resp: Response, enabled: bool) -> Response {
    if enabled && resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    if enabled && resp.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(1.5),
            CornerRadius::same(RADIUS + 1),
            Stroke::new(1.5_f32, white(120)),
            StrokeKind::Outside,
        );
    }
    resp
}

/// Place a child UI at an absolute rect inside the current layer.
pub fn at_rect(ui: &mut Ui, rect: Rect, add: impl FnOnce(&mut Ui)) {
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(Align::Min)));
    child.set_clip_rect(ui.clip_rect().intersect(rect));
    add(&mut child);
}

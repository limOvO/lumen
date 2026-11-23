// Shared zoom controls: the floating HUD (preview, bottom-right) and the
// vertical stack folded into the left rail (editor).

use egui::{Align, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2};

use crate::state::AppState;
use crate::theme;

use super::widgets::{self, Icon};

/// The "fit to window" scale for the current document, if any.
pub fn fit_scale(ctx: &egui::Context, state: &AppState) -> Option<f32> {
    let (w, h) = state.doc_size()?;
    if w == 0 || h == 0 {
        return None;
    }
    let screen = ctx.screen_rect();
    Some((screen.width() / w as f32).min(screen.height() / h as f32).min(1.0))
}

/// Multiply the zoom around the canvas centre. `floor` prevents zooming out
/// past the fitted view (same clamp as the web build).
pub fn zoom_by(state: &mut AppState, factor: f32, floor: f32) {
    let old = state.view.scale.max(1e-4);
    let mut next = (old * factor).clamp(0.02, 16.0);
    if factor < 1.0 {
        next = next.max(floor.min(old).max(0.02));
    }
    state.view.offset = state.view.offset * (next / old);
    state.view.scale = next;
    state.view.fit = false;
}

pub fn zoom_in(state: &mut AppState, floor: f32) {
    zoom_by(state, 1.25, floor);
}

pub fn zoom_out(state: &mut AppState, floor: f32) {
    zoom_by(state, 0.8, floor);
}

pub fn zoom_fit(state: &mut AppState) {
    state.view.fit = true;
}

fn hud_icon(ui: &mut Ui, icon: Icon, enabled: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::click());
    let hovered = enabled && resp.hovered();
    let painter = ui.painter();
    if hovered {
        painter.rect_filled(rect, CornerRadius::same(8), widgets::white(18));
    }
    let color = if !enabled {
        widgets::disabled(theme::DIM)
    } else if hovered {
        theme::TEXT
    } else {
        theme::DIM
    };
    widgets::paint_icon(
        painter,
        Rect::from_center_size(rect.center(), Vec2::splat(14.0)),
        icon,
        color,
        2.1,
    );
    if enabled && hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn hud_value(ui: &mut Ui, text: &str, width: f32, enabled: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 30.0), Sense::click());
    let hovered = enabled && resp.hovered();
    if hovered {
        ui.painter().rect_filled(rect, CornerRadius::same(8), widgets::white(18));
    }
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        FontId::proportional(11.0),
        if !enabled {
            widgets::disabled(theme::DIM)
        } else if hovered {
            theme::TEXT
        } else {
            theme::TEXT
        },
    );
    if enabled && hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn hud_sep(ui: &mut Ui, vertical: bool) {
    if vertical {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(22.0, 5.0), Sense::hover());
        ui.painter().hline(
            rect.x_range(),
            rect.center().y,
            Stroke::new(1.0_f32, widgets::white(26)),
        );
    } else {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(7.0, 16.0), Sense::hover());
        ui.painter().vline(
            rect.center().x,
            rect.y_range(),
            Stroke::new(1.0_f32, widgets::white(26)),
        );
    }
}

/// The +/-/percent/fit controls. `vertical` matches the in-rail stack.
pub fn stack(ui: &mut Ui, state: &mut AppState, vertical: bool) {
    let has_doc = state.doc.is_some();
    let floor = fit_scale(ui.ctx(), state).unwrap_or(0.02);
    let pct = if has_doc {
        format!("{:.0}%", state.view.scale * 100.0)
    } else {
        "—".to_string()
    };
    let enabled = ui.is_enabled() && has_doc;
    let mut body = |ui: &mut Ui| {
        let resp = hud_icon(ui, Icon::Plus, enabled);
        if resp.clicked() && enabled {
            zoom_in(state, floor);
        }
        resp.on_hover_text("放大（+）");
        let resp = hud_value(ui, &pct, if vertical { 30.0 } else { 48.0 }, enabled);
        if resp.clicked() && enabled {
            zoom_fit(state);
        }
        resp.on_hover_text("目前縮放比例，點擊適應視窗");
        let resp = hud_icon(ui, Icon::Minus, enabled);
        if resp.clicked() && enabled {
            zoom_out(state, floor);
        }
        resp.on_hover_text("縮小（-）");
        hud_sep(ui, vertical);
        let resp = hud_icon(ui, Icon::Fit, enabled);
        if resp.clicked() && enabled {
            zoom_fit(state);
        }
        resp.on_hover_text("適應視窗（0）");
    };
    if vertical {
        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(0.0, 1.0);
            body(ui);
        });
    } else {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(2.0, 0.0);
            body(ui);
        });
    }
}

/// Frame used by the floating preview HUD.
pub fn floating_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::from_rgba_unmultiplied(16, 16, 18, 190))
        .inner_margin(egui::Margin::symmetric(3, 3))
        .corner_radius(10.0)
        .stroke(Stroke::new(1.0_f32, widgets::white(28)))
}

/// Frame used by the in-rail stack.
pub fn rail_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::from_rgba_unmultiplied(35, 35, 36, 220))
        .inner_margin(egui::Margin::symmetric(3, 3))
        .corner_radius(10.0)
}

/// Alignment helper for callers that place the stack themselves.
pub fn stack_height(vertical: bool) -> f32 {
    if vertical {
        3.0 * 30.0 + 22.0 + 5.0 + 6.0
    } else {
        36.0
    }
}

/// Keep the imports used by the placement helper honest.
#[allow(dead_code)]
fn _placement_helpers() -> (Rect, Pos2, Align) {
    (Rect::NOTHING, Pos2::ZERO, Align::Center)
}

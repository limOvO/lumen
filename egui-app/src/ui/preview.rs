// Preview-mode chrome: floating caption chip (top-left), icon dock
// (counter · ⓘ info · ✎ edit) at the top-right, edge navigation arrows and the
// wheel-zoom HUD at the bottom-right. Preview has no top bar / rail / panels.

use egui::{Align2, Color32, FontId, Id, Order, Pos2, Rect, Sense, Stroke, Vec2};

use crate::state::{AppState, Phase};
use crate::theme;

use super::local;
use super::widgets::{self, Icon};
use super::zoom;

pub fn chrome(ctx: &egui::Context, state: &mut AppState) {
    if state.phase != Phase::Preview {
        return;
    }
    let t = state.locale;
    let screen = ctx.screen_rect();
    let has_doc = state.doc.is_some();

    // ------------------------------------------------------------------
    // Caption chip (top-left): "name · W×H"
    // ------------------------------------------------------------------
    let caption = match state.doc.as_ref() {
        Some(doc) => format!("{} · {}×{}", doc.name, doc.width, doc.height),
        None => "—".to_string(),
    };
    egui::Area::new(Id::new("preview-caption"))
        .order(Order::Foreground)
        .interactable(false)
        .fixed_pos(Pos2::new(screen.left() + 14.0, screen.top() + 14.0))
        .show(ctx, |ui| {
            super::topbar::caption_pill(ui, &caption);
        });

    // ------------------------------------------------------------------
    // Dock (top-right): counter · separator · info · edit
    // ------------------------------------------------------------------
    let info_open = state.show_info;
    let shift = ctx.animate_bool_with_time(Id::new("preview-dock-shift"), info_open, 0.20);
    let dock_resp = egui::Area::new(Id::new("preview-dock"))
        .order(Order::Foreground)
        .anchor(Align2::RIGHT_TOP, Vec2::new(-14.0 - 316.0 * shift, 14.0))
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(Color32::from_rgba_unmultiplied(21, 21, 23, 235))
                .stroke(Stroke::new(1.0_f32, widgets::white(40)))
                .corner_radius(egui::CornerRadius::same(19))
                .inner_margin(egui::Margin::symmetric(4, 3))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(2.0, 0.0);
                        // counter
                        let count = if has_doc {
                            format!(
                                "{} / {}",
                                state.index + 1,
                                state.siblings.len().max(1)
                            )
                        } else {
                            "— / —".to_string()
                        };
                        let (rect, _) = ui.allocate_exact_size(Vec2::new(58.0, 30.0), Sense::hover());
                        ui.painter().text(
                            rect.center(),
                            Align2::CENTER_CENTER,
                            count,
                            FontId::proportional(11.5),
                            theme::DIM,
                        );
                        // separator
                        let (sep, _) = ui.allocate_exact_size(Vec2::new(5.0, 16.0), Sense::hover());
                        ui.painter().vline(
                            sep.center().x,
                            sep.y_range(),
                            Stroke::new(1.0_f32, widgets::white(30)),
                        );
                        let info_tip = if info_open {
                            t.tr("關閉資訊浮卡（I）")
                        } else {
                            t.tr("顯示資訊浮卡（I）")
                        };
                        if widgets::pill_icon_button(
                            ui,
                            Icon::Info,
                            &info_tip,
                            info_open,
                            false,
                            true,
                        )
                        .clicked()
                        {
                            state.show_info = !state.show_info;
                        }
                        let edit_tip = t.tr("進入編輯");
                        if widgets::pill_icon_button(ui, Icon::Pencil, &edit_tip, false, true, has_doc)
                            .clicked()
                        {
                            state.phase = Phase::Edit;
                        }
                    });
                });
        });
    local::with(ctx, |l| l.preview_dock = dock_resp.response.rect);

    // ------------------------------------------------------------------
    // Edge navigation arrows (only near the left/right edge, and never while
    // the info card is open).
    // ------------------------------------------------------------------
    let can_nav = has_doc && state.siblings.len() > 1 && !info_open;
    if can_nav {
        let pointer = ctx.input(|i| i.pointer.hover_pos());
        let near = |x: f32| {
            pointer
                .map(|_| x < screen.left() + 120.0 || x > screen.right() - 120.0)
                .unwrap_or(false)
        };
        if pointer.map(|p| near(p.x)).unwrap_or(false) {
            let cy = screen.center().y;
            let left = egui::Area::new(Id::new("preview-prev"))
                .order(Order::Foreground)
                .fixed_pos(Pos2::new(screen.left() + 18.0, cy - 22.0))
                .show(ctx, |ui| {
                    nav_button(ui, Icon::Prev, &t.tr("上一張（←）"))
                });
            if left.inner.clicked() {
                state.push_action(crate::state::UiAction::Prev);
            }
            let right = egui::Area::new(Id::new("preview-next"))
                .order(Order::Foreground)
                .fixed_pos(Pos2::new(screen.right() - 18.0 - 44.0, cy - 22.0))
                .show(ctx, |ui| nav_button(ui, Icon::Next, &t.tr("下一張（→）")));
            if right.inner.clicked() {
                state.push_action(crate::state::UiAction::Next);
            }
        }
    }

    // ------------------------------------------------------------------
    // Zoom HUD (bottom-right).
    // ------------------------------------------------------------------
    egui::Area::new(Id::new("preview-zoom"))
        .order(Order::Foreground)
        .anchor(Align2::RIGHT_BOTTOM, Vec2::new(-14.0, -14.0))
        .show(ctx, |ui| {
            zoom::floating_frame().show(ui, |ui| {
                zoom::stack(ui, state, false);
            });
        });
}

fn nav_button(ui: &mut egui::Ui, icon: Icon, tip: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(44.0), Sense::click());
    let hovered = resp.hovered();
    let painter = ui.painter();
    painter.circle_filled(
        rect.center(),
        22.0,
        if hovered {
            Color32::from_rgba_unmultiplied(48, 48, 50, 245)
        } else {
            Color32::from_rgba_unmultiplied(21, 21, 23, 235)
        },
    );
    painter.circle_stroke(rect.center(), 22.0, Stroke::new(1.0_f32, widgets::white(40)));
    widgets::paint_icon(
        painter,
        Rect::from_center_size(rect.center(), Vec2::splat(18.0)),
        icon,
        if hovered { theme::TEXT } else { theme::DIM },
        1.9,
    );
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.on_hover_text(tip)
}

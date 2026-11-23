// Preview-mode "peek" info card: a floating panel on the right with the same
// information as the editor's 資訊 tab. Opens on the ⓘ dock button / I key,
// closes when clicking anywhere outside (or Esc, handled by app.rs).

use egui::{Color32, CornerRadius, Id, Order, Pos2, Rect, Vec2};

use crate::state::{AppState, Phase};
use crate::theme;

use super::local;
use super::widgets;

pub fn show(ctx: &egui::Context, state: &mut AppState) {
    if state.phase != Phase::Preview {
        return;
    }
    let open = state.show_info;
    let t = ctx.animate_bool_with_time(Id::new("peek-info-anim"), open, 0.20);
    if !open && t <= 0.001 {
        return;
    }
    let screen = ctx.screen_rect();
    let width = theme::PANEL_W;
    let pos = Pos2::new(
        screen.right() - 16.0 - width + (1.0 - t) * 18.0,
        screen.top() + 16.0,
    );
    let size = Vec2::new(width, screen.height() - 32.0);
    let card_rect = Rect::from_min_size(pos, size);

    let response = egui::Area::new(Id::new("peek-info"))
        .order(Order::Foreground)
        .fixed_pos(pos)
        .show(ctx, |ui| {
            ui.set_opacity(t);
            egui::Frame::new()
                .fill(Color32::from_rgb(38, 38, 40))
                .stroke(egui::Stroke::new(1.0_f32, widgets::white(60)))
                .corner_radius(CornerRadius::same(12))
                .shadow(egui::epaint::Shadow {
                    offset: [0, 24],
                    blur: 60,
                    spread: 0,
                    color: Color32::from_rgba_unmultiplied(0, 0, 0, 120),
                })
                .inner_margin(egui::Margin::ZERO)
                .show(ui, |ui| {
                    ui.set_width(width - 2.0);
                    ui.set_height(size.y - 2.0);
                    egui::ScrollArea::vertical()
                        .id_salt("peek-info-scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.add_space(4.0);
                            egui::Frame::new()
                                .inner_margin(egui::Margin::symmetric(14, 10))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    super::inspector::info_page(ui, state);
                                });
                        });
                });
        });
    let rect = response.response.rect;
    let _ = card_rect;

    // Click outside closes the card; presses on the dock button are ignored so
    // the toggle itself can close it.
    if state.show_info && ctx.input(|i| i.pointer.any_pressed()) {
        if let Some(p) = ctx.input(|i| i.pointer.interact_pos()) {
            let on_card = rect.contains(p);
            let on_dock = local::get(ctx, |l| l.preview_dock).contains(p);
            if !on_card && !on_dock {
                state.show_info = false;
            }
        }
    }
    // Keep animating while the card is fading out.
    if (t - 0.0).abs() < 0.999 {
        ctx.request_repaint();
    }
}

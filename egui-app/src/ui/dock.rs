// Floating annotation style dock (bottom centre of the canvas): colour
// swatches + custom colour, dashed toggle (hidden for text), width, opacity and
// font size. Asleep (dimmed) until the pointer approaches, like the web build.

use egui::{Align, Align2, Color32, CornerRadius, Id, Layout, Order, Rect, Sense, Stroke, Vec2};

use crate::state::{AppState, Phase, Tool};
use crate::theme;

use super::local;
use super::motion;
use super::widgets::{self, Icon};

pub const TOOL_COLORS: [Color32; 8] = [
    Color32::from_rgb(255, 92, 92),
    Color32::from_rgb(255, 180, 87),
    Color32::from_rgb(255, 217, 61),
    Color32::from_rgb(61, 220, 151),
    Color32::from_rgb(91, 157, 255),
    Color32::from_rgb(192, 132, 252),
    Color32::from_rgb(255, 255, 255),
    Color32::from_rgb(20, 23, 28),
];

fn is_drawing_tool(tool: Tool) -> bool {
    matches!(
        tool,
        Tool::Brush | Tool::Line | Tool::Rect | Tool::Ellipse | Tool::Arrow | Tool::Text
    )
}

/// Anchor offset for the dock relative to the screen: centred on the canvas
/// area, `bottom: 56px` above the canvas bottom, plus the web's idle
/// `translateY(58%)` slide when asleep. Pure so it can be unit-tested.
pub fn dock_offset(canvas: Rect, screen: Rect, wake: f32) -> Vec2 {
    let idle_slide = (1.0 - wake) * 0.58 * 54.0;
    Vec2::new(
        canvas.center().x - screen.center().x,
        (canvas.bottom() - screen.bottom() - 56.0) + idle_slide,
    )
}

/// Debug hook used by scripts/parity/verify.py (LUMEN_DEBUG_DOCK=1).
fn debug_log(rect: Rect, canvas: Rect, screen: Rect) {
    if std::env::var_os("LUMEN_DEBUG_DOCK").is_some() {
        eprintln!(
            "[dock] rect {:.1} {:.1} {:.1} {:.1} canvas {:.1} {:.1} {:.1} {:.1} screen {:.1} {:.1}",
            rect.left(), rect.top(), rect.width(), rect.height(),
            canvas.left(), canvas.top(), canvas.width(), canvas.height(),
            screen.width(), screen.height()
        );
    }
}

pub fn show(ctx: &egui::Context, state: &mut AppState) {
    // Fade + rise in when a drawing tool is picked (240 ms), fade + drop out
    // when leaving the tool or the editor (180 ms), web timings.
    let target =
        state.phase == Phase::Edit && state.doc.is_some() && is_drawing_tool(state.tool);
    let nominal = if target { motion::DOCK_IN } else { motion::DOCK_OUT };
    let enter = motion::blend(ctx, Id::new("style-dock-anim"), target, nominal);
    if !target && enter <= 0.001 {
        return;
    }
    let t = state.locale;
    let screen = ctx.screen_rect();
    let pointer = ctx.input(|i| i.pointer.hover_pos());
    let was_hovered = local::get(ctx, |l| match pointer {
        Some(p) => l.style_dock.expand(8.0).contains(p),
        None => false,
    });
    let wake = motion::blend(ctx, Id::new("style-dock-wake"), was_hovered, 0.20);
    // Idle dimming stays, multiplied by the enter/exit progress.
    let opacity = enter * (0.45 + 0.55 * wake);
    // Position like the web: centred on the *canvas area* (rail + inspector
    // taken into account) and 26px above the status bar. The dock previously
    // centred on the window, which pushed it over the picture on the right.
    let collapsed = local::get(ctx, |l| l.inspector_collapsed);
    let insp_w = if collapsed { 0.0 } else { local::get(ctx, |l| l.inspector_width) };
    let canvas = Rect::from_min_max(
        egui::Pos2::new(screen.left() + theme::RAIL_W, screen.top() + theme::TOPBAR_H),
        egui::Pos2::new(screen.right() - insp_w, screen.bottom() - theme::STATUS_H),
    );
    let dock_w = 600.0_f32.min((canvas.width() - 96.0).max(280.0));
    let offset = dock_offset(canvas, screen, wake)
        + Vec2::new(0.0, motion::slide_offset(enter, 18.0));
    let offset_x = offset.x;
    let offset_y = offset.y;
    let resp = egui::Area::new(Id::new("style-dock"))
        .order(Order::Foreground)
        .anchor(Align2::CENTER_BOTTOM, Vec2::new(offset_x, offset_y))
        .show(ctx, |ui| {
            ui.set_min_width(dock_w);
            ui.set_max_width(dock_w);
            ui.set_opacity(opacity);
            egui::Frame::new()
                .fill(Color32::from_rgba_unmultiplied(24, 24, 26, 245))
                .stroke(Stroke::new(1.0_f32, widgets::white(40)))
                .corner_radius(CornerRadius::same(14))
                .inner_margin(egui::Margin::symmetric(14, 10))
                .show(ui, |ui| {
                    ui.set_width(dock_w - 28.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(14.0, 8.0);
                        // Colour swatches + custom colour + dashed toggle.
                        // Web centres the swatch row against the slider stacks;
                        // a plain nested horizontal aligned them to the top.
                        ui.allocate_ui_with_layout(
                            Vec2::new(252.0, 30.0),
                            Layout::left_to_right(Align::Center),
                            |ui| {
                            ui.spacing_mut().item_spacing.x = 5.0;
                            for color in TOOL_COLORS {
                                let active = state.style.color == color;
                                if widgets::swatch(ui, color, active).clicked() {
                                    state.style.color = color;
                                }
                            }
                            ui.add_space(2.0);
                            let (rect, c_resp) =
                                ui.allocate_exact_size(Vec2::splat(20.0), Sense::click());
                            let painter = ui.painter();
                            painter.circle_filled(rect.center(), 10.0, widgets::SURFACE);
                            painter.circle_stroke(
                                rect.center(),
                                10.0,
                                Stroke::new(1.0_f32, widgets::white(45)),
                            );
                            widgets::paint_icon(
                                painter,
                                Rect::from_center_size(rect.center(), Vec2::splat(13.0)),
                                Icon::Plus,
                                theme::DIM,
                                2.0,
                            );
                            egui::Popup::from_toggle_button_response(&c_resp).show(|ui| {
                                ui.set_min_width(220.0);
                                egui::color_picker::color_picker_color32(
                                    ui,
                                    &mut state.style.color,
                                    egui::color_picker::Alpha::Opaque,
                                );
                            });
                            if state.tool != Tool::Text
                                && widgets::toggle_swatch(
                                    ui,
                                    Icon::Dashed,
                                    state.style.dashed,
                                    &t.tr("虛線（筆跡／直線／圖案）"),
                                )
                                .clicked()
                            {
                                state.style.dashed = !state.style.dashed;
                            }
                            },
                        );

                        // Width / opacity / font size (label above its slider).
                        let mut width = state.style.size;
                        slider_stack(ui, &t.tr("粗細"), &format!("{:.0}", width), &mut width, 1.0, 64.0);
                        if width != state.style.size {
                            state.style.size = width.round().max(1.0);
                        }
                        let mut opacity_pct = state.style.opacity * 100.0;
                        slider_stack(
                            ui,
                            &t.tr("透明度"),
                            &format!("{:.0}%", opacity_pct),
                            &mut opacity_pct,
                            5.0,
                            100.0,
                        );
                        if (opacity_pct - state.style.opacity * 100.0).abs() > 0.01 {
                            state.style.opacity = (opacity_pct / 100.0).clamp(0.05, 1.0);
                        }
                        if state.tool == Tool::Text {
                            let mut font = state.style.font_size;
                            slider_stack(
                                ui,
                                &t.tr("字級"),
                                &format!("{:.0}", font),
                                &mut font,
                                10.0,
                                160.0,
                            );
                            if font != state.style.font_size {
                                state.style.font_size = font.round().max(10.0);
                            }
                        }
                    });
                });
        });
    local::with(ctx, |l| l.style_dock = resp.response.rect);
    debug_log(resp.response.rect, canvas, screen);
}

fn slider_stack(
    ui: &mut egui::Ui,
    label: &str,
    value_text: &str,
    value: &mut f32,
    min: f32,
    max: f32,
) {
    ui.allocate_ui_with_layout(
        Vec2::new(96.0, 30.0),
        Layout::top_down(Align::Min),
        |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(0.0, 2.0);
            widgets::slider_label(ui, label, value_text, 96.0);
            widgets::slider(ui, value, min, max, 96.0, true);
        },
    );
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dock_is_centred_on_the_canvas_and_56px_above_it() {
        let screen = Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(1280.0, 820.0));
        let canvas = Rect::from_min_max(
            egui::Pos2::new(52.0, 46.0),
            egui::Pos2::new(980.0, 796.0),
        );
        let off = dock_offset(canvas, screen, 1.0);
        // the area is anchored CENTER_BOTTOM: screen centre + offset = canvas centre
        assert!((screen.center().x + off.x - canvas.center().x).abs() < 0.5);
        // bottom edge sits 56px above the canvas bottom (866 - 56 = 740)
        assert!((screen.bottom() + off.y - (canvas.bottom() - 56.0)).abs() < 0.5);
        // idle slides down by 58% of the dock height
        let idle = dock_offset(canvas, screen, 0.0);
        assert!((idle.y - off.y - 31.32).abs() < 0.01);
    }
}

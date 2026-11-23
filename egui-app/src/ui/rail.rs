// Left tool rail: tools (V B R O A L T C) with the active indicator, plus the
// vertical zoom stack pinned to the bottom — exactly like the web editor.

use egui::{Align2, FontId, Layout, Pos2, Rect, Sense, Stroke, Ui, Vec2};

use crate::state::{AppState, Phase, Tool};
use crate::theme;

use super::widgets::{self, Icon};
use super::zoom;

struct ToolEntry {
    tool: Tool,
    icon: Icon,
    key: &'static str,
    tip: &'static str,
}

const TOOLS: [ToolEntry; 8] = [
    ToolEntry { tool: Tool::Move, icon: Icon::Move, key: "V", tip: "移動／平移" },
    ToolEntry { tool: Tool::Brush, icon: Icon::Brush, key: "B", tip: "畫筆" },
    ToolEntry { tool: Tool::Rect, icon: Icon::Rect, key: "R", tip: "矩形" },
    ToolEntry { tool: Tool::Ellipse, icon: Icon::Ellipse, key: "O", tip: "圓框" },
    ToolEntry { tool: Tool::Arrow, icon: Icon::Arrow, key: "A", tip: "箭頭（點兩下：起點 → 終點）" },
    ToolEntry { tool: Tool::Line, icon: Icon::Line, key: "L", tip: "直線（點兩下：起點 → 終點）" },
    ToolEntry { tool: Tool::Text, icon: Icon::Text, key: "T", tip: "文字" },
    ToolEntry { tool: Tool::Crop, icon: Icon::Crop, key: "C", tip: "裁切" },
];

pub fn show(ctx: &egui::Context, state: &mut AppState) {
    if state.phase != Phase::Edit {
        return;
    }
    let t = state.locale;
    egui::SidePanel::left("toolrail")
        .exact_width(theme::RAIL_W)
        .resizable(false)
        .show_separator_line(false)
        .frame(
            egui::Frame::new()
                .fill(widgets::RAIL)
                .inner_margin(egui::Margin::ZERO),
        )
        .show(ctx, |ui| {
            let panel_rect = ui.max_rect();
            ui.painter().vline(
                panel_rect.right() - 0.5,
                panel_rect.y_range(),
                Stroke::new(1.0_f32, widgets::white(22)),
            );
            ui.vertical_centered(|ui| {
                ui.add_space(8.0);
                ui.spacing_mut().item_spacing = Vec2::new(0.0, 4.0);
                for (i, entry) in TOOLS.iter().enumerate() {
                    if i == 7 {
                        // separator before the crop tool
                        ui.add_space(2.0);
                        let (sep, _) =
                            ui.allocate_exact_size(Vec2::new(24.0, 1.0), Sense::hover());
                        ui.painter().hline(
                            sep.x_range(),
                            sep.center().y,
                            Stroke::new(1.0_f32, widgets::white(22)),
                        );
                        ui.add_space(2.0);
                    }
                    let active = state.tool == entry.tool;
                    let (rect, resp) =
                        ui.allocate_exact_size(Vec2::splat(34.0), Sense::click());
                    let hovered = resp.hovered();
                    let painter = ui.painter();
                    if active {
                        painter.rect_filled(rect, egui::CornerRadius::same(9), widgets::white(20));
                    } else if hovered {
                        painter.rect_filled(rect, egui::CornerRadius::same(9), widgets::white(14));
                    }
                    let color = if active || hovered { theme::TEXT } else { theme::DIM };
                    widgets::paint_icon(
                        painter,
                        Rect::from_center_size(rect.center(), Vec2::splat(19.0)),
                        entry.icon,
                        color,
                        1.8,
                    );
                    if active {
                        painter.rect_filled(
                            Rect::from_min_size(
                                Pos2::new(panel_rect.left() + 3.0, rect.center().y - 9.0),
                                Vec2::new(3.0, 18.0),
                            ),
                            egui::CornerRadius::same(2),
                            theme::TEXT,
                        );
                    }
                    let tip = format!("{}  ·  {}", t.tr(entry.tip), entry.key);
                    let resp = resp.on_hover_text(tip);
                    if resp.clicked() {
                        state.set_tool(entry.tool);
                    }
                }
            });

            // Zoom stack pinned to the bottom of the rail.
            let zoom_h = zoom::stack_height(true);
            let zoom_rect = Rect::from_min_size(
                Pos2::new(panel_rect.center().x - 19.0, panel_rect.bottom() - theme::STATUS_H - 6.0 - zoom_h),
                Vec2::new(38.0, zoom_h),
            );
            widgets::at_rect(ui, zoom_rect, |ui| {
                zoom::rail_frame().show(ui, |ui| {
                    zoom::stack(ui, state, true);
                });
            });
        });
}

/// Fill helper used by layouts that want the rail tint outside a panel.
#[allow(dead_code)]
pub fn rail_fill() -> egui::Color32 {
    widgets::RAIL
}

/// Where the rail's tool glyphs live inside a rect (used by tests/screenshots).
#[allow(dead_code)]
pub fn tool_icon_positions(rect: Rect) -> Vec<Pos2> {
    let mut out = Vec::new();
    let mut y = rect.top() + 8.0 + 17.0;
    for _ in TOOLS {
        out.push(Pos2::new(rect.center().x, y));
        y += 38.0;
    }
    out
}

/// Compact label used in unit tests.
#[allow(dead_code)]
pub fn tool_tip_prefix(entry: &ToolEntry) -> (Tool, &'static str) {
    (entry.tool, entry.key)
}

#[allow(dead_code)]
fn _font_helper(ui: &Ui) -> FontId {
    let _ = ui;
    FontId::proportional(10.0)
}

#[allow(dead_code)]
fn _layout_helper() -> (Align2, Layout) {
    (Align2::CENTER_CENTER, Layout::top_down(egui::Align::Center))
}

// Left tool rail: tools (V B R O A L T C) with the active indicator, plus the
// vertical zoom stack pinned to the bottom — exactly like the web editor.
// The whole panel slides/fades in and out with the preview <-> edit phase.

use egui::{Color32, Id, Pos2, Rect, Sense, Stroke, Vec2};

use crate::state::{AppState, Phase, Tool};
use crate::theme;

use super::motion;
use super::widgets;
use super::zoom;

struct ToolEntry {
    tool: Tool,
    /// Name of the generated SVG geometry in `ui::tool_icons`.
    icon: &'static str,
    key: &'static str,
    tip: &'static str,
}

const TOOLS: [ToolEntry; 8] = [
    ToolEntry { tool: Tool::Move, icon: "move", key: "V", tip: "移動／平移" },
    ToolEntry { tool: Tool::Brush, icon: "brush", key: "B", tip: "畫筆" },
    ToolEntry { tool: Tool::Rect, icon: "rect", key: "R", tip: "矩形" },
    ToolEntry { tool: Tool::Ellipse, icon: "ellipse", key: "O", tip: "圓框" },
    ToolEntry { tool: Tool::Arrow, icon: "arrow", key: "A", tip: "箭頭（點兩下：起點 → 終點）" },
    ToolEntry { tool: Tool::Line, icon: "line", key: "L", tip: "直線（點兩下：起點 → 終點）" },
    ToolEntry { tool: Tool::Text, icon: "text", key: "T", tip: "文字" },
    ToolEntry { tool: Tool::Crop, icon: "crop", key: "C", tip: "裁切" },
];

/// Draw one of the web build's 24x24 SVG icons at `rect` (round caps, 1.8 stroke).
fn paint_tool_icon(painter: &egui::Painter, rect: Rect, name: &str, color: Color32) {
    let Some(icon) = super::tool_icons::rail(name) else { return };
    let scale = rect.width() / 24.0;
    let stroke = Stroke::new((1.8 * scale).max(1.0), color);
    for path in icon.paths {
        if path.len() < 2 {
            continue;
        }
        let pts: Vec<Pos2> = path
            .iter()
            .map(|(x, y)| Pos2::new(rect.left() + x * scale, rect.top() + y * scale))
            .collect();
        painter.add(egui::Shape::line(pts.clone(), stroke));
        for p in [pts[0], *pts.last().unwrap()] {
            painter.circle_filled(p, stroke.width * 0.5, color);
        }
    }
}

pub fn show(ctx: &egui::Context, state: &mut AppState) {
    // Slide in on preview -> edit, slide out on edit -> preview (web 220–240 ms).
    // The no-document state keeps the editor chrome visible (body.no-doc).
    let target = state.phase == Phase::Edit || state.doc.is_none();
    let t = motion::blend(ctx, Id::new("toolrail-slide"), target, motion::PANEL);
    if !target && t <= 0.001 {
        return;
    }
    let width = (theme::RAIL_W * t).max(0.0);
    egui::SidePanel::left("toolrail")
        .exact_width(width)
        .resizable(false)
        .show_separator_line(false)
        .frame(
            egui::Frame::new()
                .fill(motion::faded(widgets::RAIL, t))
                .inner_margin(egui::Margin::ZERO),
        )
        .show(ctx, |ui| {
            ui.set_opacity(t);
            // Keep the content at its full width while the panel width grows, so
            // it is revealed/clipped instead of reflowing; plus the web's -6px
            // translateX slide.
            let base = ui.max_rect();
            let content = Rect::from_min_size(
                Pos2::new(base.left() - motion::slide_offset(t, 6.0), base.top()),
                Vec2::new(theme::RAIL_W, base.height()),
            );
            widgets::at_rect(ui, content, |ui| draw_rail(ui, state));
        });
}

fn draw_rail(ui: &mut egui::Ui, state: &mut AppState) {
    let t = state.locale;
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
                // separator before the crop tool (web: 24x1, 6px vertical margin)
                ui.add_space(2.0);
                let (sep, _) = ui.allocate_exact_size(Vec2::new(24.0, 1.0), Sense::hover());
                ui.painter().hline(
                    sep.x_range(),
                    sep.center().y,
                    Stroke::new(1.0_f32, widgets::white(22)),
                );
                ui.add_space(2.0);
            }
            let active = state.tool == entry.tool;
            let (rect, resp) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::click());
            let hovered = resp.hovered();
            let painter = ui.painter();
            if active {
                // web `--brand-soft` = 10% #f9fafb
                painter.rect_filled(rect, egui::CornerRadius::same(8), widgets::brand_soft());
            } else if hovered {
                // web `--surface` = 6% white over the panel
                painter.rect_filled(rect, egui::CornerRadius::same(8), widgets::white(15));
            }
            let color = if active || hovered { theme::TEXT } else { theme::DIM };
            paint_tool_icon(
                painter,
                Rect::from_center_size(rect.center(), Vec2::splat(19.0)),
                entry.icon,
                color,
            );
            if active {
                // web `.rail-btn[aria-pressed]::before`: 2px bar at left:-7, 9px insets
                painter.rect_filled(
                    Rect::from_min_size(
                        Pos2::new(rect.left() - 7.0, rect.top() + 9.0),
                        Vec2::new(2.0, rect.height() - 18.0),
                    ),
                    egui::CornerRadius::same(2),
                    theme::TEXT,
                );
            }
            let tip = format!("{}  ·  {}", t.tr(entry.tip), entry.key);
            if resp.on_hover_text(tip).clicked() {
                state.set_tool(entry.tool);
            }
        }
    });

    // In the no-document state the zoom controls float in the canvas instead.
    if state.doc.is_none() {
        return;
    }
    // Zoom stack pinned to the bottom of the rail.
    let zoom_h = zoom::stack_height(true);
    let zoom_rect = Rect::from_min_size(
        Pos2::new(
            panel_rect.center().x - 19.0,
            panel_rect.bottom() - theme::STATUS_H - 6.0 - zoom_h,
        ),
        Vec2::new(38.0, zoom_h),
    );
    widgets::at_rect(ui, zoom_rect, |ui| {
        zoom::rail_frame().show(ui, |ui| {
            zoom::stack(ui, state, true);
        });
    });
}

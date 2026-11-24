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
    let Some(icon) = super::svg_icons::rail(name) else { return };
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
    let active_idx = TOOLS
        .iter()
        .position(|e| e.tool == state.tool)
        .unwrap_or(0);
    // The 2px bar slides between rows (web: 120–180 ms ease-out).
    let bar_idx = ui.ctx().animate_value_with_time(
        Id::new("rail-bar-idx"),
        active_idx as f32,
        motion::scaled_duration(ui.ctx().style().animation_time, 0.18),
    );
    let mut rows: Vec<(f32, f32)> = Vec::with_capacity(TOOLS.len());
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
            rows.push((rect.center().y, rect.left()));
            // Hover fades instead of snapping on/off.
            let hover = motion::blend(
                ui.ctx(),
                Id::new(("rail-hover", i)),
                resp.hovered(),
                0.12,
            );
            // Press scale 0.94 while held, 120 ms spring back on release.
            let press = motion::press_scale(
                ui.ctx(),
                Id::new(("rail-press", i)),
                &resp,
                0.94,
            );
            let scaled = Rect::from_center_size(rect.center(), rect.size() * press);
            // Active pill: crossfades on switch and pops in (0.8 -> 1.12 -> 1).
            let (progress, active_alpha) = motion::toggle(
                ui.ctx(),
                Id::new(("rail-active", i)),
                active,
                0.18,
            );
            let painter = ui.painter();
            if active_alpha > 0.01 {
                let scale = if active {
                    motion::pop_scale(progress)
                } else {
                    1.0
                };
                let pill = Rect::from_center_size(
                    scaled.center(),
                    Vec2::splat(30.0 * scale * press),
                );
                painter.rect_filled(
                    pill,
                    egui::CornerRadius::same(8),
                    widgets::brand_soft().gamma_multiply(active_alpha),
                );
            } else if hover > 0.01 {
                painter.rect_filled(
                    scaled,
                    egui::CornerRadius::same(8),
                    widgets::white((15.0 * hover) as u8),
                );
            }
            let color = if active || hover > 0.5 { theme::TEXT } else { theme::DIM };
            paint_tool_icon(
                painter,
                Rect::from_center_size(scaled.center(), Vec2::splat(19.0) * press),
                entry.icon,
                color,
            );
            let tip = format!("{}  ·  {}", t.tr(entry.tip), entry.key);
            if resp.on_hover_text(tip).clicked() {
                state.set_tool(entry.tool);
            }
        }
    });
    // Left indicator bar, sliding between the active row positions.
    if rows.len() > 1 {
        let clamped = bar_idx.clamp(0.0, (rows.len() - 1) as f32);
        let i0 = clamped.floor() as usize;
        let i1 = (i0 + 1).min(rows.len() - 1);
        let frac = clamped - i0 as f32;
        let y = rows[i0].0 + (rows[i1].0 - rows[i0].0) * frac;
        let x = rows[active_idx.min(rows.len() - 1)].1 - 7.0;
        ui.painter().rect_filled(
            Rect::from_min_size(Pos2::new(x, y - 9.0), Vec2::new(2.0, 18.0)),
            egui::CornerRadius::same(2),
            theme::TEXT,
        );
    }

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

// Shortcut reference window (opens with ? or the top-bar help button). Grouped
// rows match src/index.html; Esc is handled by app.rs.

use egui::{Align, Align2, Color32, CornerRadius, Id, Layout, Order, Pos2, Rect, Sense, Vec2};

use crate::state::{AppState, Phase};
use crate::theme;

use super::widgets::{self, Icon};

struct Group {
    title: &'static str,
    rows: &'static [(&'static str, &'static [&'static str])],
    note: Option<&'static str>,
}

const GROUPS: &[Group] = &[
    Group {
        title: "檔案",
        rows: &[
            ("開啟檔案", &["Ctrl", "O"]),
            ("儲存", &["Ctrl", "S"]),
            ("另存新檔", &["Ctrl", "Shift", "S"]),
            ("上一張／下一張", &["←", "→"]),
        ],
        note: None,
    },
    Group {
        title: "編輯",
        rows: &[
            ("復原", &["Ctrl", "Z"]),
            ("重做", &["Ctrl", "Shift", "Z"]),
            ("刪除選取標註", &["Delete"]),
            ("取消選取／退出裁切", &["Esc"]),
        ],
        note: None,
    },
    Group {
        title: "視圖",
        rows: &[
            ("放大", &["+", "="]),
            ("縮小", &["-"]),
            ("適應視窗", &["0"]),
            ("資訊分頁（編輯中）", &["I"]),
            ("全螢幕", &["F"]),
            ("平移畫布", &["Space"]),
        ],
        note: None,
    },
    Group {
        title: "階段",
        rows: &[
            ("資訊浮卡（預覽）", &["I"]),
            ("回預覽（編輯中，先取消選取／裁切）", &["Esc"]),
        ],
        note: Some("預覽→編輯請按右上角「編輯」按鈕"),
    },
    Group {
        title: "工具",
        rows: &[
            ("移動", &["V"]),
            ("畫筆", &["B"]),
            ("矩形", &["R"]),
            ("圓框", &["O"]),
            ("箭頭", &["A"]),
            ("文字", &["T"]),
            ("裁切", &["C"]),
        ],
        note: None,
    },
];

pub fn show(ctx: &egui::Context, state: &mut AppState) {
    if !state.show_shortcuts {
        return;
    }
    let t = state.locale;
    let screen = ctx.screen_rect();
    let fade = ctx.animate_bool_with_time(Id::new("shortcuts-fade"), true, 0.14);

    // Scrim: clicking it closes the window.
    let scrim = egui::Area::new(Id::new("shortcuts-scrim"))
        .order(Order::Middle)
        .fixed_pos(screen.left_top())
        .show(ctx, |ui| {
            let (rect, resp) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter().rect_filled(
                rect,
                0.0,
                Color32::from_rgba_unmultiplied(10, 10, 12, (170.0 * fade) as u8),
            );
            resp
        });
    if scrim.inner.clicked() {
        state.show_shortcuts = false;
        return;
    }

    let width = 600.0;
    let height = (screen.height() - 56.0).min(620.0);
    let pos = Pos2::new(screen.center().x - width / 2.0, screen.center().y - height / 2.0);
    egui::Area::new(Id::new("shortcuts-card"))
        .order(Order::Foreground)
        .fixed_pos(pos)
        .show(ctx, |ui| {
            ui.set_opacity(fade);
            egui::Frame::new()
                .fill(theme::PANEL)
                .stroke(egui::Stroke::new(1.0_f32, widgets::white(60)))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(egui::Margin::ZERO)
                .show(ui, |ui| {
                    ui.set_width(width);
                    ui.set_height(height);
                    // Head
                    let (head, _) = ui.allocate_exact_size(Vec2::new(width, 58.0), Sense::hover());
                    let painter = ui.painter();
                    painter.text(
                        Pos2::new(head.left() + 16.0, head.top() + 18.0),
                        Align2::LEFT_CENTER,
                        t.tr("快捷鍵"),
                        egui::FontId::proportional(13.5),
                        theme::TEXT,
                    );
                    painter.text(
                        Pos2::new(head.left() + 16.0, head.top() + 38.0),
                        Align2::LEFT_CENTER,
                        t.tr("按 ? 開啟 · Esc 關閉"),
                        egui::FontId::proportional(10.5),
                        widgets::white(120),
                    );
                    painter.hline(
                        head.x_range(),
                        head.bottom() - 0.5,
                        egui::Stroke::new(1.0_f32, widgets::white(24)),
                    );
                    let close_rect = Rect::from_min_size(
                        Pos2::new(head.right() - 16.0 - 36.0, head.center().y - 18.0),
                        Vec2::splat(36.0),
                    );
                    let close = ui.interact(
                        close_rect,
                        ui.id().with("shortcuts-close"),
                        Sense::click(),
                    );
                    let circle = if close.hovered() { widgets::white(50) } else { widgets::white(20) };
                    ui.painter().circle_filled(close_rect.center(), 18.0, circle);
                    ui.painter().circle_stroke(
                        close_rect.center(),
                        18.0,
                        egui::Stroke::new(1.0_f32, widgets::white(120)),
                    );
                    if close.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    widgets::paint_icon(
                        ui.painter(),
                        Rect::from_center_size(close_rect.center(), Vec2::splat(15.0)),
                        Icon::Close,
                        if close.hovered() { theme::TEXT } else { theme::DIM },
                        2.0,
                    );
                    if close.clicked() {
                        state.show_shortcuts = false;
                    }

                    // Body: two columns of groups.
                    let body_rect = Rect::from_min_max(
                        Pos2::new(head.left() + 16.0, head.bottom() + 4.0),
                        Pos2::new(head.right() - 16.0, pos.y + height - 10.0),
                    );
                    let mut body_ui = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(body_rect)
                            .layout(Layout::top_down(Align::Min)),
                    );
                    body_ui.set_clip_rect(ui.clip_rect().intersect(body_rect));
                    egui::ScrollArea::vertical()
                        .id_salt("shortcuts-scroll")
                        .auto_shrink([false, false])
                        .show(&mut body_ui, |ui| {
                            // Row-major two-column grid, matching the web modal:
                            // left 檔案/視圖/工具, right 編輯/階段.
                            let gap = 24.0;
                            let col_w = (ui.available_width() - gap) / 2.0;
                            ui.horizontal_top(|ui| {
                                ui.spacing_mut().item_spacing.x = gap;
                                ui.allocate_ui_with_layout(
                                    Vec2::new(col_w, 0.0),
                                    Layout::top_down(Align::Min),
                                    |ui| {
                                        ui.set_width(col_w);
                                        for i in [0usize, 2, 4] {
                                            group_ui(ui, &GROUPS[i], t);
                                        }
                                    },
                                );
                                ui.allocate_ui_with_layout(
                                    Vec2::new(col_w, 0.0),
                                    Layout::top_down(Align::Min),
                                    |ui| {
                                        ui.set_width(col_w);
                                        for i in [1usize, 3] {
                                            group_ui(ui, &GROUPS[i], t);
                                        }
                                    },
                                );
                            });
                        });
                });
        });
}

fn group_ui(ui: &mut egui::Ui, group: &Group, locale: crate::i18n::Locale) {
    ui.add_space(4.0);
    let (title, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), 20.0),
        Sense::hover(),
    );
    let painter = ui.painter();
    painter.text(
        Pos2::new(title.left(), title.center().y),
        Align2::LEFT_CENTER,
        locale.tr(group.title),
        egui::FontId::proportional(10.0),
        widgets::white(115),
    );
    painter.hline(
        title.x_range(),
        title.bottom() - 0.5,
        egui::Stroke::new(1.0_f32, widgets::white(26)),
    );
    for (label, keys) in group.rows {
        let (row, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::hover());
        let painter = ui.painter();
        painter.text(
            Pos2::new(row.left(), row.center().y),
            Align2::LEFT_CENTER,
            locale.tr(label),
            egui::FontId::proportional(11.5),
            theme::DIM,
        );
        // Keys are laid out right to left.
        let mut x = row.right();
        for key in keys.iter().rev() {
            let galley = painter.layout_no_wrap(
                key.to_string(),
                egui::FontId::monospace(9.5),
                theme::TEXT,
            );
            let w = (galley.size().x + 12.0).max(21.0);
            let chip = Rect::from_min_size(
                Pos2::new(x - w, row.center().y - 10.5),
                Vec2::new(w, 21.0),
            );
            painter.rect_filled(chip, CornerRadius::same(5), widgets::SURFACE_2);
            painter.rect_stroke(
                chip,
                CornerRadius::same(5),
                egui::Stroke::new(1.0_f32, widgets::white(46)),
                egui::StrokeKind::Inside,
            );
            painter.galley(
                Pos2::new(
                    chip.center().x - galley.size().x / 2.0,
                    chip.center().y - galley.size().y / 2.0,
                ),
                galley,
                theme::TEXT,
            );
            x -= w + 3.0;
        }
    }
    if let Some(note) = group.note {
        let (rect, _) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), 22.0),
            Sense::hover(),
        );
        ui.painter().text(
            Pos2::new(rect.left(), rect.center().y),
            Align2::LEFT_CENTER,
            locale.tr(note),
            egui::FontId::proportional(10.0),
            widgets::white(110),
        );
    }
}

/// Groups exposed for tests (titles + row counts).
#[allow(dead_code)]
pub fn group_titles() -> Vec<&'static str> {
    GROUPS.iter().map(|g| g.title).collect()
}

/// Editor-only window (the router keeps it out of preview chrome).
#[allow(dead_code)]
pub fn phase_allows(state: &AppState) -> bool {
    matches!(state.phase, Phase::Preview | Phase::Edit)
}


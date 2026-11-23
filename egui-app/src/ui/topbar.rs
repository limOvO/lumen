// Editor top bar: file name + save state on the left, actions on the right.
// In preview mode this module delegates to the floating preview chrome.

use egui::{Align, FontId, Layout, Pos2, Rect, Sense, Stroke, Ui, Vec2};

use crate::state::{AppState, Phase, UiAction};
use crate::theme;

use super::local;
use super::widgets::{self, Btn, Icon};

pub fn show(ctx: &egui::Context, state: &mut AppState) {
    match state.phase {
        Phase::Preview => super::preview::chrome(ctx, state),
        Phase::Edit => editor_bar(ctx, state),
    }
}

fn editor_bar(ctx: &egui::Context, state: &mut AppState) {
    let t = state.locale;
    let collapsed = local::get(ctx, |l| l.inspector_collapsed);
    egui::TopBottomPanel::top("topbar")
        .exact_height(theme::TOPBAR_H)
        .show_separator_line(false)
        .frame(
            egui::Frame::new()
                .fill(theme::PANEL)
                .inner_margin(egui::Margin::symmetric(12, 0)),
        )
        .show(ctx, |ui| {
            let rect = ui.max_rect();
            ui.painter().hline(
                rect.x_range(),
                rect.bottom() - 0.5,
                Stroke::new(1.0_f32, widgets::white(30)),
            );
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);

                // Brand mark -------------------------------------------------
                let (brand, _) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::hover());
                ui.painter().rect_filled(brand, egui::CornerRadius::same(8), widgets::SURFACE);
                widgets::paint_icon(
                    ui.painter(),
                    Rect::from_center_size(brand.center(), Vec2::splat(17.0)),
                    Icon::Brand,
                    theme::TEXT,
                    1.8,
                );
                ui.add_space(2.0);

                // File name + dirty dot --------------------------------------
                let name = state
                    .doc
                    .as_ref()
                    .map(|d| d.name.clone())
                    .unwrap_or_else(|| t.tr("未開啟圖片"));
                ui.label(
                    egui::RichText::new(name)
                        .size(13.0)
                        .strong()
                        .color(theme::TEXT),
                );
                if state.doc.is_some() && state.dirty {
                    let (dot, resp) = ui.allocate_exact_size(Vec2::splat(6.0), Sense::hover());
                    ui.painter().circle_filled(dot.center(), 3.0, theme::WARN);
                    resp.on_hover_text(t.tr("有未儲存的變更"));
                }

                // Save state -------------------------------------------------
                ui.add_space(4.0);
                let (save_text, save_color) = if state.doc.is_none() {
                    (t.tr("無變更"), widgets::white(120))
                } else if state.dirty {
                    (t.tr("有未儲存的變更"), theme::WARN)
                } else {
                    (t.tr("無變更"), theme::DIM)
                };
                let dot_color = if state.doc.is_some() && state.dirty {
                    theme::WARN
                } else {
                    widgets::white(90)
                };
                let (dot, _) = ui.allocate_exact_size(Vec2::splat(6.0), Sense::hover());
                ui.painter().circle_filled(dot.center(), 2.5, dot_color);
                ui.label(egui::RichText::new(save_text).size(10.5).color(save_color));

                // Right-hand actions (laid out right to left). ----------------
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
                    let tip_help = t.tr("快捷鍵說明（?）");
                    if widgets::icon_button(ui, Icon::Help, &tip_help, true).clicked() {
                        state.show_shortcuts = !state.show_shortcuts;
                    }
                    let tip_collapse =
                        if collapsed { t.tr("展開屬性面板") } else { t.tr("收合屬性面板") };
                    if widgets::icon_button(ui, Icon::Panel, &tip_collapse, true).clicked() {
                        local::with(ui.ctx(), |l| l.inspector_collapsed = !l.inspector_collapsed);
                    }
                    widgets::sep_v(ui);

                    let redo = widgets::icon_button(ui, Icon::Redo, &t.tr("重做（Ctrl+Shift+Z）"), true);
                    if redo.clicked() {
                        state.redo();
                    }
                    let undo = widgets::icon_button(ui, Icon::Undo, &t.tr("復原（Ctrl+Z）"), true);
                    if undo.clicked() {
                        state.undo();
                    }
                    widgets::sep_v(ui);

                    let has_doc = state.doc.is_some();
                    let saveas = widgets::bar_button(
                        ui,
                        &t.tr("另存"),
                        Btn { icon: Some(Icon::SaveAs), ..Default::default() },
                        has_doc,
                    );
                    if saveas.clicked() {
                        state.push_action(UiAction::SaveAs);
                    }
                    let save = widgets::bar_button(
                        ui,
                        &t.tr("儲存"),
                        Btn { icon: Some(Icon::Save), primary: true, ..Default::default() },
                        has_doc,
                    );
                    if save.clicked() {
                        state.push_action(UiAction::Save);
                    }
                    let open = widgets::bar_button(
                        ui,
                        &t.tr("開啟"),
                        Btn { icon: Some(Icon::Folder), ..Default::default() },
                        true,
                    );
                    if open.clicked() {
                        state.push_action(UiAction::Open);
                    }
                    widgets::sep_v(ui);

                    let compare = widgets::bar_button(
                        ui,
                        &t.tr("比較"),
                        Btn {
                            icon: Some(Icon::Eye),
                            active: state.compare,
                            accent: state.compare,
                            ..Default::default()
                        },
                        has_doc,
                    );
                    if compare.clicked() {
                        state.compare = !state.compare;
                    }
                    let preview = widgets::bar_button(
                        ui,
                        &t.tr("預覽"),
                        Btn { icon: Some(Icon::Eye), ..Default::default() },
                        has_doc,
                    );
                    if preview.clicked() {
                        state.phase = Phase::Preview;
                        state.compare = false;
                    }
                });
            });
        });
}

/// A caption pill drawn with an explicit height (chips).
pub(crate) fn caption_pill(ui: &mut Ui, text: &str) -> Rect {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), FontId::proportional(11.5), theme::TEXT);
    let size = Vec2::new(galley.size().x + 24.0, 30.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, egui::CornerRadius::same(15), widgets::CHIP_BG);
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(15),
        Stroke::new(1.0_f32, widgets::white(40)),
        egui::StrokeKind::Inside,
    );
    painter.galley(
        Pos2::new(rect.center().x - galley.size().x / 2.0, rect.center().y - galley.size().y / 2.0),
        galley,
        theme::TEXT,
    );
    rect
}

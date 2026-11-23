// Right-side inspector for the editor: 資訊 / 調整 / 裁切 tabs.
// The info page is shared with the preview peek card (info.rs).

use egui::{Align, FontId, Id, Pos2, Rect, Sense, Stroke, Ui, Vec2};

use crate::i18n::Locale;
use crate::state::{AppState, OutputFormat, Phase, StatusKind, Tool, UiAction};
use crate::theme;

use super::format::{self, RatioPreset};
use super::local::{self, PanelTab};
use super::motion;
use super::widgets::{self, Icon};

pub fn show(ctx: &egui::Context, state: &mut AppState) {
    // Same slide/fade as the rail: width 0 -> 300 with a small x-offset.
    // Collapsing the inspector (top-bar button) animates through the same path.
    let collapsed = local::get(ctx, |l| l.inspector_collapsed);
    let target = state.phase == Phase::Edit && !collapsed;
    let t = motion::blend(ctx, Id::new("inspector-slide"), target, motion::PANEL);
    if !target && t <= 0.001 {
        return;
    }
    // Tabs follow the tool: picking the crop tool activates 裁切. Only update
    // while the panel is actually being shown so the closing frame keeps its tab.
    let tab = if target {
        local::with(ctx, |l| {
            if l.last_tool != Some(state.tool) {
                l.last_tool = Some(state.tool);
                if state.tool == Tool::Crop {
                    l.tab = PanelTab::Crop;
                }
            }
            l.tab
        })
    } else {
        local::get(ctx, |l| l.tab)
    };
    let width = (theme::PANEL_W * t).max(0.0);

    egui::SidePanel::right("inspector")
        .exact_width(width)
        .resizable(false)
        .show_separator_line(false)
        .frame(
            egui::Frame::new()
                .fill(motion::faded(theme::PANEL, t))
                .inner_margin(egui::Margin::ZERO)
                .stroke(Stroke::new(1.0_f32, motion::faded(theme::LINE, t))),
        )
        .show(ctx, |ui| {
            ui.set_opacity(t);
            // Full-width content anchored to the right edge, revealed by the
            // growing panel (no reflow), plus the web translateX(6px) -> 0.
            let base = ui.max_rect();
            let content = Rect::from_min_size(
                Pos2::new(
                    base.right() - theme::PANEL_W + motion::slide_offset(t, 6.0),
                    base.top(),
                ),
                Vec2::new(theme::PANEL_W, base.height()),
            );
            widgets::at_rect(ui, content, |ui| panel_content(ui, state, tab));
        });
}

fn panel_content(ui: &mut Ui, state: &mut AppState, tab: PanelTab) {
    // Left hairline: the panel is flush against the canvas.
    let rect = ui.max_rect();
    ui.painter().vline(
        rect.left(),
        rect.y_range(),
        Stroke::new(1.0_f32, widgets::white(26)),
    );
    let active = tabs(ui, tab, state.locale);
    if active != tab {
        local::with(ui.ctx(), |l| l.tab = active);
    }
    ui.painter().hline(
        ui.max_rect().x_range(),
        ui.max_rect().top(),
        Stroke::new(1.0_f32, widgets::white(26)),
    );
    egui::ScrollArea::vertical()
        .id_salt("inspector-scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            page(ui, active, state);
        });
}

/// Tab strip: three equal labels with a sliding 2px underline.
fn tabs(ui: &mut Ui, current: PanelTab, locale: Locale) -> PanelTab {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 38.0), Sense::hover());
    let mut next = current;
    let col_w = (rect.width() - 20.0) / 3.0;
    for (i, tab) in PanelTab::ALL.into_iter().enumerate() {
        let r = Rect::from_min_size(
            Pos2::new(rect.left() + 10.0 + i as f32 * col_w, rect.top() + 4.0),
            Vec2::new(col_w, 30.0),
        );
        let resp = ui.interact(r, ui.id().with(("insp-tab", i)), Sense::click());
        let color = if resp.hovered() || tab == current {
            theme::TEXT
        } else {
            theme::DIM
        };
        ui.painter().text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            locale.tr(tab.label()),
            FontId::proportional(12.0),
            color,
        );
        if tab == current {
            // Weight 600 approximation.
            ui.painter().text(
                r.center() + Vec2::new(0.35, 0.0),
                egui::Align2::CENTER_CENTER,
                locale.tr(tab.label()),
                FontId::proportional(12.0),
                color,
            );
        }
        if resp.clicked() {
            next = tab;
        }
    }
    // Sliding underline.
    let ctx = ui.ctx().clone();
    let anim = ctx.animate_value_with_time(
        ui.id().with("insp-underline"),
        next.index() as f32,
        0.18,
    );
    let x = rect.left() + 10.0 + anim * (col_w + 2.0);
    ui.painter().rect_filled(
        Rect::from_min_size(
            Pos2::new(x, rect.bottom() - 2.0),
            Vec2::new(col_w, 2.0),
        ),
        egui::CornerRadius::same(1),
        theme::TEXT,
    );
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0_f32, widgets::white(26)),
    );
    next
}

// ---------------------------------------------------------------------------
// 資訊 page (also used by the preview peek card)
// ---------------------------------------------------------------------------

pub fn info_page(ui: &mut Ui, state: &mut AppState) {
    let t = state.locale;
    let Some(doc) = state.doc.as_ref() else {
        ui.add_space(16.0);
        ui.vertical_centered(|ui| {
            widgets::hint(ui, t.tr("尚未載入圖片"));
        });
        return;
    };
    let (name, dims, format_name, bytes, mtime) =
        (doc.name.clone(), format!("{} × {} px", doc.width, doc.height), doc.format.clone(), doc.bytes, doc.mtime.clone());
    let (w, h) = (doc.width, doc.height);
    let path = doc.path.clone();
    let exif = doc.exif.clone();

    widgets::kv_row(ui, &t.tr("檔名"), &name, false);
    widgets::kv_row(ui, &t.tr("尺寸"), &dims, true);
    widgets::kv_row(ui, &t.tr("格式"), &format_name, false);
    widgets::kv_row(ui, &t.tr("檔案大小"), &format::fmt_bytes(bytes), true);
    widgets::kv_row(ui, &t.tr("修改時間"), &mtime, true);
    widgets::kv_row(ui, &t.tr("長寬比"), &format::ratio_label(w, h), true);

    ui.add_space(12.0);
    let count = match t {
        Locale::En => format!("{} items", exif.len()),
        Locale::ZhHans => format!("{} 项", exif.len()),
        Locale::ZhHant => format!("{} 項", exif.len()),
    };
    widgets::section_head(ui, &t.tr("EXIF"), Some(&count));
    if exif.is_empty() {
        ui.add_space(6.0);
        widgets::hint(ui, t.tr("儲存後的檔案已移除 EXIF／GPS（隱私保護）"));
    } else {
        for (k, v) in &exif {
            let (rect, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::hover());
            let painter = ui.painter();
            painter.text(
                Pos2::new(rect.left(), rect.center().y),
                egui::Align2::LEFT_CENTER,
                k,
                FontId::proportional(11.0),
                theme::DIM,
            );
            painter.text(
                Pos2::new(rect.right(), rect.center().y),
                egui::Align2::RIGHT_CENTER,
                v,
                FontId::proportional(11.5),
                theme::TEXT,
            );
        }
    }

    ui.add_space(12.0);
    if widgets::ghost_block_icon(ui, &t.tr("複製檔案路徑"), Icon::Copy, true).clicked() {
        ui.ctx().copy_text(path.display().to_string());
        let now = ui.ctx().input(|i| i.time);
        state.status(t.tr("已複製檔案路徑"), StatusKind::Ok, now);
    }
}

// ---------------------------------------------------------------------------
// 調整 page
// ---------------------------------------------------------------------------

pub fn adjust_page(ui: &mut Ui, state: &mut AppState) {
    let t = state.locale;
    let Some(doc) = state.doc.as_ref() else {
        ui.add_space(12.0);
        widgets::hint(ui, t.tr("尚未載入圖片"));
        return;
    };
    let (dw, dh) = (doc.width, doc.height);
    // Auto resolves against the document; quality only goes away for PNG.
    let resolved = state.output.format.resolve(doc);
    let target = crate::export::target(
        dw,
        dh,
        state.crop,
        state.output.max_dim,
        state.output.scale,
    );
    let (ow, oh) = (target.out_w, target.out_h);
    // Base = the clamped crop (before scale / max_dim), like `baseDims()` in the web build.
    let base = (
        (target.w.round().max(1.0)) as u32,
        (target.h.round().max(1.0)) as u32,
    );
    let base_long = base.0.max(base.1);

    // 尺寸（像素）: editable W/H + ratio lock + percentage chips ------------
    widgets::field_label(ui, t.tr("尺寸（像素）"));
    ui.add_space(5.0);
    let lock_on = local::get(ui.ctx(), |l| l.lock_ratio);
    let mut edited = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        // 2 fields + "×" + lock + 3 gaps must fit the 276px panel content width.
        let field_w = ((ui.available_width() - 30.0 - 10.0 - 18.0) / 2.0).max(64.0);
        if let Some(v) = widgets::number_field(ui, "out-w", Some(ow), "", true, field_w) {
            edited = Some((v, true));
        }
        widgets::text(ui, "×", 10.5, widgets::white(120));
        if let Some(v) = widgets::number_field(ui, "out-h", Some(oh), "", true, field_w) {
            edited = Some((v, false));
        }
        let tip = if lock_on { t.tr("已鎖定長寬比") } else { t.tr("已解除長寬比鎖定") };
        if widgets::icon_button_active(ui, Icon::Lock, lock_on, &tip).clicked() {
            local::with(ui.ctx(), |l| l.lock_ratio = !l.lock_ratio);
            // Locked keeps a proportional long-edge cap; unlocked clears it back
            // to the native size (the web build's free-form mode).
            if lock_on {
                state.output.max_dim = None;
            } else {
                state.output.max_dim = Some(ow.max(oh).max(1));
            }
            state.output.scale = None;
        }
    });
    if let Some((value, width_axis)) = edited {
        let (max_dim, scale) = format::dimension_to_output(base, value, width_axis);
        state.output.max_dim = max_dim;
        state.output.scale = scale;
    }

    ui.add_space(6.0);
    let pct = ((ow.max(oh) as f32 / base_long.max(1) as f32) * 100.0).round() as u32;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(5.0, 5.0);
        for p in [25u32, 50, 100, 150, 200] {
            let active = pct == p;
            if widgets::chip(ui, &format!("{p}%"), active, true).clicked() {
                state.output.max_dim = None;
                state.output.scale =
                    if p == 100 { None } else { Some(p as f32 / 100.0) };
            }
        }
    });
    ui.add_space(4.0);
    widgets::hint(ui, format!("→ {} × {} px", ow, oh));

    // 長邊上限 --------------------------------------------------------------
    ui.add_space(14.0);
    ui.horizontal(|ui| {
        widgets::field_label(ui, t.tr("長邊上限"));
        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            if widgets::link_button(ui, &t.tr("清除"), state.output.max_dim.is_some()).clicked() {
                state.output.max_dim = None;
            }
        });
    });
    ui.add_space(5.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let w = ui.available_width() - 48.0;
        if let Some(v) =
            widgets::number_field(ui, "max-dim", state.output.max_dim, &t.tr("不限制"), true, w)
        {
            state.output.max_dim = Some(v);
            state.output.scale = None;
        }
        widgets::text(ui, "px", 10.5, widgets::white(110));
    });

    // 輸出格式 --------------------------------------------------------------
    ui.add_space(14.0);
    widgets::field_label(ui, t.tr("輸出格式"));
    ui.add_space(5.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        for f in [
            OutputFormat::Auto,
            OutputFormat::Jpeg,
            OutputFormat::Png,
            OutputFormat::Webp,
        ] {
            if widgets::chip(ui, &t.tr(f.label()), state.output.format == f, true).clicked() {
                state.output.format = f;
            }
        }
    });

    // 品質 ------------------------------------------------------------------
    ui.add_space(14.0);
    let quality_disabled = resolved == OutputFormat::Png;
    ui.horizontal(|ui| {
        widgets::field_label(ui, t.tr("品質"));
        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            widgets::text(
                ui,
                format!("{}", state.output.quality),
                10.5,
                if quality_disabled { theme::DIM } else { theme::TEXT },
            );
        });
    });
    ui.add_space(6.0);
    let mut q = state.output.quality as f32;
    if widgets::slider(ui, &mut q, 1.0, 100.0, ui.available_width(), !quality_disabled).changed() {
        state.output.quality = q.round().clamp(1.0, 100.0) as u8;
    }
    if quality_disabled {
        ui.add_space(4.0);
        widgets::hint(ui, t.tr("PNG 為無損格式，不使用品質設定。"));
    }

    // 目標大小 --------------------------------------------------------------
    ui.add_space(14.0);
    widgets::field_label(ui, t.tr("目標大小"));
    ui.add_space(5.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let w = ui.available_width() - 48.0;
        if let Some(v) =
            widgets::number_field(ui, "target-kb", state.output.target_kb, &t.tr("自動"), true, w)
        {
            state.output.target_kb = Some(v);
        }
        widgets::text(ui, "KB", 10.5, widgets::white(110));
    });
    if state.output.target_kb.is_some() {
        ui.add_space(4.0);
        widgets::hint(ui, t.tr("已啟用目標大小，將自動調整品質。"));
    }

    // 預估大小 --------------------------------------------------------------
    ui.add_space(14.0);
    widgets::field_label(ui, t.tr("預估大小"));
    ui.add_space(5.0);
    let estimate_text = match state.estimate {
        Some(est) => format::format_estimate(Some(est.bytes), est.width, est.height),
        None => "—".to_string(),
    };
    ui.horizontal(|ui| {
        if widgets::ghost_block_sized(ui, &t.tr("估算大小"), 96.0, true).clicked() {
            state.push_action(UiAction::Estimate);
        }
        ui.add_space(10.0);
        widgets::text(ui, &estimate_text, 10.5, theme::DIM);
    });

    // 匯出 ------------------------------------------------------------------
    ui.add_space(16.0);
    if widgets::primary_block(ui, &t.tr("匯出…"), true).clicked() {
        state.push_action(UiAction::Export);
    }
    ui.add_space(6.0);
    widgets::hint(ui, t.tr("匯出會保留裁切、縮放與所有標註。"));
}

// ---------------------------------------------------------------------------
// 裁切 page
// ---------------------------------------------------------------------------

pub fn crop_page(ui: &mut Ui, state: &mut AppState) {
    let t = state.locale;
    let Some((dw, dh)) = state.doc_size() else {
        ui.add_space(12.0);
        widgets::hint(ui, t.tr("尚未載入圖片"));
        return;
    };
    let crop = state.crop;
    let active = RatioPreset::from_ratio(crop.and_then(|c| c.ratio), dw, dh);

    widgets::field_label(ui, t.tr("長寬比"));
    ui.add_space(5.0);
    let mut picked: Option<RatioPreset> = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(5.0, 5.0);
        for preset in RatioPreset::ALL {
            let label = preset.label_localized(t);
            if widgets::chip(ui, &label, preset == active, true).clicked() {
                picked = Some(preset);
            }
        }
    });

    ui.add_space(14.0);
    let value = format::crop_readout(crop, dw, dh);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::hover());
    let painter = ui.painter();
    painter.text(
        Pos2::new(rect.left(), rect.center().y),
        egui::Align2::LEFT_CENTER,
        format!("{}:", t.tr("目前裁切")),
        FontId::proportional(11.0),
        theme::DIM,
    );
    painter.text(
        Pos2::new(rect.right(), rect.center().y),
        egui::Align2::RIGHT_CENTER,
        value,
        FontId::monospace(11.5),
        theme::TEXT,
    );
    painter.hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0_f32, widgets::white(18)));

    ui.add_space(8.0);
    widgets::hint(
        ui,
        t.tr("在畫布上拖曳建立裁切框，拖動把手調整；按住 Shift 鎖定比例。"),
    );

    ui.add_space(14.0);
    if widgets::primary_block(ui, &t.tr("套用裁切"), true).clicked() {
        match state.crop {
            None => {
                let now = ui.ctx().input(|i| i.time);
                state.status(t.tr("請先在畫布上拖曳出裁切框"), StatusKind::Warn, now);
            }
            Some(c) => {
                let x = c.x.clamp(0.0, (dw as f32 - 1.0).max(0.0));
                let y = c.y.clamp(0.0, (dh as f32 - 1.0).max(0.0));
                let w = c.w.clamp(1.0, dw as f32 - x);
                let h = c.h.clamp(1.0, dh as f32 - y);
                state.crop = Some(crate::crop::CropRect { x, y, w, h, ratio: c.ratio });
                state.push_action(UiAction::ApplyCrop);
                local::with(ui.ctx(), |l| l.tab = PanelTab::Adjust);
                let now = ui.ctx().input(|i| i.time);
                let msg = t
                    .tr("裁切已套用：${w} × ${h} px")
                    .replace("${w}", &format!("{w:.0}"))
                    .replace("${h}", &format!("{h:.0}"));
                state.status(msg, StatusKind::Ok, now);
            }
        }
    }
    ui.add_space(6.0);
    if widgets::ghost_block(ui, &t.tr("重設裁切"), true).clicked() {
        state.push_action(UiAction::ResetCrop);
        let now = ui.ctx().input(|i| i.time);
        state.status(t.tr("已重設裁切"), StatusKind::Info, now);
    }

    if let Some(preset) = picked {
        if state.doc.is_some() {
            state.set_tool(Tool::Crop);
            state.commit();
            let ratio = preset.value(dw, dh);
            state.crop = match (state.crop, ratio) {
                (None, r) => Some(crate::crop::CropRect::centered(dw, dh, r)),
                (Some(c), None) => Some(crate::crop::CropRect { ratio: None, ..c }),
                (Some(c), Some(r)) => Some(c.with_ratio(dw, dh, Some(r))),
            };
        }
    }
}

pub fn page(ui: &mut Ui, tab: PanelTab, state: &mut AppState) {
    ui.add_space(2.0);
    let margin = egui::Margin::symmetric(12, 12);
    egui::Frame::new().inner_margin(margin).show(ui, |ui| {
        ui.set_width(ui.available_width());
        match tab {
            PanelTab::Info => info_page(ui, state),
            PanelTab::Adjust => adjust_page(ui, state),
            PanelTab::Crop => crop_page(ui, state),
        }
        // Keep the last row clear of the status-bar overlay.
        ui.add_space(theme::STATUS_H + 4.0);
    });
}

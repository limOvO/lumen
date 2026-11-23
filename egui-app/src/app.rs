// Lumen (egui edition): preview surface + editor shell.
// This round covers the viewer end-to-end (open, fit/zoom/pan, folder nav,
// info panel, status bar, i18n); the annotation toolset comes next.

use std::path::{Path, PathBuf};

use egui::{Color32, Key, Rect, Vec2};

use crate::doc::{siblings, Doc, LoadOptions};
use crate::i18n::Locale;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Preview,
    Edit,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Move,
    Brush,
    Line,
    Rect,
    Ellipse,
    Arrow,
    Text,
    Crop,
}

impl Tool {
    fn label(self) -> &'static str {
        match self {
            Tool::Move => "移動",
            Tool::Brush => "畫筆",
            Tool::Line => "直線",
            Tool::Rect => "矩形",
            Tool::Ellipse => "圓框",
            Tool::Arrow => "箭頭",
            Tool::Text => "文字",
            Tool::Crop => "裁切",
        }
    }
}

pub struct View {
    pub scale: f32,
    pub offset: Vec2,
    pub fit: bool,
}

impl Default for View {
    fn default() -> Self {
        Self { scale: 1.0, offset: Vec2::ZERO, fit: true }
    }
}

pub struct LumenApp {
    pub locale: Locale,
    pub doc: Option<Doc>,
    pub error: Option<String>,
    pub view: View,
    pub phase: Phase,
    pub tool: Tool,
    pub show_info: bool,
    pub siblings: Vec<PathBuf>,
    pub index: usize,
    pub fullscreen: bool,
}

const BG: Color32 = Color32::from_rgb(16, 16, 18);
const PANEL: Color32 = Color32::from_rgb(35, 35, 36);
const TEXT: Color32 = Color32::from_rgb(249, 250, 251);
const DIM: Color32 = Color32::from_rgb(156, 163, 175);
const LINE: Color32 = Color32::from_rgba_premultiplied(255, 255, 255, 25);

impl LumenApp {
    pub fn new(initial: Option<PathBuf>) -> Self {
        let locale = Locale::detect();
        let mut app = Self {
            locale,
            doc: None,
            error: None,
            view: View::default(),
            phase: Phase::Preview,
            tool: Tool::Move,
            show_info: false,
            siblings: Vec::new(),
            index: 0,
            fullscreen: false,
        };
        if let Some(path) = initial {
            app.open(&path, None);
        }
        app
    }

    pub fn open(&mut self, path: &Path, ctx: Option<&egui::Context>) {
        let cap = ui_max_pixels(ctx);
        match Doc::load(path, LoadOptions { max_pixels: cap }) {
            Ok(doc) => {
                let name = doc.name.clone();
                self.siblings = siblings(path);
                self.index = self.siblings.iter().position(|p| p == path).unwrap_or(0);
                self.doc = Some(doc);
                self.error = None;
                self.view = View::default();
                if let Some(ctx) = ctx {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
                        "{name} — Lumen"
                    )));
                }
            }
            Err(err) => {
                self.error = Some(self.locale.tr(&err));
            }
        }
    }

    fn step(&mut self, delta: i64, ctx: &egui::Context) {
        if self.siblings.is_empty() {
            return;
        }
        let n = self.siblings.len() as i64;
        let next = ((self.index as i64 + delta + n) % n) as usize;
        let path = self.siblings[next].clone();
        self.open(&path, Some(ctx));
    }

    fn fit_scale(&self, avail: Vec2, img: Vec2) -> f32 {
        (avail.x / img.x).min(avail.y / img.y).min(1.0)
    }

    fn zoom_at(&mut self, factor: f32, anchor: egui::Pos2, center: egui::Pos2) {
        let old = self.view.scale;
        let next = (old * factor).clamp(0.02, 16.0);
        let rel = anchor - center;
        self.view.offset = (self.view.offset - rel) * (next / old) + rel;
        self.view.scale = next;
        self.view.fit = false;
    }

    fn apply_theme(ctx: &egui::Context) {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = PANEL;
        visuals.window_fill = PANEL;
        visuals.extreme_bg_color = BG;
        visuals.override_text_color = Some(TEXT);
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, LINE);
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(40, 40, 42);
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(52, 52, 55);
        visuals.widgets.active.bg_fill = Color32::from_rgb(64, 64, 68);
        ctx.set_visuals(visuals);
    }

    fn top_bar(&mut self, ctx: &egui::Context) {
        let t = self.locale;
        egui::TopBottomPanel::top("top")
            .exact_height(46.0)
            .frame(
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::symmetric(12, 6))
                    .stroke(egui::Stroke::new(1.0, LINE)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    let name = self.doc.as_ref().map(|d| d.name.clone()).unwrap_or_default();
                    ui.label(egui::RichText::new(name).color(TEXT).strong());
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(t.tr("無變更")).color(DIM).size(11.5));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(t.tr("關閉")).clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if ui.button(t.tr("全螢幕")).clicked() {
                            self.fullscreen = !self.fullscreen;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
                        }
                        let info_label = if self.show_info { t.tr("隱藏資訊") } else { t.tr("資訊") };
                        if ui.button(info_label).clicked() {
                            self.show_info = !self.show_info;
                        }
                        let phase_label = match self.phase {
                            Phase::Preview => t.tr("進入編輯"),
                            Phase::Edit => t.tr("回到預覽"),
                        };
                        if ui.button(phase_label).clicked() {
                            self.phase = match self.phase {
                                Phase::Preview => Phase::Edit,
                                Phase::Edit => Phase::Preview,
                            };
                        }
                    });
                });
            });
    }

    fn info_panel(&mut self, ctx: &egui::Context) {
        let t = self.locale;
        egui::SidePanel::right("info")
            .exact_width(300.0)
            .resizable(true)
            .frame(
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::symmetric(14, 12))
                    .stroke(egui::Stroke::new(1.0, LINE)),
            )
            .show(ctx, |ui| {
                let Some(doc) = self.doc.as_ref() else { return };
                ui.label(egui::RichText::new(t.tr("資訊")).strong().size(13.0));
                ui.add_space(8.0);
                let rows = [
                    (t.tr("檔名"), doc.name.clone()),
                    (t.tr("尺寸"), format!("{} × {} px", doc.width, doc.height)),
                    (t.tr("格式"), doc.format.clone()),
                    (t.tr("檔案大小"), format!("{:.1} KB", doc.bytes as f64 / 1024.0)),
                    (t.tr("修改時間"), doc.mtime.clone()),
                    (t.tr("目前路徑"), doc.path.display().to_string()),
                ];
                egui::Grid::new("info-grid").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                    for (k, v) in rows {
                        ui.label(egui::RichText::new(k).color(DIM).size(11.5));
                        ui.label(egui::RichText::new(v).color(TEXT).size(11.5));
                        ui.end_row();
                    }
                });
                ui.add_space(10.0);
                ui.separator();
                ui.add_space(6.0);
                ui.label(egui::RichText::new(format!("EXIF · {}", doc.exif.len())).color(DIM).size(11.5));
                ui.add_space(4.0);
                for (k, v) in &doc.exif {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(k).color(DIM).size(11.0));
                        ui.label(egui::RichText::new(v).color(TEXT).size(11.0));
                    });
                }
            });
    }

    fn tool_rail(&mut self, ctx: &egui::Context) {
        let t = self.locale;
        egui::SidePanel::left("rail")
            .exact_width(56.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::symmetric(8, 10))
                    .stroke(egui::Stroke::new(1.0, LINE)),
            )
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    for tool in [
                        Tool::Move,
                        Tool::Brush,
                        Tool::Line,
                        Tool::Rect,
                        Tool::Ellipse,
                        Tool::Arrow,
                        Tool::Text,
                        Tool::Crop,
                    ] {
                        let selected = self.tool == tool;
                        let label = t.tr(tool.label());
                        let btn = egui::Button::new(
                            egui::RichText::new(label.chars().next().unwrap_or('?').to_string())
                                .size(14.0),
                        )
                        .min_size(Vec2::splat(36.0))
                        .fill(if selected { Color32::from_rgb(64, 64, 68) } else { Color32::TRANSPARENT });
                        if ui.add(btn).on_hover_text(label).clicked() {
                            self.tool = tool;
                        }
                    }
                });
            });
    }

    fn status_bar(&mut self, ctx: &egui::Context) {
        let t = self.locale;
        egui::TopBottomPanel::bottom("status")
            .exact_height(26.0)
            .frame(
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::symmetric(12, 4))
                    .stroke(egui::Stroke::new(1.0, LINE)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if let Some(doc) = self.doc.as_ref() {
                        ui.label(
                            egui::RichText::new(format!(
                                "{} × {} · {} · {:.1} KB",
                                doc.width,
                                doc.height,
                                doc.format,
                                doc.bytes as f64 / 1024.0
                            ))
                            .color(DIM)
                            .size(11.0),
                        );
                    } else if let Some(err) = self.error.as_ref() {
                        ui.label(egui::RichText::new(err).color(Color32::from_rgb(239, 68, 68)).size(11.0));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let z = format!("{:.0}%", self.view.scale * 100.0);
                        if ui
                            .add(egui::Button::new(egui::RichText::new(z).size(11.0)).frame(false))
                            .on_hover_text(t.tr("適應視窗"))
                            .clicked()
                        {
                            self.view.fit = true;
                        }
                    });
                });
            });
    }

    fn canvas(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG))
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                let painter = ui.painter_at(rect);
                painter.rect_filled(rect, 0.0, BG);
                let Some(doc) = self.doc.as_ref() else {
                    painter.text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        self.locale.tr("尚未載入圖片"),
                        egui::FontId::proportional(14.0),
                        DIM,
                    );
                    return;
                };
                let img = Vec2::new(doc.width as f32, doc.height as f32);
                if self.view.fit {
                    self.view.scale = self.fit_scale(rect.size(), img);
                    self.view.offset = Vec2::ZERO;
                }
                let preview_scale = doc.decoded_scale();
                let need_full = doc.needs_full(self.view.scale);
                let (tex, _tex_scale) = {
                    let doc = self.doc.as_mut().expect("doc");
                    doc.texture_for(ctx, self.view.scale)
                };
                let size = img * self.view.scale;
                let draw_rect = Rect::from_center_size(rect.center() + self.view.offset, size);
                painter.image(
                    tex.id(),
                    draw_rect,
                    Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
                painter.rect_stroke(
                    draw_rect,
                    0.0,
                    egui::Stroke::new(1.0, Color32::from_rgba_premultiplied(255, 255, 255, 26)),
                    egui::StrokeKind::Outside,
                );
                if need_full && preview_scale < 1.0 {
                    ui.ctx().request_repaint();
                }

                // interactions
                let response = ui.interact(rect, ui.id().with("canvas"), egui::Sense::click_and_drag());
                let scroll = ui.input(|i| i.raw_scroll_delta.y);
                if scroll.abs() > 0.0 && response.hovered() {
                    let factor = (scroll * 0.0015).exp();
                    let anchor = response.hover_pos().unwrap_or(rect.center());
                    self.zoom_at(factor, anchor, rect.center());
                }
                if response.dragged_by(egui::PointerButton::Primary) && !self.view.fit {
                    self.view.offset += response.drag_delta();
                }
                if response.double_clicked() {
                    self.view.fit = true;
                }
            });
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        let (left, right, plus, minus, zero, info, edit, esc, fullscreen) = ctx.input(|i| {
            (
                i.key_pressed(Key::ArrowLeft),
                i.key_pressed(Key::ArrowRight),
                i.key_pressed(Key::Plus) || i.key_pressed(Key::Equals),
                i.key_pressed(Key::Minus),
                i.key_pressed(Key::Num0),
                i.key_pressed(Key::I),
                i.key_pressed(Key::E) || i.key_pressed(Key::V),
                i.key_pressed(Key::Escape),
                i.key_pressed(Key::F),
            )
        });
        if left {
            self.step(-1, ctx);
        }
        if right {
            self.step(1, ctx);
        }
        if plus {
            let center = ctx.screen_rect().center();
            self.zoom_at(1.25, center, center);
        }
        if minus {
            let center = ctx.screen_rect().center();
            self.zoom_at(0.8, center, center);
        }
        if zero {
            self.view.fit = true;
        }
        if info {
            self.show_info = !self.show_info;
        }
        if edit {
            self.phase = match self.phase {
                Phase::Preview => Phase::Edit,
                Phase::Edit => Phase::Preview,
            };
        }
        if fullscreen {
            self.fullscreen = !self.fullscreen;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
        }
        if esc {
            if self.phase == Phase::Edit {
                self.phase = Phase::Preview;
            } else if self.fullscreen {
                self.fullscreen = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}

fn ui_max_pixels(ctx: Option<&egui::Context>) -> u64 {
    let (w, h) = ctx
        .map(|c| {
            let r = c.screen_rect();
            (r.width().max(640.0), r.height().max(480.0))
        })
        .unwrap_or((1280.0, 820.0));
    ((w * h * 4.0) as u64).max(3_500_000)
}

impl eframe::App for LumenApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        Self::apply_theme(ctx);
        self.handle_keys(ctx);
        self.top_bar(ctx);
        if self.phase == Phase::Edit {
            self.tool_rail(ctx);
        }
        if self.show_info {
            self.info_panel(ctx);
        }
        self.status_bar(ctx);
        self.canvas(ctx);
    }
}

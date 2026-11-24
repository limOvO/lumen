// Save-as dialog (the web build's 「另存新檔」 modal).
//
// Parity source: src/js/ui/save-modal.js + the openSaveDialog/saveAs flow in
// src/js/main.js — folder + file name fields, JPEG/PNG/WebP, quality slider,
// optional target size (KB), live size estimate, overwrite warning and inline
// errors. The dialog never encodes or estimates anything itself: app.rs feeds
// it `export::estimate_size` output through the `estimate` argument.
//
// Public API (wired by app.rs, owner: dialogs-dev):
//   SaveDialog::new(suggested: &Path) -> SaveDialog          // open == true
//   dialog.open                                              // show/hide
//   dialog.dir / dialog.name / dialog.output / dialog.overwrite_ok / dialog.error
//   dialog.show(ctx, state, estimate) -> Option<SaveChoice>
//   dialog.open_with(suggested, output) / dialog.close()     // reuse helpers

use std::path::{Path, PathBuf};

use egui::{
    Align, Align2, Color32, CornerRadius, CursorIcon, FontId, Id, Order, Pos2, Rect, Sense,
    Stroke, StrokeKind, Vec2,
};

use crate::export::Estimated;
use crate::i18n::Locale;
use crate::state::{AppState, OutputFormat, OutputSettings};
use crate::theme;
use crate::ui::format::format_estimate;
use crate::ui::widgets;

const CARD_W: f32 = 520.0;
const HEAD_H: f32 = 58.0;
const FOOT_H: f32 = 46.0;
const PAD: f32 = 16.0;
const DIR_ID: &str = "lumen-save-dir";
const NAME_ID: &str = "lumen-save-name";
const TARGET_ID: &str = "lumen-save-target-kb";

/// The decision the app uses to write the file.
#[derive(Clone)]
pub struct SaveChoice {
    pub path: PathBuf,
    pub output: OutputSettings,
}

pub struct SaveDialog {
    pub open: bool,
    pub dir: String,
    pub name: String,
    pub output: OutputSettings,
    /// User ticked 「覆寫並儲存」 for the current target path.
    pub overwrite_ok: bool,
    pub error: Option<String>,
    /// Last positive target size, restored when the toggle is switched back on.
    last_target: u32,
    /// Target path the current `overwrite_ok` belongs to (any edit disarms it).
    armed_for: String,
    focus_name: bool,
}

impl SaveDialog {
    /// Prefills folder + file name from `suggested` (normally
    /// `save::suggested_name(doc, format)`).
    pub fn new(suggested: &Path) -> Self {
        let mut dialog = Self {
            open: true,
            dir: String::new(),
            name: String::new(),
            output: OutputSettings::default(),
            overwrite_ok: false,
            error: None,
            last_target: 400,
            armed_for: String::new(),
            focus_name: true,
        };
        dialog.apply_suggested(suggested);
        dialog
    }

    /// Re-open for a document: resets the fields from the suggestion and copies
    /// the current output settings into the dialog.
    pub fn open_with(&mut self, suggested: &Path, output: &OutputSettings) {
        self.output = output.clone();
        self.last_target = output.target_kb.unwrap_or(400).max(1);
        self.apply_suggested(suggested);
        self.overwrite_ok = false;
        self.armed_for.clear();
        self.error = None;
        self.open = true;
        self.focus_name = true;
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    fn apply_suggested(&mut self, suggested: &Path) {
        let text = suggested.to_string_lossy();
        let (dir, name) = split_path(&text);
        self.dir = dir;
        // With `Auto` the concrete encoder is only known in `show` (it needs the
        // document); keep the suggestion's extension so a PNG document never
        // flashes a `.jpg` name before the first frame resolves it.
        self.name = if self.output.format == OutputFormat::Auto {
            let sanitized = sanitize_file_name(&name);
            if has_known_extension(&sanitized) {
                sanitized
            } else {
                ensure_ext(&sanitized, OutputFormat::Jpeg)
            }
        } else {
            ensure_ext(&name, self.output.format)
        };
    }

    /// Identity of the settings this dialog would save with — lets app.rs
    /// refresh `state.estimate` only when something actually changed.
    pub fn settings_snapshot(&self) -> (OutputFormat, u8, Option<u32>, Option<f32>, Option<u32>) {
        (
            self.output.format,
            self.output.quality,
            self.output.max_dim,
            self.output.scale,
            self.output.target_kb,
        )
    }

    /// Draws the modal. Returns the chosen path + settings when confirmed.
    /// `estimate` is the app's current `export::estimate_size` result for
    /// `output` (refresh it when settings change; the dialog only displays it).
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        state: &AppState,
        estimate: Option<&Estimated>,
    ) -> Option<SaveChoice> {
        if !self.open {
            return None;
        }
        let locale = state.locale;
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.open = false;
            return None;
        }

        let doc_dir: Option<PathBuf> =
            state.doc.as_ref().and_then(|d| d.path.parent()).map(|p| p.to_path_buf());
        // The save modal always saves a concrete format (web: normalizeFormat);
        // `Auto` resolves against the document, else falls back to JPEG.
        let concrete = concrete_format(self.output.format, state.doc.as_ref());
        if concrete != self.output.format {
            self.output.format = concrete;
            self.name = ensure_ext(&self.name, concrete);
        }
        let screen = ctx.screen_rect();
        let height = (screen.height() - 56.0).clamp(380.0, 560.0);
        let body_h = (height - HEAD_H - FOOT_H).max(200.0);
        let total_h = HEAD_H + body_h + FOOT_H;
        // Never let the card exceed the window (resize safety); min window is 880x560.
        let avail = ctx.screen_rect().shrink(20.0);
        let card_w = CARD_W.min(avail.width());
        let card_h = total_h.min(avail.height());

        let scrim = egui::Area::new(Id::new("lumen-save-scrim"))
            .order(Order::Foreground)
            .fixed_pos(screen.left_top())
            .show(ctx, |ui| {
                let (rect, resp) = ui.allocate_exact_size(screen.size(), Sense::click());
                ui.painter().rect_filled(
                    rect,
                    CornerRadius::ZERO,
                    Color32::from_rgba_unmultiplied(10, 10, 12, 170),
                );
                resp
            });
        if scrim.inner.clicked() {
            self.open = false;
            return None;
        }

        let mut cancel = false;
        let mut confirm = false;
        let mut needs_ack = false;

        egui::Window::new(locale.tr("另存新檔"))
            .id(Id::new("lumen-save-dialog"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .movable(false)
            .order(Order::Foreground)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .fixed_size(Vec2::new(card_w, card_h))
            .frame(
                egui::Frame::new()
                    .fill(theme::PANEL)
                    .stroke(Stroke::new(1.0_f32, widgets::white(60)))
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(egui::Margin::ZERO),
            )
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                ui.set_width(card_w);
                ui.set_height(card_h);

                // --- head ---------------------------------------------------
                let (head, _) =
                    ui.allocate_exact_size(Vec2::new(card_w, HEAD_H), Sense::hover());
                {
                    let painter = ui.painter();
                    painter.text(
                        Pos2::new(head.left() + PAD, head.top() + 20.0),
                        Align2::LEFT_CENTER,
                        locale.tr("另存新檔"),
                        FontId::proportional(13.5),
                        theme::TEXT,
                    );
                    painter.text(
                        Pos2::new(head.left() + PAD, head.top() + 38.0),
                        Align2::LEFT_CENTER,
                        locale.tr("選擇儲存位置與輸出設定"),
                        FontId::proportional(10.5),
                        widgets::white(120),
                    );
                    painter.hline(
                        head.x_range(),
                        head.bottom() - 0.5,
                        Stroke::new(1.0_f32, widgets::white(24)),
                    );
                }
                let close_rect = Rect::from_center_size(
                    Pos2::new(head.right() - PAD - 14.0, head.top() + 25.0),
                    Vec2::splat(28.0),
                );
                let close =
                    ui.interact(close_rect, Id::new("lumen-save-close"), Sense::click());
                if close.hovered() {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    ui.painter().circle_filled(
                        close_rect.center(),
                        14.0,
                        widgets::white(24),
                    );
                }
                paint_close(
                    ui.painter(),
                    Rect::from_center_size(close_rect.center(), Vec2::splat(14.0)),
                    if close.hovered() { theme::TEXT } else { theme::DIM },
                );
                if close.clicked() {
                    cancel = true;
                }

                // --- body ---------------------------------------------------
                let (body_rect, _) =
                    ui.allocate_exact_size(Vec2::new(CARD_W, body_h), Sense::hover());
                let mut body_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(body_rect)
                        .layout(egui::Layout::top_down(Align::Min)),
                );
                body_ui.set_clip_rect(ui.clip_rect().intersect(body_rect));
                let body_w = CARD_W - PAD * 2.0;
                let enter_field = egui::ScrollArea::vertical()
                    .id_salt("lumen-save-body")
                    .auto_shrink([false, false])
                    .show(&mut body_ui, |ui| {
                        ui.set_width(body_w);
                        ui.spacing_mut().item_spacing = Vec2::ZERO;
                        ui.add_space(12.0);

                        // 儲存位置 -------------------------------------------
                        widgets::field_label(ui, locale.tr("儲存位置"));
                        ui.add_space(5.0);
                        let dir_resp =
                            text_input(ui, DIR_ID, &mut self.dir, "", true);
                        if dir_resp.changed() {
                            self.error = None;
                        }
                        let enter_dir = dir_resp.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter));

                        // 檔名 -------------------------------------------------
                        ui.add_space(10.0);
                        widgets::field_label(ui, locale.tr("檔名"));
                        ui.add_space(5.0);
                        let name_resp =
                            text_input(ui, NAME_ID, &mut self.name, "", true);
                        if self.focus_name {
                            name_resp.request_focus();
                            self.focus_name = false;
                        }
                        if name_resp.changed() {
                            self.error = None;
                        }
                        let enter_name = name_resp.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter));

                        // 格式 -------------------------------------------------
                        ui.add_space(12.0);
                        widgets::field_label(ui, locale.tr("格式"));
                        ui.add_space(5.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 5.0;
                            for format in
                                [OutputFormat::Jpeg, OutputFormat::Png, OutputFormat::Webp]
                            {
                                let active = self.output.format == format;
                                if widgets::chip(ui, format.label(), active, true).clicked()
                                    && !active
                                {
                                    // Web behaviour: renaming keeps the stem, swaps the ext.
                                    self.output.format = format;
                                    self.name = ensure_ext(&self.name, format);
                                    self.overwrite_ok = false;
                                    self.error = None;
                                }
                            }
                        });

                        // 品質 -------------------------------------------------
                        ui.add_space(12.0);
                        let target_on = self.output.target_kb.is_some();
                        let quality_disabled =
                            self.output.format == OutputFormat::Png || target_on;
                        ui.horizontal(|ui| {
                            widgets::field_label(ui, locale.tr("品質"));
                            ui.with_layout(
                                egui::Layout::right_to_left(Align::Center),
                                |ui| {
                                    widgets::text(
                                        ui,
                                        format!("{}", self.output.quality),
                                        10.5,
                                        if quality_disabled {
                                            theme::DIM
                                        } else {
                                            theme::TEXT
                                        },
                                    );
                                },
                            );
                        });
                        ui.add_space(6.0);
                        let mut quality = self.output.quality as f32;
                        if widgets::slider(
                            ui,
                            &mut quality,
                            1.0,
                            100.0,
                            ui.available_width(),
                            !quality_disabled,
                        )
                        .changed()
                        {
                            self.output.quality =
                                quality.round().clamp(1.0, 100.0) as u8;
                            self.error = None;
                        }
                        if self.output.format == OutputFormat::Png {
                            ui.add_space(4.0);
                            widgets::hint(
                                ui,
                                locale.tr("PNG 為無損格式，不使用品質設定。"),
                            );
                        }

                        // 目標大小 ---------------------------------------------
                        ui.add_space(12.0);
                        widgets::field_label(ui, locale.tr("目標大小"));
                        ui.add_space(5.0);
                        ui.horizontal(|ui| {
                            if toggle_box(ui, target_on).clicked() {
                                if target_on {
                                    self.output.target_kb = None;
                                } else {
                                    self.output.target_kb =
                                        Some(self.last_target.max(1));
                                }
                                self.error = None;
                            }
                            ui.add_space(8.0);
                            let field_w = (ui.available_width() - 42.0).max(70.0);
                            if let Some(value) = widgets::number_field(
                                ui,
                                TARGET_ID,
                                self.output.target_kb,
                                &locale.tr("自動"),
                                target_on,
                                field_w,
                            ) {
                                self.output.target_kb = Some(value);
                                self.last_target = value;
                                self.error = None;
                            }
                            widgets::text(ui, "KB", 10.5, widgets::white(110));
                        });
                        if target_on {
                            ui.add_space(4.0);
                            widgets::hint(
                                ui,
                                locale.tr("已啟用目標大小，將自動調整品質。"),
                            );
                        }

                        // 預估大小 ---------------------------------------------
                        ui.add_space(14.0);
                        let (estimate_row, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 26.0),
                            Sense::hover(),
                        );
                        let estimate_text = if let Some(kb) = self.output.target_kb {
                            target_hint(locale, kb)
                        } else {
                            match estimate {
                                Some(est) => {
                                    format_estimate(Some(est.bytes), est.width, est.height)
                                }
                                None => "—".to_string(),
                            }
                        };
                        {
                            let painter = ui.painter();
                            painter.text(
                                Pos2::new(estimate_row.left(), estimate_row.center().y),
                                Align2::LEFT_CENTER,
                                locale.tr("預估大小"),
                                FontId::proportional(11.0),
                                theme::DIM,
                            );
                            painter.text(
                                Pos2::new(estimate_row.right(), estimate_row.center().y),
                                Align2::RIGHT_CENTER,
                                estimate_text,
                                FontId::monospace(10.5),
                                if self.output.target_kb.is_some() {
                                    widgets::white(150)
                                } else {
                                    theme::TEXT
                                },
                            );
                        }

                        // 覆寫確認 ---------------------------------------------
                        let target_now =
                            resolve_target(&self.dir, &self.name, self.output.format, doc_dir.as_deref());
                        let target_key = target_now
                            .as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_default();
                        if !ack_still_valid(self.overwrite_ok, &self.armed_for, &target_key)
                        {
                            self.overwrite_ok = false;
                        }
                        self.armed_for = target_key;
                        let exists =
                            target_now.as_ref().map_or(false, |p| p.is_file());
                        needs_ack = exists && !self.overwrite_ok;
                        if exists {
                            ui.add_space(10.0);
                            let (warn, _) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 30.0),
                                Sense::hover(),
                            );
                            let inner = warn.shrink2(Vec2::new(0.0, 4.0));
                            let painter = ui.painter();
                            painter.rect_filled(
                                inner,
                                CornerRadius::same(7),
                                Color32::from_rgba_unmultiplied(245, 158, 11, 26),
                            );
                            painter.rect_stroke(
                                inner,
                                CornerRadius::same(7),
                                Stroke::new(
                                    1.0_f32,
                                    Color32::from_rgba_unmultiplied(245, 158, 11, 82),
                                ),
                                StrokeKind::Inside,
                            );
                            painter.text(
                                Pos2::new(inner.left() + 9.0, inner.center().y),
                                Align2::LEFT_CENTER,
                                locale.tr("同名檔案已存在，儲存後將覆寫原檔案。"),
                                FontId::proportional(11.0),
                                theme::WARN,
                            );
                            ui.add_space(6.0);
                            let was_ok = self.overwrite_ok;
                            if toggle_row(ui, &locale.tr("覆寫並儲存"), was_ok) {
                                self.overwrite_ok = !was_ok;
                                needs_ack = exists && !self.overwrite_ok;
                            }
                        }

                        // 錯誤 -------------------------------------------------
                        if let Some(err) = self.error.clone() {
                            ui.add_space(10.0);
                            let (strip, _) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 30.0),
                                Sense::hover(),
                            );
                            let inner = strip.shrink2(Vec2::new(0.0, 4.0));
                            let painter = ui.painter();
                            painter.rect_filled(
                                inner,
                                CornerRadius::same(7),
                                Color32::from_rgba_unmultiplied(239, 68, 68, 26),
                            );
                            painter.rect_stroke(
                                inner,
                                CornerRadius::same(7),
                                Stroke::new(
                                    1.0_f32,
                                    Color32::from_rgba_unmultiplied(239, 68, 68, 90),
                                ),
                                StrokeKind::Inside,
                            );
                            painter.text(
                                Pos2::new(inner.left() + 9.0, inner.center().y),
                                Align2::LEFT_CENTER,
                                err,
                                FontId::proportional(11.0),
                                theme::ERR,
                            );
                        }

                        ui.add_space(12.0);
                        enter_name || enter_dir
                    })
                    .inner;
                if enter_field {
                    confirm = true;
                }

                // --- footer -------------------------------------------------
                let (foot, _) =
                    ui.allocate_exact_size(Vec2::new(CARD_W, FOOT_H), Sense::hover());
                let row_y = foot.center().y - 15.0;
                let primary_w = 96.0;
                let cancel_w = 76.0;
                let cancel_rect = Rect::from_min_size(
                    Pos2::new(foot.right() - PAD - primary_w - 8.0 - cancel_w, row_y),
                    Vec2::new(cancel_w, 30.0),
                );
                let primary_rect = Rect::from_min_size(
                    Pos2::new(foot.right() - PAD - primary_w, row_y),
                    Vec2::new(primary_w, 30.0),
                );
                {
                    ui.painter().hline(
                        foot.x_range(),
                        foot.top() + 0.5,
                        Stroke::new(1.0_f32, widgets::white(24)),
                    );
                }
                if painted_button(
                    ui,
                    cancel_rect,
                    &locale.tr("取消"),
                    false,
                    true,
                ) {
                    cancel = true;
                }
                let label = if needs_ack {
                    locale.tr("覆寫並儲存")
                } else {
                    locale.tr("儲存")
                };
                if painted_button(ui, primary_rect, &label, true, !needs_ack) {
                    confirm = true;
                }
            });

        if cancel {
            self.open = false;
            return None;
        }
        if !confirm {
            return None;
        }

        // Validate + emit.
        if sanitize_file_name(&self.name).is_empty() {
            self.error = Some(locale.tr("請輸入檔名"));
            return None;
        }
        let dir_text = self.dir.trim();
        if !dir_text.is_empty() && !resolve_dir(dir_text, doc_dir.as_deref()).is_dir() {
            self.error = Some(locale.tr("路徑不存在"));
            return None;
        }
        let Some(path) = resolve_target(
            &self.dir,
            &self.name,
            self.output.format,
            doc_dir.as_deref(),
        ) else {
            self.error = Some(locale.tr("請輸入檔名"));
            return None;
        };
        self.name = ensure_ext(&self.name, self.output.format);
        if path.is_file() && !self.overwrite_ok {
            // The confirm button is disabled in this state; never emit a silent
            // overwrite if a file appeared between frames.
            self.overwrite_ok = true;
            return None;
        }
        self.open = false;
        Some(SaveChoice { path, output: self.output.clone() })
    }
}

// ---------------------------------------------------------------------------
// Painted controls
// ---------------------------------------------------------------------------

fn text_input(
    ui: &mut egui::Ui,
    id: &str,
    buffer: &mut String,
    hint: &str,
    mono: bool,
) -> egui::Response {
    let width = ui.available_width();
    let font = if mono {
        FontId::monospace(11.0)
    } else {
        FontId::proportional(11.5)
    };
    let resp = ui.add(
        egui::TextEdit::singleline(buffer)
            .id(Id::new(id))
            .hint_text(hint)
            .font(font)
            .text_color(theme::TEXT)
            .background_color(theme::BG)
            .margin(egui::Margin::symmetric(8, 6))
            .desired_width(width)
            .min_size(Vec2::new(width, 30.0))
            .vertical_align(Align::Center),
    );
    ui.painter().rect_stroke(
        resp.rect.expand(1.0),
        CornerRadius::same(7),
        Stroke::new(
            1.0_f32,
            if resp.has_focus() { theme::TEXT } else { widgets::white(38) },
        ),
        StrokeKind::Outside,
    );
    resp
}

fn painted_button(
    ui: &mut egui::Ui,
    rect: Rect,
    label: &str,
    primary: bool,
    enabled: bool,
) -> bool {
    let resp = ui.interact(rect, ui.id().with(("lumen-save-btn", label)), Sense::click());
    let hovered = enabled && resp.hovered();
    let painter = ui.painter();
    let radius = CornerRadius::same(widgets::RADIUS);
    if primary {
        painter.rect_filled(
            rect,
            radius,
            if !enabled {
                widgets::white(40)
            } else if hovered {
                Color32::WHITE
            } else {
                theme::TEXT
            },
        );
    } else {
        if hovered {
            painter.rect_filled(rect, radius, widgets::white(15));
        }
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(1.0_f32, widgets::white(36)),
            StrokeKind::Inside,
        );
    }
    let color = if !enabled {
        widgets::disabled(theme::DIM)
    } else if primary {
        theme::BG
    } else if hovered {
        theme::TEXT
    } else {
        theme::DIM
    };
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(11.5),
        color,
    );
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    enabled && resp.clicked()
}

fn toggle_box(ui: &mut egui::Ui, on: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::click());
    let painter = ui.painter();
    let radius = CornerRadius::same(4);
    painter.rect_filled(
        rect,
        radius,
        if on { theme::TEXT } else { theme::BG },
    );
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(
            1.0_f32,
            if on { theme::TEXT } else { widgets::white(60) },
        ),
        StrokeKind::Inside,
    );
    if on {
        paint_check(
            painter,
            Rect::from_center_size(rect.center(), Vec2::splat(11.0)),
            theme::BG,
        );
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp
}

fn toggle_row(ui: &mut egui::Ui, label: &str, on: bool) -> bool {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::click());
    let painter = ui.painter();
    let box_rect = Rect::from_center_size(
        Pos2::new(rect.left() + 8.0, rect.center().y),
        Vec2::splat(14.0),
    );
    let radius = CornerRadius::same(4);
    painter.rect_filled(box_rect, radius, if on { theme::TEXT } else { theme::BG });
    painter.rect_stroke(
        box_rect,
        radius,
        Stroke::new(
            1.0_f32,
            if on { theme::TEXT } else { widgets::white(60) },
        ),
        StrokeKind::Inside,
    );
    if on {
        paint_check(
            painter,
            Rect::from_center_size(box_rect.center(), Vec2::splat(10.0)),
            theme::BG,
        );
    }
    painter.text(
        Pos2::new(box_rect.right() + 8.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(11.0),
        if on { theme::TEXT } else { theme::DIM },
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.clicked()
}

fn paint_close(painter: &egui::Painter, rect: Rect, color: Color32) {
    let stroke = Stroke::new(1.8_f32, color);
    painter.line_segment(
        [Pos2::new(rect.left(), rect.top()), Pos2::new(rect.right(), rect.bottom())],
        stroke,
    );
    painter.line_segment(
        [Pos2::new(rect.right(), rect.top()), Pos2::new(rect.left(), rect.bottom())],
        stroke,
    );
}

fn paint_check(painter: &egui::Painter, rect: Rect, color: Color32) {
    let p = |x: f32, y: f32| {
        Pos2::new(rect.left() + x * rect.width(), rect.top() + y * rect.height())
    };
    painter.add(egui::Shape::line(
        vec![p(0.08, 0.52), p(0.4, 0.84), p(0.92, 0.16)],
        Stroke::new(1.9_f32, color),
    ));
}

// ---------------------------------------------------------------------------
// Pure helpers (unit-tested below).
// ---------------------------------------------------------------------------

/// Keep only the last path segment, drop control characters, trim whitespace
/// and trailing dots. `/` and `\` cannot end up in a file name.
fn sanitize_file_name(raw: &str) -> String {
    let last = raw.rsplit(['/', '\\']).next().unwrap_or(raw);
    let cleaned: String = last.chars().filter(|c| !c.is_control()).collect();
    let mut out = cleaned.trim().to_string();
    while out.ends_with('.') || out.ends_with(' ') {
        out.pop();
    }
    out
}

/// Drop the last `.ext` when the extension is alphanumeric (mirrors the web
/// regex `/\.[A-Za-z0-9]+$/`).
fn strip_ext(name: &str) -> &str {
    if let Some(index) = name.rfind('.') {
        let ext = &name[index + 1..];
        if !ext.is_empty() && ext.chars().all(|c| c.is_ascii_alphanumeric()) {
            return &name[..index];
        }
    }
    name
}

/// Normalise the file name to the selected format's extension.
fn ensure_ext(raw: &str, format: OutputFormat) -> String {
    let sanitized = sanitize_file_name(raw);
    let stem = strip_ext(&sanitized).trim();
    let stem = if stem.is_empty() { "未命名" } else { stem };
    format!("{stem}.{}", format.ext())
}

/// Recognised output extension (the ones the save-as dialog writes).
fn has_known_extension(name: &str) -> bool {
    let Some((_, ext)) = name.rsplit_once('.') else { return false };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "jpg" | "jpeg" | "png" | "webp"
    )
}

fn split_path(path: &str) -> (String, String) {
    let value = path.trim();
    if value.is_empty() {
        return (String::new(), String::new());
    }
    match value.rfind(['/', '\\']) {
        None => (String::new(), value.to_string()),
        Some(0) => (value[..1].to_string(), value[1..].to_string()),
        Some(index) => (value[..index].to_string(), value[index + 1..].to_string()),
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

fn expand_home(text: &str) -> PathBuf {
    if let Some(home) = home_dir() {
        if text == "~" {
            return home;
        }
        if let Some(rest) = text.strip_prefix("~/") {
            return home.join(rest);
        }
    }
    PathBuf::from(text)
}

fn resolve_dir(dir: &str, fallback: Option<&Path>) -> PathBuf {
    let text = dir.trim();
    if text.is_empty() {
        return fallback.map(|p| p.to_path_buf()).unwrap_or_default();
    }
    expand_home(text)
}

/// Absolute-ish target path for the current fields, or `None` when the file
/// name is empty after sanitisation.
fn resolve_target(
    dir: &str,
    name: &str,
    format: OutputFormat,
    fallback_dir: Option<&Path>,
) -> Option<PathBuf> {
    if sanitize_file_name(name).is_empty() {
        return None;
    }
    let file = ensure_ext(name, format);
    Some(resolve_dir(dir, fallback_dir).join(file))
}

/// `overwrite_ok` only applies to the target it was ticked for; an empty
/// target (no file name yet) never keeps an acknowledgement.
fn ack_still_valid(ok: bool, armed_for: &str, target: &str) -> bool {
    !target.is_empty() && ok && armed_for == target
}

/// Save-as never writes `Auto`: resolve it against the document like the web
/// modal's `normalizeFormat` (JPEG when no document is available).
fn concrete_format(format: OutputFormat, doc: Option<&crate::doc::Doc>) -> OutputFormat {
    match format {
        OutputFormat::Auto => doc
            .map(|doc| OutputFormat::Auto.resolve(doc))
            .unwrap_or(OutputFormat::Jpeg),
        other => other,
    }
}

/// 「依目標 N KB 自動調整」 — the dictionary key keeps the web's `${…}` token.
fn target_hint(locale: Locale, kb: u32) -> String {
    locale
        .tr("依目標 ${Math.round(target)} KB 自動調整")
        .replace("${Math.round(target)}", &kb.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("lumen-dialog-tests-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn sanitize_drops_separators_controls_and_trailing_dots() {
        assert_eq!(sanitize_file_name("  photo.png  "), "photo.png");
        assert_eq!(sanitize_file_name("dir/photo.png"), "photo.png");
        assert_eq!(sanitize_file_name("dir\\photo.png"), "photo.png");
        assert_eq!(sanitize_file_name("bad\tna\nme.png"), "badname.png");
        assert_eq!(sanitize_file_name("trailing..."), "trailing");
        assert_eq!(sanitize_file_name("   "), "");
        assert_eq!(sanitize_file_name(".."), "");
        assert_eq!(sanitize_file_name("照片 01.JPG"), "照片 01.JPG");
    }

    #[test]
    fn extension_is_normalised_to_the_selected_format() {
        assert_eq!(ensure_ext("photo", OutputFormat::Jpeg), "photo.jpg");
        assert_eq!(ensure_ext("photo.PNG", OutputFormat::Png), "photo.png");
        assert_eq!(ensure_ext("photo.jpeg", OutputFormat::Webp), "photo.webp");
        assert_eq!(ensure_ext("a.b.c.webp", OutputFormat::Jpeg), "a.b.c.jpg");
        // Empty stem falls back to the dictionary's placeholder name.
        assert_eq!(ensure_ext("", OutputFormat::Png), "未命名.png");
        assert_eq!(ensure_ext(".hidden", OutputFormat::Png), "未命名.png");
        // No extension / dots that are not an extension.
        assert_eq!(ensure_ext("no ext", OutputFormat::Jpeg), "no ext.jpg");
        assert_eq!(ensure_ext("v1.2 beta", OutputFormat::Png), "v1.2 beta.png");
    }

    #[test]
    fn strip_ext_matches_the_web_regex() {
        assert_eq!(strip_ext("a.jpg"), "a");
        assert_eq!(strip_ext("a.tar.gz"), "a.tar");
        assert_eq!(strip_ext("noext"), "noext");
        assert_eq!(strip_ext(".hidden"), "");
        assert_eq!(strip_ext("a."), "a.");
        assert_eq!(strip_ext("a.b-c"), "a.b-c");
    }

    #[test]
    fn split_path_handles_unix_and_windows_style() {
        assert_eq!(
            split_path("/home/lumen/out/IMG_1-edit.jpg"),
            ("/home/lumen/out".to_string(), "IMG_1-edit.jpg".to_string())
        );
        assert_eq!(
            split_path("/root.png").clone(),
            ("/".to_string(), "root.png".to_string())
        );
        assert_eq!(split_path("bare.png"), (String::new(), "bare.png".to_string()));
        assert_eq!(split_path("C:\\pics\\x.png"), ("C:\\pics".to_string(), "x.png".to_string()));
        assert_eq!(
            split_path("/home/lumen/out/"),
            ("/home/lumen/out".to_string(), String::new())
        );
        assert_eq!(split_path("/"), ("/".to_string(), String::new()));
    }

    #[test]
    fn resolve_target_uses_fallback_dir_and_sanitises() {
        let fallback = PathBuf::from("/tmp/photos");
        assert_eq!(
            resolve_target("", "IMG_1.jpg", OutputFormat::Jpeg, Some(&fallback)),
            Some(PathBuf::from("/tmp/photos/IMG_1.jpg"))
        );
        assert_eq!(
            resolve_target("/out", "IMG_1.jpg", OutputFormat::Png, None),
            Some(PathBuf::from("/out/IMG_1.png"))
        );
        // A pasted path in the name field keeps only the file name.
        assert_eq!(
            resolve_target("/out", "/tmp/other/name.webp", OutputFormat::Jpeg, None),
            Some(PathBuf::from("/out/name.jpg"))
        );
        // Empty name -> nothing to save.
        assert_eq!(resolve_target("/out", "   ", OutputFormat::Jpeg, None), None);
        // Directory trimming + a bare relative target.
        assert_eq!(
            resolve_target("  /out  ", "x", OutputFormat::Webp, None),
            Some(PathBuf::from("/out/x.webp"))
        );
        assert_eq!(
            resolve_target("", "x.png", OutputFormat::Png, None),
            Some(PathBuf::from("x.png"))
        );
    }

    #[test]
    fn overwrite_detection_sees_existing_files() {
        let dir = temp_dir("overwrite");
        let existing = dir.join("shot.png");
        std::fs::write(&existing, b"old").expect("write");
        let dir_text = dir.to_string_lossy().to_string();

        let target = resolve_target(&dir_text, "shot.png", OutputFormat::Png, None)
            .expect("target");
        assert_eq!(target, existing);
        assert!(target.is_file());

        let fresh = resolve_target(&dir_text, "new.png", OutputFormat::Png, None)
            .expect("target");
        assert!(!fresh.is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn overwrite_ack_is_tied_to_the_target_path() {
        assert!(ack_still_valid(true, "/out/a.jpg", "/out/a.jpg"));
        assert!(!ack_still_valid(true, "/out/a.jpg", "/out/b.jpg"));
        assert!(!ack_still_valid(false, "/out/a.jpg", "/out/a.jpg"));
        // Empty target (no file name yet) never keeps an ack.
        assert!(!ack_still_valid(true, "", ""));
    }

    #[test]
    fn target_hint_fills_the_web_token() {
        let zh = target_hint(Locale::ZhHant, 500);
        assert!(zh.contains("500"), "{zh}");
        assert!(!zh.contains("${"), "token must be replaced: {zh}");
        let en = target_hint(Locale::En, 250);
        assert!(en.contains("250"), "{en}");
        assert!(!en.contains("${"), "token must be replaced: {en}");
    }

    #[test]
    fn show_draws_two_frames_without_panicking() {
        let ctx = egui::Context::default();
        let state = AppState::new(Locale::ZhHant);
        let mut input = egui::RawInput::default();
        input.screen_rect = Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::Vec2::new(1280.0, 820.0),
        ));
        let mut dialog = SaveDialog::new(Path::new("/tmp/lumen-shot-edit.jpg"));
        let mut choice = None;
        for _ in 0..2 {
            let _ = ctx.run(input.clone(), |ctx| {
                choice = dialog.show(ctx, &state, None);
            });
        }
        assert!(choice.is_none());
        assert!(dialog.open, "the dialog only closes on confirm/cancel");
    }

    #[test]
    fn new_prefills_from_the_suggested_name() {
        let dialog = SaveDialog::new(Path::new("/home/lumen/pics/IMG_1-edit.jpg"));
        assert!(dialog.open);
        assert_eq!(dialog.dir, "/home/lumen/pics");
        assert_eq!(dialog.name, "IMG_1-edit.jpg");
        assert!(!dialog.overwrite_ok);
        assert!(dialog.error.is_none());
        // The Lead's OutputSettings defaults to Auto; `show` resolves it to a
        // concrete encoder before anything is written.
        assert_eq!(dialog.output.format, OutputFormat::Auto);
    }

    #[test]
    fn auto_prefill_keeps_the_suggested_extension() {
        // Default output is Auto: the suggested .png must survive prefill.
        let dialog = SaveDialog::new(Path::new("/pics/holiday-edit.png"));
        assert_eq!(dialog.dir, "/pics");
        assert_eq!(dialog.name, "holiday-edit.png");
        // Unknown/absent extension falls back to JPEG until `show` resolves.
        let dialog = SaveDialog::new(Path::new("/pics/holiday-edit.tiff"));
        assert_eq!(dialog.name, "holiday-edit.jpg");
        let dialog = SaveDialog::new(Path::new("/pics/holiday"));
        assert_eq!(dialog.name, "holiday.jpg");
        // Explicit output format still wins over the suggestion.
        let mut dialog = SaveDialog::new(Path::new("/pics/holiday-edit.png"));
        dialog.output.format = OutputFormat::Webp;
        dialog.apply_suggested(Path::new("/pics/holiday-edit.png"));
        assert_eq!(dialog.name, "holiday-edit.webp");
    }

    #[test]
    fn settings_snapshot_changes_with_the_fields_app_rs_estimates_from() {
        let mut dialog = SaveDialog::new(Path::new("/pics/a-edit.jpg"));
        let before = dialog.settings_snapshot();
        assert_eq!(before.0, OutputFormat::Auto);
        dialog.output.format = OutputFormat::Png;
        dialog.output.quality = 77;
        dialog.output.max_dim = Some(2000);
        dialog.output.scale = Some(0.5);
        dialog.output.target_kb = Some(500);
        assert_ne!(before, dialog.settings_snapshot());
        assert_eq!(
            dialog.settings_snapshot(),
            (OutputFormat::Png, 77, Some(2000), Some(0.5), Some(500))
        );
    }

    #[test]
    fn open_with_copies_output_settings_and_resets_ack() {
        let mut dialog = SaveDialog::new(Path::new("/tmp/a-edit.png"));
        dialog.overwrite_ok = true;
        let output = OutputSettings {
            format: OutputFormat::Webp,
            quality: 72,
            max_dim: Some(1600),
            target_kb: Some(300),
            ..Default::default()
        };
        dialog.open_with(Path::new("/tmp/a-edit.png"), &output);
        assert_eq!(dialog.output.format, OutputFormat::Webp);
        assert_eq!(dialog.output.quality, 72);
        assert_eq!(dialog.output.max_dim, Some(1600));
        assert_eq!(dialog.output.target_kb, Some(300));
        assert!(!dialog.overwrite_ok, "re-opening must re-confirm overwrites");
        // Name is normalised to the output format's extension.
        assert_eq!(dialog.name, "a-edit.webp");
        assert_eq!(dialog.last_target, 300);
    }

    #[test]
    fn auto_format_resolves_before_saving() {
        // No document: JPEG, exactly like normalizeFormat's fallback.
        assert_eq!(concrete_format(OutputFormat::Auto, None), OutputFormat::Jpeg);
        // Explicit formats pass through untouched.
        assert_eq!(concrete_format(OutputFormat::Png, None), OutputFormat::Png);
        assert_eq!(concrete_format(OutputFormat::Webp, None), OutputFormat::Webp);
    }

    #[test]
    fn format_switch_keeps_the_stem() {
        let mut name = ensure_ext("holiday.2024.png", OutputFormat::Png);
        assert_eq!(name, "holiday.2024.png");
        name = ensure_ext(&name, OutputFormat::Jpeg);
        assert_eq!(name, "holiday.2024.jpg");
        name = ensure_ext(&name, OutputFormat::Webp);
        assert_eq!(name, "holiday.2024.webp");
    }
}

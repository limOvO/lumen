// Orchestrator: owns AppState, drives the UI modules, paints the canvas and
// delegates gestures/annotations/crop to the feature modules.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

use egui::{Pos2, Rect, Vec2};

use crate::browser::Browser;
use crate::dialogs::{SaveChoice, SaveDialog};
use crate::doc::{siblings, Doc, LoadOptions};
use crate::interaction::{self, CanvasView};
use crate::state::{AppState, Phase, StatusKind, UiAction};
use crate::native_dialog::{NativeDialog, Outcome};
use crate::state::OutputFormat;
use crate::{export, save};
use crate::{theme, ui};

/// What the running system dialog will do with its result.
enum NativeKind {
    Open,
    Save(crate::state::OutputSettings),
}

fn use_internal_browser() -> bool {
    std::env::var_os("LUMEN_INTERNAL_BROWSER").is_some()
}

/// Single-parameter preview↔edit glide. Scale, position and the (animating)
/// canvas rect all interpolate with one eased parameter, so every image corner
/// travels in a straight line instead of a slide-then-shrink curve.
#[derive(Clone, Copy)]
struct PhaseAnim {
    t0: f64,
    from_rect: egui::Rect,
    to_rect: egui::Rect,
    from_scale: f32,
    from_off: Vec2,
    to_scale: f32,
}

const PHASE_GLIDE: f64 = 0.30;

pub struct LumenApp {
    pub state: AppState,
    estimate_key: String,
    estimate_at: f64,
    focused_once: bool,
    pub browser: Browser,
    pub dialog: SaveDialog,
    dialog_snap: Option<(OutputFormat, u8, Option<u32>, Option<f32>, Option<u32>)>,
    incoming: Option<Receiver<PathBuf>>,
    last_phase: Phase,
    phase_pending: bool,
    phase_anim: Option<PhaseAnim>,
    debug_last_zoom: f32,
    native: Option<(NativeDialog, NativeKind)>,
    switch: Option<(f32, f64)>,                   // direction, t0
    switch_dir: f32,
    perf: bool,
    frame_ms: [f32; 30],
    frame_idx: usize,
}

impl LumenApp {
    fn log_event(&self, msg: &str) {
        if std::env::var_os("LUMEN_LOG").is_none() {
            return;
        }
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/tmp/lumen-egui.log")
        {
            let _ = writeln!(
                f,
                "{:?} phase={} doc={:?} browser={} dialog={} compare={} err={:?} :: {}",
                std::time::SystemTime::now(),
                match self.state.phase {
                    Phase::Preview => "preview",
                    Phase::Edit => "edit",
                },
                self.state.doc.as_ref().map(|d| d.name.clone()),
                self.browser.open,
                self.dialog.open,
                self.state.compare,
                self.state.error,
                msg
            );
        }
    }
    pub fn new(initial: Option<PathBuf>, incoming: Option<Receiver<PathBuf>>) -> Self {
        let mut app = Self {
            state: AppState::new(crate::i18n::Locale::detect()),
            estimate_key: String::new(),
            estimate_at: 0.0,
            focused_once: false,
            browser: {
                let mut b = Browser::new(None);
                b.open = false;
                b
            },
            dialog: {
                let mut d = SaveDialog::new(Path::new("image.jpg"));
                d.open = false;
                d
            },
            dialog_snap: None,
            incoming,
            last_phase: Phase::Preview,
            phase_pending: false,
            phase_anim: None,
            debug_last_zoom: -1.0,
            native: None,
            switch: None,
            switch_dir: 0.0,
            perf: false,
            frame_ms: [0.0; 30],
            frame_idx: 0,
        };
        if let Some(path) = initial {
            app.open(&path, None);
        }
        app
    }

    pub fn open(&mut self, path: &Path, ctx: Option<&egui::Context>) {
        let cap = max_pixels(ctx);
        match Doc::load(path, LoadOptions { max_pixels: cap }) {
            Ok(doc) => {
                let name = doc.name.clone();
                if self.state.doc.is_some() {
                    let t = now();
                    self.switch = Some((self.switch_dir, t));
                    self.switch_dir = 0.0;
                }
                self.state.siblings = siblings(path);
                self.state.index = self.state.siblings.iter().position(|p| p == path).unwrap_or(0);
                let cap = max_pixels(ctx);
                let neighbours: Vec<std::path::PathBuf> = {
                    let i = self.state.index;
                    [-1i64, 1]
                        .iter()
                        .filter_map(|d| {
                            let j = i as i64 + d;
                            if j >= 0 && (j as usize) < self.state.siblings.len() {
                                Some(self.state.siblings[j as usize].clone())
                            } else {
                                None
                            }
                        })
                        .collect()
                };
                if !neighbours.is_empty() {
                    // One worker decodes both neighbours sequentially (lower peak
                    // memory than two concurrent decodes) after the first paint.
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(250));
                        for path in neighbours {
                            crate::doc::preload_preview(path, cap);
                        }
                    });
                }
                self.state.doc = Some(doc);
                self.state.error = None;
                self.state.reset_for_doc();
                export::clear_decode_cache();
                if let Some(ctx) = ctx {
                    let title = format!("{name} — Lumen");
                    ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
                }
            }
            Err(err) => {
                self.state.error = Some(self.state.locale.tr(&err));
            }
        }
    }

    fn step(&mut self, delta: i64, ctx: &egui::Context) {
        if self.state.siblings.is_empty() {
            return;
        }
        self.switch_dir = if delta >= 0 { 1.0 } else { -1.0 };
        let n = self.state.siblings.len() as i64;
        let next = ((self.state.index as i64 + delta + n) % n) as usize;
        let path = self.state.siblings[next].clone();
        self.open(&path, Some(ctx));
    }

    fn apply_theme(ctx: &egui::Context) {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = theme::PANEL;
        visuals.window_fill = theme::PANEL;
        visuals.extreme_bg_color = theme::BG;
        visuals.override_text_color = Some(theme::TEXT);
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, theme::LINE);
        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(40, 40, 42);
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(52, 52, 55);
        visuals.widgets.active.bg_fill = egui::Color32::from_rgb(64, 64, 68);
        ctx.set_visuals(visuals);
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        use egui::Key;
        let (left, right, plus, minus, zero, info, edit, esc, full, _undo_kp, _redo_kp, del, shortcuts) =
            ctx.input(|i| {
                (
                    i.key_pressed(Key::ArrowLeft),
                    i.key_pressed(Key::ArrowRight),
                    i.key_pressed(Key::Plus) || i.key_pressed(Key::Equals),
                    i.key_pressed(Key::Minus),
                    i.key_pressed(Key::Num0),
                    i.key_pressed(Key::I),
                    i.key_pressed(Key::E),
                    i.key_pressed(Key::Escape),
                    i.key_pressed(Key::F),
                    i.modifiers.command && i.key_pressed(Key::Z) && !i.modifiers.shift,
                    i.modifiers.command && i.key_pressed(Key::Z) && i.modifiers.shift,
                    i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace),
                    i.key_pressed(Key::Questionmark),
                )
            });
        // Modifier chords must be read from the Key event itself: X11 sends the
        // modifier and key state separately and a frame boundary can split them.
        let chords: Vec<(Key, egui::Modifiers)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key { key, pressed: true, modifiers, .. } => Some((*key, *modifiers)),
                    _ => None,
                })
                .collect()
        });
        let chord = |k: Key, cmd: bool, shift: bool| {
            chords
                .iter()
                .any(|(key, m)| *key == k && m.command == cmd && m.shift == shift)
        };
        let open_acc = chord(Key::O, true, false);
        let save_acc = chord(Key::S, true, false);
        let save_as_acc = chord(Key::S, true, true);
        let (undo, redo) = (chord(Key::Z, true, false), chord(Key::Z, true, true));
        if open_acc {
            self.state.push_action(UiAction::Open);
        }
        if save_acc {
            self.state.push_action(UiAction::Save);
        }
        if save_as_acc {
            self.state.push_action(UiAction::SaveAs);
        }
        // The canvas owns Esc while a two-click gesture / inline text editor is
        // active, and owns Delete/undo while typing (otherwise Esc would leave
        // the editor and Backspace would delete the selected annotation).
        let esc = esc && !crate::interaction::wants_escape();
        let del = del && !crate::interaction::text_editing();
        let undo = undo && !crate::interaction::text_editing();
        let redo = redo && !crate::interaction::text_editing();
        // While the inline annotation text editor is open every canvas shortcut
        // must stay out of the way (typing "e"/"v" would otherwise switch modes).
        if crate::interaction::text_editing() {
            return;
        }
        if left {
            self.step(-1, ctx);
        }
        if right {
            self.step(1, ctx);
        }
        if plus {
            let c = ctx.screen_rect().center();
            self.zoom_at(1.25, c, c);
        }
        if minus {
            let c = ctx.screen_rect().center();
            self.zoom_at(0.8, c, c);
        }
        if zero {
            self.state.view.fit = true;
        }
        if info {
            self.state.show_info = !self.state.show_info;
        }
        if shortcuts {
            self.state.show_shortcuts = !self.state.show_shortcuts;
        }
        if undo {
            self.state.undo();
        }
        if redo {
            self.state.redo();
        }
        if del {
            self.state.delete_selected();
        }
        // tool shortcuts (edit mode only): V B R O A L T C
        if self.state.phase == Phase::Edit {
            use crate::state::Tool;
            let tool_key = ctx.input(|i| {
                if i.key_pressed(Key::V) {
                    Some(Tool::Move)
                } else if i.key_pressed(Key::B) {
                    Some(Tool::Brush)
                } else if i.key_pressed(Key::R) {
                    Some(Tool::Rect)
                } else if i.key_pressed(Key::O) {
                    Some(Tool::Ellipse)
                } else if i.key_pressed(Key::A) {
                    Some(Tool::Arrow)
                } else if i.key_pressed(Key::L) {
                    Some(Tool::Line)
                } else if i.key_pressed(Key::T) {
                    Some(Tool::Text)
                } else if i.key_pressed(Key::C) {
                    Some(Tool::Crop)
                } else {
                    None
                }
            });
            if let Some(tool) = tool_key {
                self.state.set_tool(tool);
            }
        }
        if self.state.tool == crate::state::Tool::Text || edit {
            crate::fonts::ensure_annotation_font(ctx, self.state.locale);
        }
        if edit {
            self.state.phase = match self.state.phase {
                Phase::Preview => Phase::Edit,
                Phase::Edit => Phase::Preview,
            };
            if self.state.phase == Phase::Edit {
                self.state.set_tool(self.state.tool);
            }
        }
        if ctx.input(|i| i.key_pressed(Key::F3)) {
            self.perf = !self.perf;
        }
        if full {
            self.state.fullscreen = !self.state.fullscreen;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.state.fullscreen));
        }
        if esc {
            if self.state.show_shortcuts {
                self.state.show_shortcuts = false;
            } else if self.state.show_info {
                self.state.show_info = false;
            } else if self.state.phase == Phase::Edit {
                self.state.phase = Phase::Preview;
            } else if self.state.fullscreen {
                self.state.fullscreen = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
            }
        }
    }

    fn zoom_at(&mut self, factor: f32, anchor: Pos2, center: Pos2) {
        let old = self.state.view.scale;
        let mut next = (old * factor).clamp(0.02, 16.0);
        // In the viewer the minimum is the fitted view (same padded formula the
        // canvas uses); keyboard zoom used to allow going below it.
        if self.state.phase == Phase::Preview {
            if let Some((w, h)) = self.state.doc_size() {
                let viewport = Vec2::new(center.x, center.y) * 2.0;
                let floor = crate::interaction::preview_fit_scale(
                    Vec2::new(w as f32, h as f32),
                    viewport,
                );
                if next < floor {
                    next = floor;
                }
            }
        }
        let rel = anchor - center;
        self.state.view.offset = (self.state.view.offset - rel) * (next / old) + rel;
        self.state.view.scale = next;
        self.state.view.fit = false;
    }

    /// Ctrl+S / 儲存: flatten with the current settings and overwrite the file.
    fn save_overwrite(&mut self) {
        let Some(doc) = self.state.doc.as_ref() else {
            self.state.status("先開啟一張圖片", StatusKind::Info, now());
            return;
        };
        let fmt = self.state.output.format.resolve(doc);
        let flat = export::flatten(
            doc,
            &self.state.objects,
            self.state.crop,
            self.state.output.max_dim,
            self.state.output.scale,
        );
        let bytes = match self.state.output.target_kb {
            Some(kb) => export::encode_to_target(&flat.image, fmt, kb),
            None => export::encode(&flat.image, fmt, self.state.output.quality),
        };
        match save::overwrite(doc, &bytes) {
            Ok(()) => {
                self.state.dirty = false;
                self.state.status(format!("已儲存 · {} × {} px", flat.width, flat.height), StatusKind::Ok, now());
            }
            Err(err) => self.state.status(err, StatusKind::Err, now()),
        }
    }

    fn open_native_save(&mut self, ctx: &egui::Context) {
        let Some(doc) = self.state.doc.as_ref() else {
            self.state.status("先開啟一張圖片", StatusKind::Info, now());
            return;
        };
        let format = self.state.output.format.resolve(doc);
        let suggested = save::suggested_name(doc, format);
        let suggested_path = std::path::PathBuf::from(&suggested);
        let title = self.state.locale.tr("另存新檔");
        if let Some(dlg) = NativeDialog::save(ctx, &suggested_path, &title) {
            self.native = Some((dlg, NativeKind::Save(self.state.output.clone())));
        } else {
            self.open_save_dialog();
        }
    }

    fn open_save_dialog(&mut self) {
        let Some(doc) = self.state.doc.as_ref() else {
            self.state.status("先開啟一張圖片", StatusKind::Info, now());
            return;
        };
        let format = self.state.output.format.resolve(doc);
        let suggested = save::suggested_name(doc, format);
        let output = self.state.output.clone();
        self.dialog.open_with(&suggested, &output);
    }

    fn save_choice(&mut self, ctx: &egui::Context, choice: SaveChoice) {
        let Some(doc) = self.state.doc.as_ref() else { return };
        let flat = export::flatten(
            doc,
            &self.state.objects,
            self.state.crop,
            choice.output.max_dim,
            choice.output.scale,
        );
        // `Auto` must be resolved against the document before encoding (the
        // native picker hands us the raw settings, unlike the in-app dialog).
        let format = choice.output.format.resolve(doc);
        let bytes = match choice.output.target_kb {
            Some(kb) => export::encode_to_target(&flat.image, format, kb),
            None => export::encode(&flat.image, format, choice.output.quality),
        };
        let save_result = save::save_as(&choice.path, &bytes);
        if std::env::var_os("LUMEN_LOG").is_some() {
            eprintln!(
                "[save] path={} bytes={} result={:?} exists={} len={:?}",
                choice.path.display(),
                bytes.len(),
                save_result.as_ref().map(|p| p.display().to_string()),
                choice.path.exists(),
                std::fs::metadata(&choice.path).ok().map(|m| m.len())
            );
        }
        match save_result {
            Ok(path) => {
                let size = bytes.len();
                self.state.output = choice.output;
                self.open(&path, Some(ctx));
                self.state.dirty = false;
                self.state.status(
                    format!("已儲存 · {} × {} px · {:.1} KB", flat.width, flat.height, size as f64 / 1024.0),
                    StatusKind::Ok,
                    now(),
                );
            }
            Err(err) => self.state.status(err, StatusKind::Err, now()),
        }
    }

    /// Recompute the live size estimate (throttled; the decode cache makes it cheap).
    fn refresh_estimate(&mut self, ctx: Option<&egui::Context>) {
        // Never spend decode/encode time while the user is dragging.
        if let Some(ctx) = ctx {
            if ctx.input(|i| i.pointer.any_down()) {
                return;
            }
        }
        let now_ms = now() * 1000.0;
        let Some(doc) = self.state.doc.as_ref() else { return };
        let key = format!(
            "{:?}|{}|{:?}|{:?}|{:?}|{:?}|{}",
            self.state.output.format,
            self.state.output.quality,
            self.state.output.max_dim,
            self.state.output.scale,
            self.state.output.target_kb,
            self.state.crop.map(|c| (c.x.round() as i64, c.y.round() as i64, c.w.round() as i64, c.h.round() as i64, c.ratio.map(|r| (r * 1000.0) as i64))),
            self.state.objects.len()
        );
        // Recompute only when the settings actually changed, and debounce bursts
        // (this used to fall through every frame after the window elapsed, which
        // burned a full core while editing).
        if key == self.estimate_key {
            return;
        }
        if now_ms - self.estimate_at < 400.0 {
            return;
        }
        self.estimate_key = key;
        self.estimate_at = now_ms;
        self.state.estimate =
            Some(export::estimate_size(doc, &self.state.objects, &self.state.output, self.state.crop));
    }

    fn drain_actions(&mut self, ctx: &egui::Context) {
        for action in self.state.take_actions() {
            match action {
                UiAction::Prev => self.step(-1, ctx),
                UiAction::Next => self.step(1, ctx),
                UiAction::CloseWindow => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                UiAction::ApplyCrop => {
                    self.state.crop_committed = true;
                }
                UiAction::ResetCrop => {
                    self.state.crop = None;
                    self.state.crop_committed = false;
                }
                UiAction::Save => self.save_overwrite(),
                UiAction::Estimate => self.refresh_estimate(None),
                UiAction::Open => {
                    crate::fonts::ensure_system(ctx, self.state.locale);
                    let start = self
                        .state
                        .doc
                        .as_ref()
                        .and_then(|d| d.path.parent().map(|p| p.to_path_buf()));
                    let title = self.state.locale.tr("開啟圖片");
                    if use_internal_browser() {
                        let start = self.state.doc.as_ref().map(|d| d.path.clone());
                        self.browser.open_at(start);
                    } else if let Some(dlg) =
                        NativeDialog::open(ctx, start.as_deref(), &title)
                    {
                        self.native = Some((dlg, NativeKind::Open));
                    } else {
                        // No zenity/kdialog: keep the in-app browser.
                        let start = self.state.doc.as_ref().map(|d| d.path.clone());
                        self.browser.open_at(start);
                    }
                }
                UiAction::SaveAs | UiAction::Export => {
                    crate::fonts::ensure_system(ctx, self.state.locale);
                    if use_internal_browser() {
                        self.open_save_dialog();
                    } else {
                        self.open_native_save(ctx);
                    }
                }
            }
        }
    }

    fn canvas(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::CANVAS))
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                let painter = ui.painter_at(rect);
                let Some(doc) = self.state.doc.as_mut() else {
                    painter.text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        self.state.locale.tr("尚未載入圖片"),
                        egui::FontId::proportional(14.0),
                        theme::DIM,
                    );
                    return;
                };
                let (w, h) = (doc.width as f32, doc.height as f32);
                let stage = rect.shrink2(Vec2::new(32.0, 32.0));
                let fit_scale = (stage.width() / w).min(stage.height() / h).min(1.0);
                if self.phase_pending {
                    self.phase_pending = false;
                    // Predict the final canvas rect for the destination phase; the
                    // panels animate their widths, so we animate the rect too and
                    // drive image scale/offset with the same parameter.
                    let screen = ctx.screen_rect();
                    let (collapsed, insp_w) = crate::ui::local::get(ctx, |l| {
                        (l.inspector_collapsed, l.inspector_width)
                    });
                    let to_rect = match self.state.phase {
                        Phase::Preview => screen,
                        Phase::Edit => egui::Rect::from_min_max(
                            egui::pos2(screen.left() + theme::RAIL_W, screen.top() + theme::TOPBAR_H),
                            egui::pos2(
                                screen.right() - if collapsed { 0.0 } else { insp_w },
                                screen.bottom() - theme::STATUS_H,
                            ),
                        ),
                    };
                    let to_stage = to_rect.shrink2(Vec2::new(32.0, 32.0));
                    let to_scale = (to_stage.width() / w).min(to_stage.height() / h).min(1.0);
                    // Keep the glide in sync with slowed-motion debugging and with
                    // the panel animation (which honours LUMEN_MOTION_SLOW too).
                    let slow = std::env::var("LUMEN_MOTION_SLOW")
                        .ok()
                        .and_then(|v| v.parse::<f64>().ok())
                        .map(|v| v.clamp(1.0, 10.0))
                        .unwrap_or(1.0);
                    self.phase_anim = Some(PhaseAnim {
                        t0: now() - (PHASE_GLIDE * slow - PHASE_GLIDE),
                        from_rect: rect,
                        to_rect,
                        from_scale: self.state.view.scale,
                        from_off: self.state.view.offset,
                        to_scale,
                    });
                }
                if let Some(anim) = self.phase_anim {
                    let p = (((now() - anim.t0) / PHASE_GLIDE) as f32).clamp(0.0, 1.0);
                    let e = 1.0 - (1.0 - p).powi(3);
                    let anim_rect = lerp_rect(anim.from_rect, anim.to_rect, e);
                    let scale = anim.from_scale + (anim.to_scale - anim.from_scale) * e;
                    let offset = anim.from_off * (1.0 - e);
                    let tex = doc.texture_for(ctx, scale).0;
                    let center = anim_rect.center() + offset;
                    painter.image(
                        tex.id(),
                        egui::Rect::from_center_size(center, Vec2::new(w * scale, h * scale)),
                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                    if p < 1.0 {
                        ctx.request_repaint();
                    } else {
                        self.phase_anim = None;
                        self.state.view.fit = true;
                        self.state.view.scale = anim.to_scale;
                        self.state.view.offset = Vec2::ZERO;
                    }
                    return;
                }
                if self.state.view.fit {
                    // Web parity: the fit view keeps 32 px of breathing room on
                    // every side (64 px total per axis) before fitting.
                    self.state.view.scale = fit_scale;
                    self.state.view.offset = Vec2::ZERO;
                }
                let scale = self.state.view.scale;
                let tex = doc.texture_for(ctx, scale).0;
                // Image switch: slide in from the arrow direction and fade in.
                let mut switch_shift = 0.0;
                let mut switch_alpha = 1.0;
                if let Some((dir, t0)) = self.switch {
                    let p = (((now() - t0) / 0.24) as f32).clamp(0.0, 1.0);
                    let e = 1.0 - (1.0 - p).powi(3);
                    switch_shift = (1.0 - e) * dir * 70.0;
                    switch_alpha = e;
                    if p < 1.0 {
                        ctx.request_repaint();
                    } else {
                        self.switch = None;
                    }
                }
                let center = rect.center() + self.state.view.offset;
                let img_center = Vec2::new(w / 2.0, h / 2.0);
                let draw_rect = Rect::from_center_size(
                    center + Vec2::new(switch_shift, 0.0),
                    Vec2::new(w * scale, h * scale),
                );
                let tint = egui::Color32::WHITE.gamma_multiply(switch_alpha);
                painter.image(
                    tex.id(),
                    draw_rect,
                    Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    tint,
                );

                let to_screen = move |p: Pos2| center + (p.to_vec2() - img_center) * scale;
                let to_image = move |p: Pos2| (img_center + (p.to_vec2() - center.to_vec2()) / scale).to_pos2();

                if !self.state.compare {
                    for obj in &self.state.objects {
                        obj.draw(&painter, &to_screen, scale);
                    }
                }
                if self.state.tool == crate::state::Tool::Crop {
                    if let Some(crop) = self.state.crop.as_ref() {
                        crop.draw(&painter, &to_screen, scale);
                    }
                }

                let view = CanvasView {
                    rect,
                    to_screen: Box::new(to_screen),
                    to_image: Box::new(to_image),
                    zoom: scale,
                };
                interaction::handle_canvas(ui, &mut self.state, &view);
            });
    }
}

impl eframe::App for LumenApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        Self::apply_theme(ctx);
        if !self.focused_once {
            self.focused_once = true;
            crate::single_instance::attach_context(ctx.clone());
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        // Drag & drop parity with the web build: drop a file anywhere to open it.
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        for path in dropped {
            if path.is_dir() {
                if let Some(first) = crate::doc::siblings(&path.join("first.jpg")).into_iter().find(|p| p.is_file()) {
                    self.open(&first, Some(ctx));
                } else {
                    self.browser.open_at(Some(path));
                }
            } else {
                self.open(&path, Some(ctx));
            }
        }
        let modal_open = self.browser.open || self.dialog.open;
        if !modal_open {
            self.handle_keys(ctx);
        }
        ui::topbar::show(ctx, &mut self.state);
        ui::rail::show(ctx, &mut self.state);
        ui::inspector::show(ctx, &mut self.state);
        if !(self.browser.open || self.dialog.open) {
            ui::dock::show(ctx, &mut self.state);
        }
        ui::status::show(ctx, &mut self.state);
        ui::info::show(ctx, &mut self.state);
        ui::shortcuts::show(ctx, &mut self.state);
        self.canvas(ctx);
        if let Some(doc) = self.state.doc.as_mut() {
            if doc.poll_full(ctx) {
                ctx.request_repaint_after(std::time::Duration::from_millis(40));
            }
        }
        self.drain_actions(ctx);
        if let Some(rx) = self.incoming.take() {
            while let Ok(path) = rx.try_recv() {
                self.open(&path, Some(ctx));
            }
            self.incoming = Some(rx);
        }
        if let Some((dlg, _)) = self.native.as_mut() {
            if let Some(result) = dlg.poll() {
                let (_, kind) = self.native.take().expect("native dialog state");
                match (kind, result) {
                    (NativeKind::Open, Outcome::Picked(path)) => self.open(&path, Some(ctx)),
                    (NativeKind::Save(output), Outcome::Picked(path)) => {
                        self.save_choice(ctx, SaveChoice { path, output })
                    }
                    // No portal/session or the tool failed: fall back so the app
                    // stays usable (this is also the path under Xvfb).
                    (NativeKind::Open, Outcome::Failed) => {
                        let start = self.state.doc.as_ref().map(|d| d.path.clone());
                        self.browser.open_at(start);
                    }
                    (NativeKind::Save(_), Outcome::Failed) => self.open_save_dialog(),
                    (_, Outcome::Cancelled) => {}
                }
            }
        }
        if self.browser.open {
            let locale = self.state.locale;
            self.log_event("browser_open");
            if let Some(path) = self.browser.show(ctx, locale) {
                self.open(&path, Some(ctx));
            }
        }
        if self.dialog.open {
            let snap = self.dialog.settings_snapshot();
            if Some(snap) != self.dialog_snap {
                self.dialog_snap = Some(snap);
                self.state.estimate = self.state.doc.as_ref().map(|doc| {
                    export::estimate_size(doc, &self.state.objects, &self.dialog.output, self.state.crop)
                });
            }
            if let Some(choice) = self.dialog.show(ctx, &self.state, self.state.estimate.as_ref()) {
                self.save_choice(ctx, choice);
            }
        } else {
            self.dialog_snap = None;
        }
        self.log_event("frame");
        if std::env::var_os("LUMEN_DEBUG_ZOOM").is_some() {
            let s = self.state.view.scale;
            if (s - self.debug_last_zoom).abs() > 1e-4 {
                self.debug_last_zoom = s;
                eprintln!("[zoom] {:.4}", s);
            }
        }
        if self.state.tool == crate::state::Tool::Text
            || self
                .state
                .doc
                .as_ref()
                .map(|d| crate::fonts::needs_system(&d.name))
                .unwrap_or(false)
        {
            crate::fonts::ensure_system(ctx, self.state.locale);
        }
        if self.state.phase == Phase::Edit && self.state.doc.is_some() {
            self.refresh_estimate(Some(ctx));
        }

        // Phase transitions: remember the start view, then glide to the new fit.
        if self.state.phase != self.last_phase {
            self.last_phase = self.state.phase;
            self.phase_pending = true;
            ctx.request_repaint();
        }
        if self.perf {
            let dt = ctx.input(|i| i.unstable_dt) * 1000.0;
            self.frame_ms[self.frame_idx % 30] = dt;
            self.frame_idx += 1;
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
            let n = self.frame_idx.min(30) as f32;
            let avg: f32 = self.frame_ms.iter().take(n as usize).sum::<f32>() / n;
            let fps = if avg > 0.0 { 1000.0 / avg } else { 0.0 };
            let text = format!("{:.0} fps · {:.1} ms/frame", fps, avg);
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("lumen-perf"),
            ));
            let pos = ctx.screen_rect().right_top() + Vec2::new(-190.0, 56.0);
            painter.text(
                pos,
                egui::Align2::LEFT_TOP,
                text,
                egui::FontId::monospace(12.0),
                theme::TEXT,
            );
        }
    }
}

fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn lerp_rect(a: egui::Rect, b: egui::Rect, t: f32) -> egui::Rect {
    egui::Rect::from_min_max(
        a.min + (b.min - a.min) * t,
        a.max + (b.max - a.max) * t,
    )
}

fn max_pixels(ctx: Option<&egui::Context>) -> u64 {
    // 2x the viewport keeps fit-view sharp without wasting RAM on huge decodes;
    // zooming past `decoded_scale` triggers the async full-res upgrade anyway.
    let (w, h) = ctx
        .map(|c| {
            let r = c.screen_rect();
            (r.width().max(640.0), r.height().max(480.0))
        })
        .unwrap_or((1280.0, 820.0));
    ((w * h * 1.5) as u64).max(2_000_000)
}

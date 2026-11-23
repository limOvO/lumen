// Experimental wgpu + egui preview shell (no WebView, no GTK).
use std::path::{Path, PathBuf};

use eframe::egui;

fn image_ext(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()).map(|s| s.to_ascii_lowercase()).as_deref(),
        Some("jpg") | Some("jpeg") | Some("png") | Some("webp") | Some("gif") | Some("bmp")
            | Some("tif") | Some("tiff") | Some("avif") | Some("svg") | Some("ico")
    )
}

fn siblings(path: &Path) -> Vec<PathBuf> {
    let dir = match path.parent() { Some(d) => d, None => return vec![path.into()] };
    let mut list: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_file() && image_ext(p)).collect())
        .unwrap_or_default();
    list.sort();
    if list.is_empty() { list.push(path.into()); }
    list
}

struct App {
    path: Option<PathBuf>,
    texture: Option<egui::TextureHandle>,
    size: (u32, u32),
    zoom: f32,
    fit: bool,
    info: String,
}

impl App {
    fn load(&mut self, ctx: &egui::Context, path: PathBuf) {
        match image::open(&path) {
            Ok(img) => {
                let rgba = img.to_rgba8();
                let (w, h) = rgba.dimensions();
                let pixels = rgba.into_raw();
                let color = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &pixels);
                self.texture = Some(ctx.load_texture("image", color, egui::TextureOptions::LINEAR));
                self.size = (w, h);
                self.fit = true;
                let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?").to_string();
                self.info = format!("{name}   {w} × {h}   {:.1} KB", bytes as f64 / 1024.0);
                ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!("{name} — Lumen")));
                self.path = Some(path);
            }
            Err(err) => eprintln!("cannot open {}: {err}", path.display()),
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut picked: Option<PathBuf> = None;
        let mut step: i32 = 0;
        ctx.input(|i| {
            if i.key_pressed(egui::Key::ArrowLeft) { step = -1; }
            if i.key_pressed(egui::Key::ArrowRight) { step = 1; }
            if i.key_pressed(egui::Key::Escape) { ctx.send_viewport_cmd(egui::ViewportCommand::Close); }
            if i.key_pressed(egui::Key::Num0) { self.fit = true; }
            if i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals) {
                self.zoom = (self.zoom * 1.25).clamp(0.02, 16.0);
                self.fit = false;
            }
            if i.key_pressed(egui::Key::Minus) {
                self.zoom = (self.zoom * 0.8).clamp(0.02, 16.0);
                self.fit = false;
            }
            if i.zoom_delta() != 1.0 { self.zoom = (self.zoom * i.zoom_delta()).clamp(0.02, 16.0); self.fit = false; }
        });
        if step != 0 {
            if let Some(cur) = self.path.clone() {
                let list = siblings(&cur);
                if let Some(pos) = list.iter().position(|p| p == &cur) {
                    let n = list.len() as i32;
                    let next = &list[((pos as i32 + step + n) % n) as usize];
                    picked = Some(next.clone());
                }
            }
        }
        if let Some(p) = picked { self.load(ctx, p); }

        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(egui::Color32::from_rgb(16, 16, 18)))
            .show(ctx, |ui| {
                if let Some(tex) = &self.texture {
                    let avail = ui.available_size();
                    let (iw, ih) = (self.size.0 as f32, self.size.1 as f32);
                    if self.fit {
                        self.zoom = (avail.x / iw).min(avail.y / ih).min(1.0);
                    }
                    let draw = egui::vec2(iw * self.zoom, ih * self.zoom);
                    let rect = egui::Rect::from_center_size(ui.max_rect().center(), draw);
                    ui.painter().image(tex.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                }
                ui.allocate_ui_at_rect(egui::Rect::from_min_size(ui.max_rect().left_bottom() + egui::vec2(10.0, -24.0), egui::vec2(900.0, 20.0)), |ui| {
                    ui.label(&self.info);
                });
            });
    }
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let start = args.get(1).cloned();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 820.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Lumen",
        options,
        Box::new(move |cc| {
            let mut app = App { path: None, texture: None, size: (1, 1), zoom: 1.0, fit: true, info: String::new() };
            if let Some(p) = start { app.load(&cc.egui_ctx, PathBuf::from(p)); }
            Ok(Box::new(app))
        }),
    )
}

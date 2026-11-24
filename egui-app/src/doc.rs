// Document loading: preview-sized decode, EXIF, on-demand full resolution.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

use egui::{ColorImage, TextureHandle, TextureOptions};
use image::imageops::FilterType;

pub fn image_ext(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()).map(|s| s.to_ascii_lowercase()).as_deref(),
        Some("jpg") | Some("jpeg") | Some("png") | Some("webp") | Some("gif") | Some("bmp")
            | Some("tif") | Some("tiff") | Some("avif") | Some("svg") | Some("ico")
    )
}

pub fn siblings(path: &Path) -> Vec<PathBuf> {
    let Some(dir) = path.parent() else { return vec![path.to_path_buf()] };
    let mut list: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_file() && image_ext(p)).collect())
        .unwrap_or_default();
    list.sort();
    if list.is_empty() {
        list.push(path.to_path_buf());
    }
    list
}

pub struct Doc {
    pub path: PathBuf,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
    pub format: String,
    pub mtime: String,
    pub exif: Vec<(String, String)>,
    // CPU-side pixels are dropped once uploaded: the GPU texture is the source
    // of truth for drawing, which keeps resident memory low.
    preview: Option<Arc<ColorImage>>,
    decoded_scale: f32,
    preview_tex: Option<TextureHandle>,
    full_tex: Option<TextureHandle>,
    full_rx: Option<Receiver<Arc<ColorImage>>>,
}


// ---------------------------------------------------------------------------
// Preview cache: decoded previews keyed by (path, len, mtime, cap). The app
// prefetches the neighbouring images in the background so ←/→ switching is a
// texture upload instead of a decode.
// ---------------------------------------------------------------------------

struct CachedPreview {
    key: (std::path::PathBuf, u64, u128, u64),
    image: Arc<ColorImage>,
    width: u32,
    height: u32,
    scale: f32,
}

static PREVIEW_CACHE: std::sync::OnceLock<std::sync::Mutex<std::collections::VecDeque<CachedPreview>>> =
    std::sync::OnceLock::new();

const PREVIEW_CACHE_CAP: usize = 2;
/// Total decoded-preview bytes kept in RAM (current doc texture is separate).
const PREVIEW_CACHE_BYTES: usize = 24 * 1024 * 1024;

fn cache() -> &'static std::sync::Mutex<std::collections::VecDeque<CachedPreview>> {
    PREVIEW_CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::VecDeque::new()))
}

fn file_stamp(path: &Path) -> (u64, u128) {
    std::fs::metadata(path)
        .map(|m| {
            let len = m.len();
            let mtime = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            (len, mtime)
        })
        .unwrap_or((0, 0))
}

fn cache_lookup(
    path: &Path,
    len: u64,
    mtime: u128,
    cap: u64,
) -> Option<(Arc<ColorImage>, u32, u32, f32)> {
    let key = (path.to_path_buf(), len, mtime, cap);
    let mut guard = cache().lock().ok()?;
    let pos = guard.iter().position(|e| e.key == key)?;
    let entry = guard.remove(pos)?;
    let out = (
        entry.image.clone(),
        entry.width,
        entry.height,
        entry.scale,
    );
    guard.push_back(entry);
    Some(out)
}

fn cache_store(
    path: &Path,
    len: u64,
    mtime: u128,
    cap: u64,
    image: Arc<ColorImage>,
    width: u32,
    height: u32,
    scale: f32,
) {
    if let Ok(mut guard) = cache().lock() {
        let key = (path.to_path_buf(), len, mtime, cap);
        guard.push_back(CachedPreview {
            key,
            image,
            width,
            height,
            scale,
        });
        while guard.len() > PREVIEW_CACHE_CAP {
            guard.pop_front();
        }
        while guard.iter().map(|e| e.image.size[0] * e.image.size[1] * 4).sum::<usize>()
            > PREVIEW_CACHE_BYTES
            && guard.len() > 1
        {
            guard.pop_front();
        }
    }
}

/// Decode bytes into a preview capped at `cap` pixels. JPEGs go through
/// libjpeg-turbo's scaled IDCT (much faster than a full decode + resize).
fn build_preview(
    path: &Path,
    bytes: &[u8],
    fmt: image::ImageFormat,
    cap: u64,
) -> Result<(Arc<ColorImage>, u32, u32, f32), String> {
    if fmt == image::ImageFormat::Jpeg {
        if let Some((rgba, pw, ph, fw, fh, scale)) = crate::jpeg_fast::decode_scaled(bytes, cap) {
            let image = Arc::new(ColorImage::from_rgba_unmultiplied(
                [pw as usize, ph as usize],
                &rgba,
            ));
            return Ok((image, fw, fh, scale));
        }
    }
    let img = image::load_from_memory(bytes).map_err(|e| format!("圖片解碼失敗：{e}"))?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let area = (w as u64) * (h as u64);
    let (pw, ph, scale) = if area > cap.max(1) {
        let ratio = (cap as f64 / area as f64).sqrt();
        let pw = ((w as f64 * ratio).round() as u32).max(1);
        let ph = ((h as f64 * ratio).round() as u32).max(1);
        (pw, ph, pw as f32 / w as f32)
    } else {
        (w, h, 1.0)
    };
    let preview_rgba = if (pw, ph) == (w, h) {
        rgba
    } else {
        image::imageops::thumbnail(&rgba, pw, ph)
    };
    let image = Arc::new(ColorImage::from_rgba_unmultiplied(
        [pw as usize, ph as usize],
        preview_rgba.as_raw(),
    ));
    drop(preview_rgba);
    let _ = path;
    Ok((image, w, h, scale))
}

/// Warm the preview cache for a neighbouring image (called on a worker thread).
pub fn preload_preview(path: std::path::PathBuf, cap: u64) {
    let (len, mtime) = file_stamp(&path);
    if cache_lookup(&path, len, mtime, cap).is_some() {
        return;
    }
    let Ok(bytes) = std::fs::read(&path) else { return };
    let Ok(fmt) = image::guess_format(&bytes) else { return };
    if let Ok((image, w, h, scale)) = build_preview(&path, &bytes, fmt, cap) {
        cache_store(&path, len, mtime, cap, image, w, h, scale);
    }
}

pub struct LoadOptions {
    /// Preview decode budget: max pixels decoded for the screen.
    pub max_pixels: u64,
}

fn format_name(_path: &Path, fmt: image::ImageFormat) -> String {
    match fmt {
        image::ImageFormat::Jpeg => "JPEG".into(),
        image::ImageFormat::Png => "PNG".into(),
        image::ImageFormat::WebP => "WEBP".into(),
        image::ImageFormat::Gif => "GIF".into(),
        image::ImageFormat::Bmp => "BMP".into(),
        image::ImageFormat::Tiff => "TIFF".into(),
        image::ImageFormat::Avif => "AVIF".into(),
        other => format!("{other:?}").to_uppercase(),
    }
}

fn read_exif(bytes: &[u8]) -> Vec<(String, String)> {
    use exif::{In, Tag, Value};
    let mut out = Vec::new();
    let Ok(exif) = exif::Reader::new().read_from_container(&mut std::io::Cursor::new(bytes)) else {
        return out;
    };
    let rows: &[(Tag, &str)] = &[
        (Tag::Make, "相機"),
        (Tag::Model, "機型"),
        (Tag::LensModel, "鏡頭"),
        (Tag::FNumber, "光圈"),
        (Tag::ExposureTime, "快門"),
        (Tag::PhotographicSensitivity, "ISO"),
        (Tag::FocalLength, "焦距"),
        (Tag::DateTimeOriginal, "拍攝時間"),
    ];
    for (tag, label) in rows {
        if let Some(field) = exif.get_field(*tag, In::PRIMARY) {
            let text = match field.value {
                Value::Rational(ref v) if v.len() == 1 => {
                    format!("{}/{}", v[0].num, v[0].denom.max(1))
                }
                _ => field.display_value().to_string(),
            };
            if !text.is_empty() {
                out.push((label.to_string(), text));
            }
        }
    }
    out
}

fn mtime_string(path: &Path) -> String {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| {
            // small civil-date conversion (UTC) — good enough for the info panel
            let secs = d.as_secs() as i64;
            let days = secs.div_euclid(86_400);
            let rem = secs.rem_euclid(86_400);
            let (y, m, day) = civil_from_days(days);
            format!(
                "{y:04}-{m:02}-{day:02} {:02}:{:02}",
                rem / 3600,
                (rem % 3600) / 60
            )
        })
        .unwrap_or_default()
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

impl Doc {
    pub fn load(path: &Path, opts: LoadOptions) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("讀取檔案失敗：{e}"))?;
        let fmt = image::guess_format(&bytes).map_err(|e| format!("無法辨識圖片格式：{e}"))?;
        let (len, stamp) = file_stamp(path);
        let (preview, w, h, decoded_scale) =
            match cache_lookup(path, len, stamp, opts.max_pixels) {
                Some(hit) => hit,
                None => {
                    let decoded = build_preview(path, &bytes, fmt, opts.max_pixels)?;
                    cache_store(
                        path,
                        len,
                        stamp,
                        opts.max_pixels,
                        decoded.0.clone(),
                        decoded.1,
                        decoded.2,
                        decoded.3,
                    );
                    decoded
                }
            };

        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?").to_string();
        Ok(Self {
            path: path.to_path_buf(),
            name,
            width: w,
            height: h,
            bytes: len,
            format: format_name(path, fmt),
            mtime: mtime_string(path),
            exif: read_exif(&bytes),
            preview: Some(preview),
            decoded_scale,
            preview_tex: None,
            full_tex: None,
            full_rx: None,
        })
    }

    pub fn preview_texture(&mut self, ctx: &egui::Context) -> TextureHandle {
        if self.preview_tex.is_none() {
            if let Some(image) = self.preview.take() {
                let tex = ctx.load_texture("preview", (*image).clone(), TextureOptions::LINEAR);
                self.preview_tex = Some(tex);
                drop(image);
            }
        }
        self.preview_tex.clone().expect("preview texture")
    }

    pub fn decoded_scale(&self) -> f32 {
        self.decoded_scale
    }

    /// Full-resolution pixels are only needed when the view zooms past the
    /// preview decode (or when exporting).
    pub fn needs_full(&self, scale: f32) -> bool {
        self.decoded_scale < 0.999 && scale > self.decoded_scale as f32 * 0.98
    }

    /// Kick off a background decode of the full-resolution pixels (once).
    pub fn request_full(&mut self) {
        if self.full_tex.is_some() || self.full_rx.is_some() {
            return;
        }
        let (tx, rx) = channel();
        let path = self.path.clone();
        std::thread::spawn(move || {
            let Ok(bytes) = std::fs::read(&path) else { return };
            let Ok(img) = image::load_from_memory(&bytes) else { return };
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            let image = Arc::new(ColorImage::from_rgba_unmultiplied(
                [w as usize, h as usize],
                rgba.as_raw(),
            ));
            let _ = tx.send(image);
        });
        self.full_rx = Some(rx);
    }

    /// Upload the worker's result when it arrives. Returns true while pending.
    pub fn poll_full(&mut self, ctx: &egui::Context) -> bool {
        let Some(rx) = self.full_rx.as_ref() else { return false };
        match rx.try_recv() {
            Ok(image) => {
                self.full_tex =
                    Some(ctx.load_texture("full", (*image).clone(), TextureOptions::LINEAR));
                self.full_rx = None;
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.full_rx = None;
                false
            }
        }
    }

    /// Preview texture, upgraded to full resolution once the worker finishes.
    pub fn texture_for(&mut self, ctx: &egui::Context, scale: f32) -> (TextureHandle, f32) {
        if self.needs_full(scale) {
            if self.full_tex.is_none() {
                self.request_full();
            }
            if let Some(tex) = self.full_tex.clone() {
                return (tex, 1.0);
            }
        }
        (self.preview_texture(ctx), self.decoded_scale)
    }
}

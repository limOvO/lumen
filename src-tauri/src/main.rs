// Lumen — native shell for a lightweight image viewer / annotator.
// The frontend does all pixel work (decode, canvas, encode). Rust only
// handles filesystem enumeration, metadata/EXIF, and window plumbing so
// the whole app stays a thin, fast native shell around WebKit.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Serialize;
use tauri::ipc::Response;
use tauri::{AppHandle, Emitter, Manager};

/// Event the frontend listens on. Payload is a plain absolute path string.
const OPEN_EVENT: &str = "lumen://open";
/// Event name is also used as the app name in CLI text.
const APP_NAME: &str = "Lumen";

const IMAGE_EXTS: &[&str] = &[
    "jpg", "jpeg", "jpe", "jfif", "png", "webp", "gif", "bmp", "avif", "svg", "ico", "tif", "tiff",
];

#[derive(Serialize)]
struct DirEntry {
    path: String,
    name: String,
    size: u64,
    mtime: u64,
}

#[derive(Serialize)]
struct ExifField {
    key: String,
    label: String,
    value: String,
}

#[derive(Serialize)]
struct ImageMeta {
    path: String,
    name: String,
    dir: String,
    size: u64,
    mtime: u64,
    width: Option<u32>,
    height: Option<u32>,
    format: String,
    /// Human readable EXIF rows, already sliced/annotated by the shell.
    exif: Vec<ExifField>,
}

fn ext_lower(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default()
}

fn is_image(path: &Path) -> bool {
    IMAGE_EXTS.contains(&ext_lower(path).as_str())
}

fn file_size(path: &Path) -> u64 {
    fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn mtime_secs(path: &Path) -> u64 {
    fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn format_name(path: &Path) -> String {
    let ext = ext_lower(path);
    match ext.as_str() {
        "jpg" | "jpeg" | "jpe" | "jfif" => "JPEG".into(),
        "png" => "PNG".into(),
        "webp" => "WebP".into(),
        "gif" => "GIF".into(),
        "bmp" => "BMP".into(),
        "avif" => "AVIF".into(),
        "svg" => "SVG".into(),
        "ico" => "ICO".into(),
        "tif" | "tiff" => "TIFF".into(),
        _ => ext.to_ascii_uppercase(),
    }
}

/// Width/height without a full decode. Falls back to EXIF pixel dimensions for
/// formats `imagesize` cannot parse, then to 0 (frontend uses decoded bitmap).
fn dimensions(path: &Path) -> (Option<u32>, Option<u32>) {
    if let Ok(sz) = imagesize::size(path) {
        return (Some(sz.width as u32), Some(sz.height as u32));
    }
    (None, None)
}

fn fmt_rational(v: &exif::Value) -> Option<String> {
    if let exif::Value::Rational(ref r) = v {
        if let Some(first) = r.first() {
            if first.denom != 0 {
                let f = first.num as f64 / first.denom as f64;
                if f >= 10.0 {
                    return Some(format!("{:.0}", f));
                }
                return Some(format!("{:.1}", f));
            }
        }
    }
    None
}


/// Pull a number out of whatever type the camera wrote (RATIONAL, DOUBLE,
/// FLOAT, SHORT...). Cameras are wildly inconsistent here.
fn numeric(v: &exif::Value) -> Option<f64> {
    fn rat(num: i64, den: i64) -> f64 {
        if den == 0 {
            0.0
        } else {
            num as f64 / den as f64
        }
    }
    match v {
        exif::Value::Rational(r) => r.first().map(|x| rat(x.num as i64, x.denom as i64)),
        exif::Value::SRational(r) => r.first().map(|x| rat(x.num as i64, x.denom as i64)),
        exif::Value::Double(d) => d.first().copied(),
        exif::Value::Float(f) => f.first().map(|x| *x as f64),
        exif::Value::Short(s) => s.first().map(|x| *x as f64),
        exif::Value::Long(l) => l.first().map(|x| *x as f64),
        _ => None,
    }
}

/// Exposure time as photographers write it: 1/250s, 2.5s.
fn fmt_exposure(v: &exif::Value) -> Option<String> {
    let secs = numeric(v)?;
    if secs <= 0.0 {
        return None;
    }
    if secs >= 1.0 {
        Some(format!("{:.1}s", secs))
    } else {
        Some(format!("1/{}s", (1.0 / secs).round()))
    }
}

fn fmt_fnumber(v: &exif::Value) -> Option<String> {
    let n = numeric(v)?;
    Some(format!("f/{}", trim_num(n)))
}

fn fmt_focal(v: &exif::Value) -> Option<String> {
    let n = numeric(v)?;
    Some(format!("{}mm", trim_num(n)))
}

fn trim_num(n: f64) -> String {
    if (n - n.round()).abs() < 0.05 {
        format!("{:.0}", n)
    } else {
        format!("{:.1}", n)
    }
}

fn read_exif(path: &Path) -> Vec<ExifField> {
    let mut out: Vec<ExifField> = Vec::new();
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return out,
    };
    let mut buf = std::io::BufReader::new(file);
    let exif = match exif::Reader::new().read_from_container(&mut buf) {
        Ok(e) => e,
        Err(_) => return out,
    };

    let get = |tag: exif::Tag| -> Option<String> {
        exif.get_field(tag, exif::In::PRIMARY)
            .map(|f| f.display_value().with_unit(&exif).to_string())
            .map(|s| s.trim_matches('"').to_string())
            .filter(|s| !s.is_empty())
    };

    let push = |out: &mut Vec<ExifField>, key: &str, label: &str, val: Option<String>| {
        if let Some(v) = val {
            out.push(ExifField {
                key: key.into(),
                label: label.into(),
                value: v,
            });
        }
    };

    push(&mut out, "camera", "相機", {
        let make = get(exif::Tag::Make);
        let model = get(exif::Tag::Model);
        match (make, model) {
            (Some(a), Some(b)) => {
                if b.starts_with(a.split(' ').next().unwrap_or("")) {
                    Some(b)
                } else {
                    Some(format!("{} {}", a, b))
                }
            }
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        }
    });
    push(&mut out, "lens", "鏡頭", get(exif::Tag::LensModel).or_else(|| get(exif::Tag::LensMake)));
    push(&mut out, "datetime", "拍攝時間", get(exif::Tag::DateTimeOriginal).or_else(|| get(exif::Tag::DateTime)));
    push(&mut out, "exposure", "快門", {
        exif.get_field(exif::Tag::ExposureTime, exif::In::PRIMARY)
            .and_then(|f| fmt_exposure(&f.value))
            .or_else(|| {
                exif.get_field(exif::Tag::ShutterSpeedValue, exif::In::PRIMARY)
                    .and_then(|f| {
                        let apex = numeric(&f.value)?;
                        let secs = 2f64.powf(-apex);
                        fmt_exposure(&exif::Value::Double(vec![secs]))
                    })
            })
    });
    push(&mut out, "fnumber", "光圈", {
        exif.get_field(exif::Tag::FNumber, exif::In::PRIMARY)
            .and_then(|f| fmt_fnumber(&f.value))
            .or_else(|| {
                exif.get_field(exif::Tag::ApertureValue, exif::In::PRIMARY)
                    .and_then(|f| {
                        let apex = numeric(&f.value)?;
                        // APEX aperture -> f-number
                        let n = 2f64.powf(apex / 2.0);
                        fmt_fnumber(&exif::Value::Double(vec![n]))
                    })
            })
    });
    push(&mut out, "iso", "ISO", {
        exif.get_field(exif::Tag::PhotographicSensitivity, exif::In::PRIMARY)
            .or_else(|| exif.get_field(exif::Tag::ISOSpeed, exif::In::PRIMARY))
            .map(|f| f.display_value().to_string())
    });
    push(&mut out, "focal", "焦距", {
        exif.get_field(exif::Tag::FocalLength, exif::In::PRIMARY)
            .and_then(|f| fmt_focal(&f.value))
    });
    push(&mut out, "software", "軟體", get(exif::Tag::Software));
    push(&mut out, "artist", "作者", get(exif::Tag::Artist));

    // Orientation as a readable label.
    if let Some(o) = exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY) {
        let label = match o.value.get_uint(0) {
            Some(1) => Some("正常".to_string()),
            Some(3) => Some("旋轉 180°".to_string()),
            Some(6) => Some("旋轉 90°".to_string()),
            Some(8) => Some("旋轉 270°".to_string()),
            _ => None,
        };
        if let Some(l) = label {
            out.push(ExifField {
                key: "orientation".into(),
                label: "方向".into(),
                value: l,
            });
        }
    }

    // GPS: decimal degrees if both fields exist.
    let gps = (|| -> Option<String> {
        let lat = exif.get_field(exif::Tag::GPSLatitude, exif::In::PRIMARY)?;
        let lat_ref = exif.get_field(exif::Tag::GPSLatitudeRef, exif::In::PRIMARY)?;
        let lon = exif.get_field(exif::Tag::GPSLongitude, exif::In::PRIMARY)?;
        let lon_ref = exif.get_field(exif::Tag::GPSLongitudeRef, exif::In::PRIMARY)?;
        let dms = |v: &exif::Value| -> Option<f64> {
            if let exif::Value::Rational(r) = v {
                if r.len() >= 3 {
                    let d = r[0].num as f64 / r[0].denom as f64;
                    let m = r[1].num as f64 / r[1].denom as f64;
                    let s = r[2].num as f64 / r[2].denom as f64;
                    return Some(d + m / 60.0 + s / 3600.0);
                }
            }
            None
        };
        let la = dms(&lat.value)?;
        let lo = dms(&lon.value)?;
        let la = if lat_ref.display_value().to_string().contains('S') { -la } else { la };
        let lo = if lon_ref.display_value().to_string().contains('W') { -lo } else { lo };
        Some(format!("{:.5}, {:.5}", la, lo))
    })();
    push(&mut out, "gps", "GPS", gps);

    out
}

fn inspect(path: &Path) -> ImageMeta {
    let (w, h) = dimensions(path);
    ImageMeta {
        path: path.to_string_lossy().to_string(),
        name: path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string(),
        dir: path
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default(),
        size: file_size(path),
        mtime: mtime_secs(path),
        width: w,
        height: h,
        format: format_name(path),
        exif: read_exif(path),
    }
}

/// Natural-ish sort so `img2.png` precedes `img10.png`.
fn natural_key(name: &str) -> (String, u64) {
    let lower = name.to_ascii_lowercase();
    let mut stem = String::new();
    let mut num = 0u64;
    let mut seen = false;
    for ch in lower.chars() {
        if ch.is_ascii_digit() {
            if !seen {
                num = num.saturating_mul(10).saturating_add(ch.to_digit(10).unwrap_or(0) as u64);
            }
        } else {
            if !stem.is_empty() {
                break;
            }
            seen = false;
            stem.push(ch);
        }
    }
    (stem, num)
}

#[tauri::command]
fn list_images(dir: String) -> Vec<DirEntry> {
    let dir_path = PathBuf::from(&dir);
    let mut out: Vec<DirEntry> = Vec::new();
    let rd = match fs::read_dir(&dir_path) {
        Ok(rd) => rd,
        Err(_) => return out,
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if !p.is_file() || !is_image(&p) {
            continue;
        }
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        out.push(DirEntry {
            path: p.to_string_lossy().to_string(),
            name,
            size: file_size(&p),
            mtime: mtime_secs(&p),
        });
    }
    out.sort_by(|a, b| natural_key(&a.name).cmp(&natural_key(&b.name)));
    out
}

/// Sub-directories only, so the in-app browser can walk the filesystem.
#[tauri::command]
fn list_dirs(dir: String) -> Vec<DirEntry> {
    let dir_path = PathBuf::from(&dir);
    let mut out: Vec<DirEntry> = Vec::new();
    let rd = match fs::read_dir(&dir_path) {
        Ok(rd) => rd,
        Err(_) => return out,
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if !p.is_dir() {
            continue;
        }
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        if name.starts_with('.') {
            continue;
        }
        out.push(DirEntry {
            path: p.to_string_lossy().to_string(),
            name,
            size: 0,
            mtime: mtime_secs(&p),
        });
    }
    out.sort_by(|a, b| natural_key(&a.name).cmp(&natural_key(&b.name)));
    out
}

#[tauri::command]
fn inspect_image(path: String) -> Result<ImageMeta, String> {
    let p = PathBuf::from(&path);
    if !p.is_file() {
        return Err(format!("找不到檔案：{}", path));
    }
    Ok(inspect(&p))
}

/// Raw bytes straight into the webview — no base64 hop.
#[tauri::command]
fn read_image_file(path: String) -> Result<Response, String> {
    let p = PathBuf::from(&path);
    match fs::read(&p) {
        Ok(bytes) => Ok(Response::new(bytes)),
        Err(e) => Err(format!("讀取失敗：{}", e)),
    }
}

#[tauri::command]
fn write_file(path: String, data: Vec<u8>) -> Result<u64, String> {
    let p = PathBuf::from(&path);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("建立目錄失敗：{}", e))?;
    }
    fs::write(&p, &data).map_err(|e| format!("寫入失敗：{}", e))?;
    Ok(data.len() as u64)
}

/// Base64 variant of `write_file` — avoids shipping a JSON array of numbers
/// through IPC for multi-megabyte exports.
#[tauri::command]
fn write_file_b64(path: String, data: String) -> Result<u64, String> {
    let bytes = STANDARD
        .decode(data.as_bytes())
        .map_err(|e| format!("base64 解碼失敗：{}", e))?;
    let p = PathBuf::from(&path);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("建立目錄失敗：{}", e))?;
    }
    fs::write(&p, &bytes).map_err(|e| format!("寫入失敗：{}", e))?;
    Ok(bytes.len() as u64)
}

#[tauri::command]
fn path_exists(path: String) -> bool {
    Path::new(&path).exists()
}

#[tauri::command]
fn unique_path(path: String) -> String {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return path;
    }
    let parent = p.parent().map(|x| x.to_path_buf()).unwrap_or_default();
    let stem = p
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image")
        .to_string();
    let ext = p
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    for i in 2..=9999 {
        let name = if ext.is_empty() {
            format!("{}-{}", stem, i)
        } else {
            format!("{}-{}.{}", stem, i, ext)
        };
        let cand = parent.join(name);
        if !cand.exists() {
            return cand.to_string_lossy().to_string();
        }
    }
    path
}

#[tauri::command]
fn open_external(path: String) {
    let target = PathBuf::from(&path);
    let dir = if target.is_dir() {
        target
    } else {
        target.parent().map(|p| p.to_path_buf()).unwrap_or_default()
    };
    let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
}

/// Image files passed on the command line (already absolutised + normalised).
#[tauri::command]
fn startup_files() -> Vec<String> {
    collect_files(&std::env::args().skip(1).collect::<Vec<_>>())
}

/// Respect the XDG pictures dir (localised names like ~/圖片), then fall back
/// to the common English name and finally to $HOME.
#[tauri::command]
fn home_pictures() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let cfg = PathBuf::from(&home).join(".config").join("user-dirs.dirs");
    if let Ok(text) = fs::read_to_string(cfg) {
        for line in text.lines() {
            let line = line.trim();
            if !line.starts_with("XDG_PICTURES_DIR=") {
                continue;
            }
            let raw = line
                .split('=')
                .nth(1)
                .unwrap_or("")
                .trim()
                .trim_matches('"');
            let expanded = raw.replace("$HOME", &home);
            let p = PathBuf::from(expanded);
            if p.is_dir() {
                return p.to_string_lossy().to_string();
            }
        }
    }
    for name in ["Pictures", "圖片", "Bilder", "Imágenes", "Images"] {
        let p = PathBuf::from(&home).join(name);
        if p.is_dir() {
            return p.to_string_lossy().to_string();
        }
    }
    home
}

/// Called by the frontend as soon as the boot splash has painted, so the window
/// is revealed already dark instead of flashing white (or an empty shell).
#[tauri::command]
fn app_ready(window: tauri::Window) {
    let _ = window.show();
    let _ = window.set_focus();
}

#[tauri::command]
fn set_window_title(window: tauri::Window, title: String) {
    let _ = window.set_title(&title);
}

// ---------------------------------------------------------------------------
// Single instance + `lumen://open` plumbing
// ---------------------------------------------------------------------------

fn raise_window(win: &tauri::WebviewWindow) {
    let _ = win.show();
    let _ = win.unminimize();
    let _ = win.set_focus();
}

/// Emit `lumen://open` to the main window with a path payload and raise it.
fn emit_open(app: &AppHandle, path: String) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.emit(OPEN_EVENT, &path);
        raise_window(&win);
    }
}

fn focus_main(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        raise_window(&win);
    }
}

/// Holds open requests that arrive before the webview has finished loading.
/// Shared by `Builder::on_page_load`, the single-instance socket thread and
/// the startup watchdog (all of which may race during boot).
#[derive(Default)]
struct OpenQueue {
    ready: AtomicBool,
    pending: Mutex<Vec<String>>,
}

impl OpenQueue {
    /// A path arrived: deliver now if the page is ready, otherwise keep it.
    fn dispatch(&self, app: &AppHandle, path: String) {
        if self.ready.load(Ordering::SeqCst) {
            emit_open(app, path);
        } else {
            self.pending.lock().unwrap().push(path);
        }
    }

    /// The webview finished its first paint: flush everything queued while it
    /// was booting (argv files, file-manager launches).
    fn mark_ready(&self, app: &AppHandle) {
        if self.ready.swap(true, Ordering::SeqCst) {
            return;
        }
        let queued: Vec<String> = std::mem::take(&mut *self.pending.lock().unwrap());
        for path in queued {
            emit_open(app, path);
        }
    }

    fn handle_line(&self, app: &AppHandle, line: &str) {
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            return;
        }
        if let Some(path) = line.strip_prefix("open\t") {
            let path = path.trim();
            if !path.is_empty() {
                self.dispatch(app, path.to_string());
            }
        } else if line == "focus" {
            focus_main(app);
        }
    }
}

fn serve(listener: UnixListener, queue: Arc<OpenQueue>, app: AppHandle) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let reader = BufReader::new(stream);
        for line in reader.lines() {
            match line {
                Ok(line) => queue.handle_line(&app, &line),
                Err(_) => break,
            }
        }
    }
}

/// Candidate socket locations, most private first. `$XDG_RUNTIME_DIR` is
/// per-user 0700, but inside restrictive sandboxes it can be read-only, so
/// fall back to the temp dir instead of silently losing single-instance.
fn socket_candidates() -> Vec<PathBuf> {
    let user = std::env::var("USER").unwrap_or_else(|_| "user".into());
    let name = format!("lumen-{}.sock", user);
    let mut out: Vec<PathBuf> = Vec::new();
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        if !dir.is_empty() {
            out.push(PathBuf::from(dir).join(&name));
        }
    }
    out.push(std::env::temp_dir().join(&name));
    out
}

/// Hand this launch's files to an already-running instance.
fn forward_to_running(sockets: &[PathBuf], files: &[String]) -> bool {
    for sock in sockets {
        let Ok(mut stream) = UnixStream::connect(sock) else {
            continue;
        };
        let mut payload = String::new();
        if files.is_empty() {
            payload.push_str("focus\n");
        } else {
            for file in files {
                payload.push_str("open\t");
                payload.push_str(file);
                payload.push('\n');
            }
        }
        let ok = stream.write_all(payload.as_bytes()).is_ok() && stream.flush().is_ok();
        drop(stream);
        // Leave the peer's reader thread a moment before this process exits.
        std::thread::sleep(Duration::from_millis(if ok { 60 } else { 0 }));
        return ok;
    }
    false
}

fn bind_listener(sockets: &[PathBuf]) -> Option<(PathBuf, UnixListener)> {
    for sock in sockets {
        if let Some(parent) = sock.parent() {
            let _ = fs::create_dir_all(parent);
        }
        // Only reached when connecting failed, so a leftover socket is stale.
        if sock.exists() {
            let _ = fs::remove_file(sock);
        }
        if let Ok(listener) = UnixListener::bind(sock) {
            return Some((sock.clone(), listener));
        }
    }
    None
}

/// WebKit (and Tauri) need writable data/cache/config dirs; in restricted
/// sandboxes `$HOME` is read-only, which makes app setup fail with EROFS.
/// Fall back to a temp dir so the app still boots.
fn dir_writable(dir: &Path) -> bool {
    if fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".lumen-write-probe");
    match fs::write(&probe, b"") {
        Ok(_) => {
            let _ = fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

fn ensure_xdg_dirs() {
    let base = std::env::var("XDG_RUNTIME_DIR")
        .ok()
        .filter(|d| !d.is_empty() && dir_writable(Path::new(d)))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("lumen-runtime");
    for (var, sub) in [
        ("XDG_DATA_HOME", "data"),
        ("XDG_CACHE_HOME", "cache"),
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_STATE_HOME", "state"),
    ] {
        let current_ok = std::env::var(var)
            .ok()
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .map(|p| dir_writable(&p))
            .unwrap_or(false);
        if current_ok {
            continue;
        }
        let fallback = base.join(sub);
        if dir_writable(&fallback) {
            std::env::set_var(var, &fallback);
        }
    }
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

enum Cli {
    Run,
    Version,
    Help,
}

fn parse_cli(args: &[String]) -> Cli {
    for arg in args {
        match arg.as_str() {
            "-V" | "--version" | "version" => return Cli::Version,
            "-h" | "--help" | "help" => return Cli::Help,
            _ => {}
        }
    }
    Cli::Run
}

fn print_help() {
    println!(
        "{name} {version} — 輕量級圖片檢視／標註器\n\
         \n\
         用法：\n  \
         lumen [選項] [圖片或資料夾...]\n\
         \n\
         選項：\n  \
         -h, --help       顯示這份說明並結束\n  \
         -V, --version    顯示版本並結束\n\
         \n\
         範例：\n  \
         lumen ~/Pictures/IMG_0001.jpg\n  \
         lumen ~/Pictures\n  \
         lumen %U        # 檔案管理器開啟（file:// URL 亦可）\n",
        name = APP_NAME,
        version = env!("CARGO_PKG_VERSION")
    );
}

/// `file://` URL (file managers pass these) → plain local path.
fn file_url_to_path(raw: &str) -> Option<PathBuf> {
    let rest = raw.strip_prefix("file://")?;
    // Strip an optional host component (`file://localhost/path`).
    let rest = match rest.find('/') {
        Some(0) => rest,
        Some(idx) => &rest[idx..],
        None => rest,
    };
    let mut bytes: Vec<u8> = Vec::with_capacity(rest.len());
    let raw = rest.as_bytes();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%' && i + 2 < raw.len() {
            let hex = |c: u8| -> Option<u8> {
                match c {
                    b'0'..=b'9' => Some(c - b'0'),
                    b'a'..=b'f' => Some(c - b'a' + 10),
                    b'A'..=b'F' => Some(c - b'A' + 10),
                    _ => None,
                }
            };
            if let (Some(hi), Some(lo)) = (hex(raw[i + 1]), hex(raw[i + 2])) {
                bytes.push(hi * 16 + lo);
                i += 3;
                continue;
            }
        }
        bytes.push(raw[i]);
        i += 1;
    }
    Some(PathBuf::from(String::from_utf8_lossy(&bytes).to_string()))
}

fn first_image_in(dir: &Path) -> Option<PathBuf> {
    let mut names: Vec<(String, PathBuf)> = fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && is_image(p))
        .map(|p| {
            let name = p
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            (name, p)
        })
        .collect();
    names.sort_by(|a, b| natural_key(&a.0).cmp(&natural_key(&b.0)));
    names.into_iter().next().map(|(_, p)| p)
}

/// Turn raw argv entries into absolute image paths. Directories open their
/// first image (mirrors what a file manager's "open with" usually expects).
fn collect_files(args: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut clear_flags = false;
    for arg in args {
        let arg = arg.trim();
        if arg.is_empty() {
            continue;
        }
        if !clear_flags {
            if arg == "--" {
                clear_flags = true;
                continue;
            }
            if arg.starts_with('-') {
                continue;
            }
        }
        let path = match file_url_to_path(arg) {
            Some(p) => p,
            None => PathBuf::from(arg),
        };
        let absolute = if path.is_absolute() {
            path
        } else {
            match std::env::current_dir() {
                Ok(cwd) => cwd.join(path),
                Err(_) => path,
            }
        };
        let resolved = fs::canonicalize(&absolute).unwrap_or(absolute);
        if resolved.is_dir() {
            if let Some(first) = first_image_in(&resolved) {
                out.push(first.to_string_lossy().to_string());
            }
        } else if resolved.is_file() && is_image(&resolved) {
            out.push(resolved.to_string_lossy().to_string());
        }
    }
    let mut seen: Vec<String> = Vec::new();
    for path in out {
        if !seen.contains(&path) {
            seen.push(path);
        }
    }
    seen
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match parse_cli(&args) {
        Cli::Version => {
            println!("{} {}", APP_NAME, env!("CARGO_PKG_VERSION"));
            return;
        }
        Cli::Help => {
            print_help();
            return;
        }
        Cli::Run => {}
    }

    // A second launch (file manager "Open with", `lumen foo.png`) forwards its
    // paths to the running instance and exits without creating a window.
    let files = collect_files(&args);
    let sockets = socket_candidates();
    if forward_to_running(&sockets, &files) {
        return;
    }

    // WebKit needs writable XDG dirs even when $HOME is read-only (sandboxes).
    ensure_xdg_dirs();

    // Bind before building the app: a file manager launch landing during our
    // own startup then queues instead of spawning a duplicate window.
    let listener = bind_listener(&sockets);
    let sock_for_cleanup = listener.as_ref().map(|(path, _)| path.clone());
    let listener = listener.map(|(_, listener)| listener);
    let queue = Arc::new(OpenQueue::default());
    let queue_page = queue.clone();

    tauri::Builder::default()
        // Release the queue only once the webview has actually painted, so the
        // frontend's `lumen://open` listener is registered when events fire.
        .on_page_load(move |webview, payload| {
            if matches!(payload.event(), tauri::webview::PageLoadEvent::Finished) {
                // Reveal as soon as the document is loaded: the window背景 and the
                // inline critical CSS are already dark, so this never flashes white.
                let window = webview.window();
                let _ = window.show();
                let _ = window.set_focus();
                queue_page.mark_ready(webview.app_handle());
            }
        })
        .setup(move |app| {
            let handle = app.handle().clone();

            // Show the window immediately: it is already painted in the dark
            // background colour, so the click feels instant instead of waiting
            // for the WebView to finish loading.
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
                let fallback = w.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(2500));
                    let _ = fallback.show();
                });
            }

            // Files this process was started with.
            for file in &files {
                queue.dispatch(&handle, file.clone());
            }

            // Safety net: if the page-load hook never fires (webview error),
            // still flush a little later rather than dropping user input.
            let queue_watchdog = queue.clone();
            let handle_watchdog = handle.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(10));
                queue_watchdog.mark_ready(&handle_watchdog);
            });

            if let Some(listener) = listener {
                let queue_server = queue.clone();
                let handle_server = handle.clone();
                std::thread::spawn(move || serve(listener, queue_server, handle_server));
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_images,
            list_dirs,
            inspect_image,
            read_image_file,
            write_file,
            write_file_b64,
            path_exists,
            unique_path,
            open_external,
            startup_files,
            home_pictures,
            app_ready,
            set_window_title
        ])
        .run(tauri::generate_context!())
        .expect("error while running Lumen");

    if let Some(sock) = sock_for_cleanup {
        let _ = fs::remove_file(sock);
    }
}

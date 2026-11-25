// System file pickers via zenity/kdialog (both show the desktop's native
// GTK/Qt chooser; zenity 4 goes through xdg-desktop-portal on GNOME).
// Falls back to the in-app browser when neither tool is installed.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::OnceLock;

fn have(tool: &str) -> bool {
    static CACHE: OnceLock<(bool, bool)> = OnceLock::new();
    let (z, k) = *CACHE.get_or_init(|| {
        let has = |name: &str| {
            std::process::Command::new("sh")
                .args(["-c", &format!("command -v {name} >/dev/null 2>&1")])
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        };
        let _ = tool;
        (has("zenity"), has("kdialog"))
    });
    if tool == "zenity" { z } else { k }
}

pub fn has_native_dialog() -> bool {
    have("zenity") || have("kdialog")
}

pub fn zenity_open_args(start: Option<&Path>, title: &str) -> Vec<String> {
    let mut args = vec![
        "--file-selection".to_string(),
        format!("--title={title}"),
        "--file-filter=圖片 | *.jpg *.jpeg *.png *.webp *.gif *.bmp *.tif *.tiff *.avif *.svg".to_string(),
        "--file-filter=所有檔案 | *".to_string(),
    ];
    if let Some(dir) = start {
        args.push(format!("--filename={}", dir.display()));
    }
    args
}

pub fn zenity_save_args(suggested: &Path, title: &str) -> Vec<String> {
    vec![
        "--file-selection".to_string(),
        "--save".to_string(),
        "--confirm-overwrite".to_string(),
        format!("--title={title}"),
        format!("--filename={}", suggested.display()),
        "--file-filter=圖片 | *.jpg *.jpeg *.png *.webp".to_string(),
    ]
}

pub fn kdialog_open_args(start: Option<&Path>) -> Vec<String> {
    vec![
        "--getopenfilename".to_string(),
        start
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| ".".to_string()),
        "Images (*.jpg *.jpeg *.png *.webp *.gif *.bmp *.tif *.tiff *.avif *.svg)".to_string(),
    ]
}

pub fn kdialog_save_args(suggested: &Path) -> Vec<String> {
    vec![
        "--getsavefilename".to_string(),
        suggested.display().to_string(),
        "Images (*.jpg *.jpeg *.png *.webp)".to_string(),
        "--confirmoverwrite".to_string(),
    ]
}

#[derive(Debug)]
pub enum Outcome {
    Picked(PathBuf),
    Cancelled,
    /// The tool failed immediately (no portal/session, missing backend): the
    /// caller should fall back to the in-app dialog.
    Failed,
}

/// A dialog running on a worker thread; poll it once per frame.
pub struct NativeDialog {
    rx: Receiver<Outcome>,
    done: bool,
}

impl NativeDialog {
    fn spawn(ctx: &egui::Context, args: Vec<String>, tool: &'static str) -> Self {
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let started = std::time::Instant::now();
            let output = std::process::Command::new(tool).args(&args).output();
            if std::env::var_os("LUMEN_LOG").is_some() {
                match &output {
                    Ok(o) => eprintln!(
                        "[dialog] {tool} status={:?} stdout={:?} stderr={:?}",
                        o.status,
                        String::from_utf8_lossy(&o.stdout).trim(),
                        String::from_utf8_lossy(&o.stderr).trim()
                    ),
                    Err(e) => eprintln!("[dialog] {tool} spawn failed: {e}"),
                }
            }
            let outcome = match output {
                Ok(out) if out.status.success() => {
                    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if text.is_empty() {
                        Outcome::Cancelled
                    } else {
                        Outcome::Picked(PathBuf::from(text))
                    }
                }
                // A cancel takes user time; an immediate failure means the
                // portal/display is unavailable -> let the app fall back.
                Ok(_) if started.elapsed() < std::time::Duration::from_millis(600) => {
                    Outcome::Failed
                }
                Ok(_) => Outcome::Cancelled,
                Err(_) => Outcome::Failed,
            };
            let _ = tx.send(outcome);
            ctx.request_repaint();
        });
        Self { rx, done: false }
    }

    pub fn open(ctx: &egui::Context, start: Option<&Path>, title: &str) -> Option<Self> {
        if have("zenity") {
            Some(Self::spawn(ctx, zenity_open_args(start, title), "zenity"))
        } else if have("kdialog") {
            Some(Self::spawn(ctx, kdialog_open_args(start), "kdialog"))
        } else {
            None
        }
    }

    pub fn save(ctx: &egui::Context, suggested: &Path, title: &str) -> Option<Self> {
        if have("zenity") {
            Some(Self::spawn(ctx, zenity_save_args(suggested, title), "zenity"))
        } else if have("kdialog") {
            Some(Self::spawn(ctx, kdialog_save_args(suggested), "kdialog"))
        } else {
            None
        }
    }

    /// Returns `Some(outcome)` exactly once when the user is done.
    pub fn poll(&mut self) -> Option<Outcome> {
        if self.done {
            return None;
        }
        match self.rx.try_recv() {
            Ok(res) => {
                self.done = true;
                Some(res)
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.done = true;
                Some(Outcome::Failed)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zenity_arguments_match_the_tool() {
        let open = zenity_open_args(Some(Path::new("/tmp")), "開啟圖片");
        assert_eq!(open[0], "--file-selection");
        assert!(open.iter().any(|a| a == "--filename=/tmp"));
        assert!(open.iter().any(|a| a.starts_with("--file-filter=圖片")));
        let save = zenity_save_args(Path::new("/tmp/a.jpg"), "另存新檔");
        assert!(save.iter().any(|a| a == "--save"));
        assert!(save.iter().any(|a| a == "--confirm-overwrite"));
        assert!(save.iter().any(|a| a == "--filename=/tmp/a.jpg"));
    }

    #[test]
    fn kdialog_arguments_match_the_tool() {
        let open = kdialog_open_args(Some(Path::new("/tmp")));
        assert_eq!(open[0], "--getopenfilename");
        assert_eq!(open[1], "/tmp");
        let save = kdialog_save_args(Path::new("/tmp/b.png"));
        assert_eq!(save[0], "--getsavefilename");
        assert_eq!(save[1], "/tmp/b.png");
    }
}

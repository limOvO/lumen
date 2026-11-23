// Single-instance behaviour, matching the web build: a second launch with a file
// forwards it to the running window instead of opening another one.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, OnceLock};

use std::os::unix::net::{UnixListener, UnixStream};
use std::io::{BufRead, BufReader, Write};

static LISTENER: Mutex<Option<PathBuf>> = Mutex::new(None);
static REPAINT: OnceLock<egui::Context> = OnceLock::new();

/// The UI is event-driven; a socket message must wake it up explicitly.
pub fn attach_context(ctx: egui::Context) {
    let _ = REPAINT.set(ctx);
}

fn socket_path() -> PathBuf {
    if let Ok(explicit) = std::env::var("LUMEN_SOCKET") {
        if !explicit.trim().is_empty() {
            return PathBuf::from(explicit);
        }
    }
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(runtime).join("lumen-egui.sock")
}

/// Hand one path to the already-running instance. Returns true when delivered.
pub fn forward(path: &Path) -> bool {
    let Ok(mut stream) = UnixStream::connect(socket_path()) else {
        return false;
    };
    let Some(text) = path.to_str() else { return false };
    stream.write_all(text.as_bytes()).is_ok() && stream.write_all(b"\n").is_ok()
}

/// Claim the socket and return a receiver for paths sent by later launches.
/// Stale sockets (no listener) are removed before binding.
pub fn start_listener() -> Option<Receiver<PathBuf>> {
    let path = socket_path();
    if path.exists() {
        // Probe first: a live listener means `forward` above should have won.
        if UnixStream::connect(&path).is_ok() {
            return None;
        }
        let _ = std::fs::remove_file(&path);
    }
    let Ok(listener) = UnixListener::bind(&path) else { return None };
    *LISTENER.lock().unwrap_or_else(|e| e.into_inner()) = Some(path);
    let (tx, rx) = channel::<PathBuf>();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let sender: Sender<PathBuf> = tx.clone();
            std::thread::spawn(move || {
                let reader = BufReader::new(stream);
                for line in reader.lines().map_while(Result::ok) {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() {
                        let _ = sender.send(PathBuf::from(trimmed));
                        if let Some(ctx) = REPAINT.get() {
                            ctx.request_repaint();
                        }
                    }
                }
            });
        }
    });
    Some(rx)
}

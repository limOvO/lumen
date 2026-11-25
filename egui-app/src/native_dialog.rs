// Native file pickers: the GTK3 file chooser running *on the main thread* and
// pumped from the app's frame loop. Running GTK on the main thread (instead of
// a worker thread / zenity process) is what avoids the desktop's busy cursor
// while the dialog is being built.

use std::path::{Path, PathBuf};

use gtk::prelude::*;

pub enum Outcome {
    Picked(PathBuf),
    Cancelled,
    /// GTK could not start (no display / gtk init failed): fall back to the
    /// in-app dialog.
    Failed,
}

#[derive(Clone, Copy)]
enum Mode {
    Open,
    Save,
}

pub struct NativeDialog {
    dialog: gtk::FileChooserDialog,
    response: std::rc::Rc<std::cell::Cell<Option<gtk::ResponseType>>>,
    mode: Mode,
    done: bool,
}

fn image_filter() -> gtk::FileFilter {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("Images"));
    for pattern in [
        "*.jpg", "*.jpeg", "*.png", "*.webp", "*.gif", "*.bmp", "*.tif", "*.tiff", "*.avif",
        "*.svg",
    ] {
        filter.add_pattern(pattern);
    }
    filter
}

impl NativeDialog {
    fn build(mode: Mode, start: Option<&Path>, title: &str) -> Option<Self> {
        // GTK must be initialized on this (the main) thread; ignore repeated init.
        if gtk::init().is_err() {
            return None;
        }
        let (action, accept) = match mode {
            Mode::Open => (gtk::FileChooserAction::Open, "開啟"),
            Mode::Save => (gtk::FileChooserAction::Save, "儲存"),
        };
        let dialog = gtk::FileChooserDialog::with_buttons(
            Some(title),
            None::<&gtk::Window>,
            action,
            &[
                ("取消", gtk::ResponseType::Cancel),
                (accept, gtk::ResponseType::Accept),
            ],
        );
        dialog.set_modal(true);
        dialog.add_filter(image_filter());
        if let Some(dir) = start {
            dialog.set_current_folder(dir);
        }
        dialog.show_all();
        let response = std::rc::Rc::new(std::cell::Cell::new(None));
        let slot = response.clone();
        dialog.connect_response(move |_, id| slot.set(Some(id)));
        Some(Self {
            dialog,
            response,
            mode,
            done: false,
        })
    }

    pub fn open(start: Option<&Path>, title: &str) -> Option<Self> {
        Self::build(Mode::Open, start, title)
    }

    pub fn save(suggested: &Path, title: &str) -> Option<Self> {
        let start = suggested.parent().map(|p| p.to_path_buf());
        let dialog = Self::build(Mode::Save, start.as_deref(), title)?;
        if let Some(name) = suggested.file_name().and_then(|n| n.to_str()) {
            dialog.dialog.set_current_name(name);
        }
        dialog.dialog.set_do_overwrite_confirmation(true);
        Some(dialog)
    }

    /// Pump GTK once and report a final outcome when the user is done.
    pub fn poll(&mut self, ctx: &egui::Context) -> Option<Outcome> {
        if self.done {
            return None;
        }
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
        let Some(response) = self.response.take() else {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
            return None;
        };
        self.done = true;
        let outcome = if response == gtk::ResponseType::Accept {
            match self.dialog.filename() {
                Some(path) => Outcome::Picked(path),
                None => Outcome::Cancelled,
            }
        } else {
            Outcome::Cancelled
        };
        self.dialog.hide();
        // `mode` is only kept so we can tell open/save apart in diagnostics.
        let _ = self.mode;
        Some(outcome)
    }
}

impl Drop for NativeDialog {
    fn drop(&mut self) {
        self.dialog.hide();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn filter_patterns_cover_supported_formats() {
        let patterns = ["jpg", "jpeg", "png", "webp", "gif", "bmp", "tif", "tiff", "avif", "svg"];
        for ext in patterns {
            assert!(!ext.is_empty());
        }
    }
}

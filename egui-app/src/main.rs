mod annotate;
mod app;
mod browser;
mod crop;
mod dialogs;
mod doc;
mod export;
mod fonts;
mod i18n;
mod i18n_data;
mod jpeg_fast;
mod interaction;
mod save;
mod single_instance;
mod state;
mod theme;
mod ui;

use std::path::PathBuf;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("Lumen (egui) {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "Lumen (Rust + wgpu + egui build) {}\n\n\
             usage: lumen-egui [image | folder]\n\n\
             Opens straight into the viewer. Keyboard: ←/→ images, +/-/0 zoom,\n\
             I info, E editor, V B R O A L T C tools, Ctrl+O/S/Shift+S files,\n\
             F fullscreen, ? shortcuts.\n\n\
             LUMEN_SOCKET overrides the single-instance socket path.",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(());
    }
    let initial: Option<PathBuf> = args.first().map(PathBuf::from);

    // Second launch with a file: hand it to the running window and exit.
    if let Some(path) = initial.as_ref() {
        if single_instance::forward(path) {
            return Ok(());
        }
    }
    let incoming = single_instance::start_listener();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([880.0, 560.0])
            .with_title("Lumen"),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        "Lumen",
        options,
        Box::new(move |cc| {
            let locale = i18n::Locale::detect();
            fonts::install(&cc.egui_ctx, locale);
            Ok(Box::new(app::LumenApp::new(initial.clone(), incoming)))
        }),
    )
}

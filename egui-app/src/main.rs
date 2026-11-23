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
mod interaction;
mod save;
mod state;
mod theme;
mod ui;

use std::path::PathBuf;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let initial: Option<PathBuf> = std::env::args().nth(1).map(PathBuf::from);
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
            Ok(Box::new(app::LumenApp::new(initial.clone())))
        }),
    )
}

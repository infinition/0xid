#![windows_subsystem = "windows"]

mod app;
mod icons;
mod plugins;
mod settings;
mod sounds;
mod tray;
mod ui;
mod scanner;
mod ssh;
mod wol;
mod wsl;

fn load_icon() -> Option<eframe::egui::IconData> {
    let png_data = include_bytes!("../assets/0xid.png");
    let image = image::load_from_memory(png_data).ok()?.into_rgba8();
    let (w, h) = image.dimensions();
    Some(eframe::egui::IconData {
        rgba: image.into_raw(),
        width: w,
        height: h,
    })
}

fn main() -> eframe::Result<()> {
    // Load persisted settings before anything reads them (sounds, tray hotkey…).
    settings::load();

    let start_hidden = std::env::args().any(|a| a == "--hidden");

    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title("0xID")
        .with_decorations(false)
        .with_maximized(false)              // never start maximized — avoids black fullscreen on startup
        .with_visible(!start_hidden)
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([800.0, 600.0]);

    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "0xID",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, start_hidden)))),
    )
}

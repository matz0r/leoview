mod app;
mod layout;
mod loader;

use std::path::PathBuf;

use eframe::egui;

fn main() -> eframe::Result {
    // Optional start location: a directory, or an image file to open directly.
    let start = std::env::args_os().nth(1).map(PathBuf::from);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("LeoView")
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([640.0, 400.0])
            .with_icon(window_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "LeoView",
        options,
        Box::new(|cc| Ok(Box::new(app::LeoView::new(cc, start)))),
    )
}

fn window_icon() -> egui::IconData {
    let icon = image::load_from_memory(app::ICON_PNG)
        .expect("embedded icon is a valid PNG")
        .to_rgba8();
    egui::IconData {
        width: icon.width(),
        height: icon.height(),
        rgba: icon.into_raw(),
    }
}

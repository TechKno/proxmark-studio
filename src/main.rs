#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
mod ui;
fn main() -> eframe::Result {
    let config = match proxmark_studio::launch::config_argument(std::env::args_os().skip(1)) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size([1280.0, 860.0])
        .with_min_inner_size([960.0, 700.0])
        .with_title("Proxmark Studio");
    #[cfg(target_os = "macos")]
    let viewport = viewport
        .with_fullsize_content_view(true)
        .with_title_shown(false)
        .with_titlebar_shown(false);
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "Proxmark Studio",
        options,
        Box::new(move |cc| Ok(Box::new(ui::Studio::new(cc, config)))),
    )
}

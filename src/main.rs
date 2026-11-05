#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> eframe::Result {
    let settings = foto_importer::config::Settings::load();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(settings.window_size.map(|v| {
                if v.is_finite() {
                    v.clamp(680.0, 7680.0)
                } else {
                    800.0
                }
            }))
            .with_clamp_size_to_monitor_size(true)
            .with_min_inner_size([980.0, 680.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Foto-Importer",
        options,
        Box::new(|cc| Ok(Box::new(foto_importer::ui::PhotoApp::new(cc)))),
    )
}

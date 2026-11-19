#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use foto_importer::{config::Settings, core::diag};

fn main() -> eframe::Result {
    let loading = std::time::Instant::now();
    let settings = Settings::load();
    let loaded = loading.elapsed();
    diag::init(Settings::log_dir(), settings.diagnostic_log);
    log::info!(
        target: "start",
        "sitzung version={} build={}·{} pid={} log={:?} einstellungen={:?} sammlung={:?} dunkel={} portion={} auto_metadaten={} zentral={}",
        env!("CARGO_PKG_VERSION"),
        env!("FOTO_IMPORTER_GIT_BRANCH"),
        env!("FOTO_IMPORTER_GIT_COMMIT"),
        std::process::id(),
        Settings::log_dir().display().to_string(),
        Settings::path().display().to_string(),
        settings.collection,
        settings.dark,
        settings.work_portion,
        settings.auto_metadata,
        settings.central_enabled,
    );
    log::info!(target: "start", "einstellungen_gelesen dauer_ms={:.1}", diag::millis(loaded));
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
    let result = eframe::run_native(
        "Foto-Importer",
        options,
        Box::new(|cc| Ok(Box::new(foto_importer::ui::PhotoApp::new(cc)))),
    );
    match &result {
        Ok(()) => {
            log::info!(target: "start", "sitzung ende laufzeit_s={}", diag::uptime().as_secs())
        }
        Err(e) => log::error!(target: "start", "sitzung abgebrochen fehler={:?}", e.to_string()),
    }
    diag::flush(std::time::Duration::from_secs(2));
    result
}

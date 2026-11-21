//! Messung: Dauer von `Settings::save` (atomar schreiben mit `sync_all`), das die Oberfläche
//! beim Ändern einer Einstellung auf dem UI-Thread aufruft. Schreibt nur in einen temporären
//! Ordner (`FOTO_IMPORTER_SETTINGS`), nie in die echte Einstellungsdatei.
//!
//! `cargo run --release --example settings_save_bench`
use foto_importer::config::Settings;
use std::time::{Duration, Instant};

fn main() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    // SAFETY: einziger Thread des Beispiels; vor jedem Lesen der Variable gesetzt.
    unsafe { std::env::set_var("FOTO_IMPORTER_SETTINGS", temp.path().join("settings.json")) };
    let settings = Settings::default();
    settings.save()?;
    let mut times: Vec<Duration> = (0..200)
        .map(|_| {
            let started = Instant::now();
            settings.save().map(|()| started.elapsed())
        })
        .collect::<anyhow::Result<_>>()?;
    times.sort();
    let mean = times.iter().sum::<Duration>() / times.len() as u32;
    println!(
        "Settings::save, 200 Läufe: Mittel {mean:?}, Median {:?}, p95 {:?}, Maximum {:?}",
        times[times.len() / 2],
        times[times.len() * 95 / 100],
        times[times.len() - 1]
    );
    Ok(())
}

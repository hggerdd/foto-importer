//! Kosten des Diagnose-Logs auf dem aufrufenden Thread (Oberfläche, Worker):
//! `cargo run --release --example diag_bench`. Schreibt nur in ein temporäres Verzeichnis.
use foto_importer::core::diag::{Diag, Limits, span};
use std::time::{Duration, Instant};

fn per_call(count: u32, mut work: impl FnMut(u32)) -> Duration {
    let started = Instant::now();
    for n in 0..count {
        work(n);
    }
    started.elapsed() / count
}

fn main() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let count = 4000;
    let off = Diag::start(temp.path().join("aus"), false, Limits::APP);
    let on = Diag::start(temp.path().join("an"), true, Limits::APP);
    let disabled = per_call(count, |n| {
        off.write(log::Level::Info, "job", &format_args!("ende n={n}"));
    });
    let enabled = per_call(count, |n| {
        on.write(
            log::Level::Info,
            "ui.frame",
            &format_args!(
                "langsam dauer_ms={:.1} update_ms={:.1} ansicht=faces/unknown texturen={n}",
                73.4, 12.0
            ),
        );
    });
    let flushed = Instant::now();
    on.flush(Duration::from_secs(10));
    let spans = per_call(count, |_| drop(span("test", "messung")));
    println!("aus: {disabled:?} je Zeile");
    println!("an: {enabled:?} je Zeile (Formatieren + Einreihen)");
    println!(
        "Schreib-Thread: {:?} für den Rest von {count} Zeilen",
        flushed.elapsed()
    );
    println!("span ohne Log: {spans:?}");
    Ok(())
}

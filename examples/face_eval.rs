//! Misst Erkennungsschwellen an einer Sammlung (nur lesend).
//!
//! `cargo run --example face_eval -- <Sammlungsordner>`
use foto_importer::core::{collection::Collection, evaluation};
use std::path::Path;

const THRESHOLDS: [f32; 8] = [0.363, 0.40, 0.45, 0.50, 0.55, 0.60, 0.65, 0.70];
const MARGIN: f32 = 0.05;

fn main() -> anyhow::Result<()> {
    let Some(root) = std::env::args().nth(1) else {
        eprintln!("Aufruf: cargo run --example face_eval -- <Sammlungsordner>");
        eprintln!("Liest die Sammlung nur und druckt die Schwellenmessung.");
        std::process::exit(2);
    };
    let collection = Collection::open(Path::new(&root), false)?;
    let (references, queries) = evaluation::load(&collection)?;
    let report = evaluation::evaluate(&references, &queries, &THRESHOLDS, MARGIN);
    println!(
        "{} Abfragen, {} Referenzen, {} Personen, {} übersprungen",
        report.queries, report.references, report.persons, report.skipped
    );
    println!(
        "{:<9} | {:>17} | {:>18} | {:>22} | {:>23}",
        "Schwelle",
        "eigene erreicht",
        "andere erreicht",
        "eigene mit Abstand",
        "andere mit Abstand"
    );
    let pct = |x: f32| format!("{:.1} %", x * 100.0);
    for row in &report.rows {
        println!(
            "{:<9.3} | {:>17} | {:>18} | {:>22} | {:>23}",
            row.threshold,
            pct(row.own_reaches),
            pct(row.other_reaches),
            pct(row.own_accepted),
            pct(row.other_accepted)
        );
    }
    println!("Abstand zur zweitbesten Person: {MARGIN}");
    Ok(())
}

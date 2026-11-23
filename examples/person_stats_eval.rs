//! Kennzahlen der Personenstatistik je Person (nur lesend).
//!
//! `cargo run --release --example person_stats_eval -- <Sammlungsordner>`
//!
//! Öffnet die Sammlung nur lesend und rechnet wie die Ansicht „Statistik“ (ohne
//! Zwischenspeicher des Abgleichs). Namen erscheinen auf drei Buchstaben gekürzt.
use foto_importer::core::{collection::Collection, jobs::Control, person_stats, progress::decimal};
use std::path::Path;
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let Some(root) = std::env::args().nth(1) else {
        eprintln!("Aufruf: person_stats_eval <Sammlungsordner>");
        std::process::exit(2);
    };
    let collection = Collection::open(Path::new(&root), false)?;
    let started = Instant::now();
    let stats = person_stats::compute(&collection, &Control::default())?;
    let elapsed = started.elapsed();
    let median = |d: &person_stats::Distribution| {
        d.summary.map_or("–".to_string(), |s| decimal(s.median, 2))
    };
    println!(
        "Schwellen: Vorschlag {} / sicher {}; Rechenzeit {:.1} s",
        decimal(stats.review, 2),
        decimal(stats.sure, 2),
        elapsed.as_secs_f64()
    );
    println!(
        "| Person | bestätigt | abgelehnt | offen bewertet | Median bestätigt | Median abgelehnt | Median offen | t | über t: bestätigt | über t: offen |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|");
    for index in stats.order(person_stats::Mode::Confirmed) {
        let p = &stats.people[index];
        let short: String = p
            .name
            .chars()
            .filter(|c| c.is_alphabetic())
            .take(3)
            .collect::<String>()
            .to_uppercase();
        let (t, above, rated) = match p.threshold {
            Some(t) => (
                decimal(t.t, 3),
                format!("{} von {}", t.confirmed_above, p.confirmed.n),
                t.rated_above.to_string(),
            ),
            None => ("–".into(), "–".into(), "–".into()),
        };
        println!(
            "| {short} | {} | {} | {} | {} | {} | {} | {t} | {above} | {rated} |",
            p.confirmed.n,
            p.rejected.n,
            p.rated.n,
            median(&p.confirmed),
            median(&p.rejected),
            median(&p.rated),
        );
    }
    Ok(())
}

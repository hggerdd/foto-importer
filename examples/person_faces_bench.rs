//! Misst die Seiten „Wahrscheinlich X“ und „Ungeeignet“ an einer Sammlung.
//!
//! `cargo run --release --example person_faces_bench -- <Sammlungsordner> [Person-ID] [--abgleich]`
//!
//! Ohne `--abgleich` wird nur gelesen; gemessen wird dann der Weg ohne Zwischenspeicher (ein
//! Durchgang über alle Vektoren, wie direkt nach dem Programmstart). `--abgleich` führt vorher
//! den Abgleich aus (schreibt Vorschläge in die Sammlung, daher nur mit einer Kopie) und misst
//! danach den Weg über den Zwischenspeicher.
use foto_importer::core::{collection::Collection, faces, jobs::Control, recognition};
use std::{path::Path, time::Instant};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(root) = args.first() else {
        eprintln!(
            "Aufruf: cargo run --release --example person_faces_bench -- <Sammlungsordner> [Person-ID] [--abgleich]"
        );
        std::process::exit(2);
    };
    let matching = args.iter().any(|a| a == "--abgleich");
    let mut collection = Collection::open(Path::new(root), matching)?;
    let key = faces::setting(&collection, "rust.embedder_key")?.unwrap_or_default();
    let people = faces::people(&collection)?;
    let person = match args.get(1).and_then(|a| a.parse::<i64>().ok()) {
        Some(id) => id,
        None => people
            .iter()
            .max_by_key(|p| p.count)
            .map(|p| p.id)
            .ok_or_else(|| anyhow::anyhow!("keine Person"))?,
    };
    let name = people
        .iter()
        .find(|p| p.id == person)
        .map(|p| p.name.clone())
        .unwrap_or_default();
    let faces_total: i64 = collection
        .db
        .query_row("SELECT count(*) FROM faces", [], |r| r.get(0))?;
    println!("{faces_total} Gesichter, Person {person} ({name})");
    let control = Control::default();
    if matching {
        let started = Instant::now();
        recognition::rematch(&mut collection, &key, &control)?;
        println!(
            "Abgleich (baut den Zwischenspeicher): {:?}",
            started.elapsed()
        );
    }
    for round in 1..=3 {
        let started = Instant::now();
        let found = recognition::person_candidates(
            &collection,
            &key,
            person,
            foto_importer::config::PERSON_CANDIDATE_MIN,
            &control,
        )?;
        let candidates = started.elapsed();
        let others = found.iter().filter(|f| f.better.is_some()).count();
        let started = Instant::now();
        let page = faces::page_filtered(&collection, "probable", 0, Some(person))?;
        println!(
            "Runde {round}: {} wahrscheinliche Gesichter ({} am ehesten {name}, {others} eher andere) in {candidates:?}; Seite 1 ({} Gruppen) in {:?}",
            found.len(),
            found.len() - others,
            page.groups.len(),
            started.elapsed()
        );
    }
    let started = Instant::now();
    let page = faces::page(&collection, "unsuitable", 0)?;
    println!(
        "Ungeeignet: {} Gesichter, Seite 1 in {:?}",
        page.faces_total,
        started.elapsed()
    );
    Ok(())
}

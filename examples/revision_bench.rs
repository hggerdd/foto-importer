//! Kosten der Revisionstrigger (`data_revision`, ADR Sammlung und Daten) bei Massenarbeit.
//! Nur synthetische Daten in einer frischen Sammlung unter dem angegebenen Ordner.
//!
//! ```text
//! cargo run --release --example revision_bench -- tmp/bench-revision [anzahl] [runden]
//! ```
//!
//! Gemessen werden die Schreibmuster der Läufe: Bestand (Dateien in Stapeln zu 500),
//! Metadaten (eine `meta`-Zeile je Datei plus `meta_state`), Analyse (Gesichter ohne Person
//! plus Merkmale) und Abgleich (Zuordnung `auto` in Stapeln zu 512). Je Runde eine neue
//! Sammlung; ausgegeben wird der Median.
use foto_importer::core::collection::Collection;
use rusqlite::params;
use std::{path::Path, time::Instant};

const BATCH: i64 = 500;

fn chunks(total: i64, step: i64) -> impl Iterator<Item = (i64, i64)> {
    (1..=total)
        .step_by(step as usize)
        .map(move |start| (start, (start + step - 1).min(total)))
}

fn run(root: &Path, total: i64) -> anyhow::Result<[f64; 4]> {
    if root.exists() {
        std::fs::remove_dir_all(root)?;
    }
    std::fs::create_dir_all(root)?;
    let mut c = Collection::open(root, true)?;
    let mut times = [0.0; 4];

    let started = Instant::now();
    for (from, to) in chunks(total, BATCH) {
        let tx = c.db.transaction()?;
        {
            let mut photo = tx.prepare("INSERT INTO photos(id,status) VALUES(?,'present')")?;
            let mut file = tx.prepare("INSERT INTO files(id,relpath,relpath_fold,dir,name,stem,ext,size,mtime_ns,photo_id,status) VALUES(?,?,?,?,?,?,'jpg',12,1,?,'present')")?;
            for id in from..=to {
                let dir = format!("{}/Ordner {:04}", 2000 + id % 25, id % 2000);
                let name = format!("{id}.jpg");
                photo.execute([id])?;
                file.execute(params![
                    id,
                    format!("{dir}/{name}"),
                    "",
                    dir,
                    name,
                    name,
                    id
                ])?;
            }
        }
        tx.commit()?;
    }
    times[0] = started.elapsed().as_secs_f64();

    let started = Instant::now();
    for (from, to) in chunks(total, 128) {
        let tx = c.db.transaction()?;
        {
            let mut meta = tx.prepare(
                "INSERT INTO meta(file_id,model,dt_original,iso) VALUES(?,'Kamera','2024:01:01 10:00:00',100)",
            )?;
            let mut state = tx.prepare("UPDATE files SET meta_state='done' WHERE id=?")?;
            for id in from..=to {
                meta.execute([id])?;
                state.execute([id])?;
            }
        }
        tx.commit()?;
    }
    times[1] = started.elapsed().as_secs_f64();

    let started = Instant::now();
    let vector = vec![0u8; 512 * 4];
    for (from, to) in chunks(total, 32) {
        let tx = c.db.transaction()?;
        {
            let mut face = tx.prepare("INSERT INTO faces(id,photo_id,source_file_id,source_relpath,detector_id,bbox_x1,bbox_y1,bbox_x2,bbox_y2,det_score,quality) VALUES(?,?,?,'x.jpg','det',0.1,0.1,0.5,0.5,0.9,0.8)")?;
            let mut embedding = tx.prepare("INSERT INTO face_embeddings VALUES(?,'emb',512,?)")?;
            for id in from..=to {
                face.execute([id, id, id])?;
                embedding.execute(params![id, vector])?;
            }
        }
        tx.commit()?;
    }
    times[2] = started.elapsed().as_secs_f64();

    c.db.execute_batch("INSERT INTO persons(id) VALUES(1)")?;
    let started = Instant::now();
    for (from, to) in chunks(total, 512) {
        let tx = c.db.transaction()?;
        {
            let mut assign = tx.prepare("UPDATE faces SET person_id=1,assignment='auto',confidence=0.7,assigned_at=datetime('now') WHERE id=?")?;
            for id in from..=to {
                assign.execute([id])?;
            }
        }
        tx.commit()?;
    }
    times[3] = started.elapsed().as_secs_f64();
    drop(c);
    std::fs::remove_dir_all(root)?;
    Ok(times)
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let root = Path::new(
        args.get(1)
            .map(String::as_str)
            .unwrap_or("tmp/bench-revision"),
    );
    let total: i64 = args.get(2).and_then(|v| v.parse().ok()).unwrap_or(100_000);
    let rounds: usize = args.get(3).and_then(|v| v.parse().ok()).unwrap_or(3);
    let mut all: Vec<[f64; 4]> = Vec::new();
    for round in 0..rounds {
        let times = run(root, total)?;
        eprintln!("Runde {}: {times:.2?}", round + 1);
        all.push(times);
    }
    for (i, label) in ["Bestand", "Metadaten", "Analyse", "Abgleich"]
        .iter()
        .enumerate()
    {
        let mut values: Vec<f64> = all.iter().map(|t| t[i]).collect();
        values.sort_by(f64::total_cmp);
        println!(
            "{label:<10} {total} Zeilen: Median {:.2} s",
            values[values.len() / 2]
        );
    }
    Ok(())
}

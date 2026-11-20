//! Messung der Datensicherung (`core::backup`) an einer synthetischen Sammlungsdatenbank.
//!
//! `cargo run --release --example backup_bench -- <leerer Ordner> [MB]`
//!
//! Legt im Ordner eine Sammlung an, füllt sie auf etwa `MB` (Standard 800) mit Zeilen ähnlich
//! einer echten Sammlung (Gesichtsvektoren mit 128 float32, Metadaten-JSON, Dateizeilen), misst
//! `backup::create` mit der eingestellten Stufe und zum Vergleich das reine Packen der
//! Datenbank mit Deflate-Stufe 1, 6 und 9. Nie auf eine echte Sammlung richten.
use anyhow::{Result, bail};
use foto_importer::core::{backup, collection::Collection, jobs::Control};
use std::{
    io::{Read, Write},
    path::PathBuf,
    time::Instant,
};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(args.next().unwrap_or_else(|| "tmp/backup-bench".into()));
    let target_mb: u64 = args.next().map(|a| a.parse()).transpose()?.unwrap_or(800);
    std::fs::create_dir_all(&root)?;
    if std::fs::read_dir(&root)?.next().is_some() && !root.join(".myfotomanager").is_file() {
        bail!("Ordner ist nicht leer und keine Bench-Sammlung");
    }
    let database = root.join(".myfotomanager");
    if !database.is_file() {
        let started = Instant::now();
        let c = Collection::open(&root, true)?;
        c.db.execute_batch(
            "CREATE TABLE bench_vectors(id INTEGER PRIMARY KEY, vec BLOB);
             CREATE TABLE bench_meta(id INTEGER PRIMARY KEY, json TEXT);",
        )?;
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let mut id = 0i64;
        while std::fs::metadata(&database)?.len() < target_mb * 1024 * 1024 {
            let tx = c.db.unchecked_transaction()?;
            for _ in 0..5000 {
                id += 1;
                // Normierte Vektoren: float32 um 0 mit kleiner Streuung wie bei SFace.
                let vec: Vec<u8> = (0..128)
                    .flat_map(|_| (((next() % 20000) as f32 - 10000.0) / 120000.0).to_le_bytes())
                    .collect();
                tx.execute(
                    "INSERT INTO bench_vectors VALUES(?,?)",
                    rusqlite::params![id, vec],
                )?;
                let json = format!(
                    r#"{{"SourceFile":"2019/2019-07-{:02}/nef/DSC_{:05}.NEF","Make":"NIKON CORPORATION","Model":"NIKON D750","LensModel":"24.0-120.0 mm f/4.0","ISO":{},"ExposureTime":"1/{}","FNumber":{}.0,"DateTimeOriginal":"2019:07:{:02} 1{}:{:02}:{:02}","ImageWidth":6016,"ImageHeight":4016,"SerialNumber":"60{:05}","ShutterCount":{}}}"#,
                    id % 28 + 1,
                    id % 99999,
                    100 * (id % 64 + 1),
                    id % 4000 + 1,
                    id % 16 + 2,
                    id % 28 + 1,
                    id % 10,
                    id % 60,
                    id % 60,
                    next() % 99999,
                    id
                );
                tx.execute(
                    "INSERT INTO bench_meta VALUES(?,?)",
                    rusqlite::params![id, json],
                )?;
                tx.execute(
                    "INSERT INTO settings VALUES(?,?)",
                    rusqlite::params![format!("bench-{id}"), format!("wert-{}", next() % 1000)],
                )?;
            }
            tx.commit()?;
        }
        c.db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
        println!(
            "angelegt: {} in {:.1} s",
            backup::megabytes(std::fs::metadata(&database)?.len()),
            started.elapsed().as_secs_f64()
        );
    }
    let source = std::fs::metadata(&database)?.len();
    for run in 1..=2 {
        let started = Instant::now();
        let control = Control::default();
        let created = backup::create(&root, &backup::Plan::default(), "bench", &control)?;
        let phases: Vec<String> = control
            .timings()
            .into_iter()
            .map(|(name, s)| format!("{name} {s:.1} s"))
            .collect();
        println!(
            "lauf {run}: Datenbank {} -> ZIP {} ({:.0} %) in {:.1} s, Stufe {} [{}]",
            backup::megabytes(source),
            backup::megabytes(created.bytes),
            created.bytes as f64 * 100.0 / source as f64,
            started.elapsed().as_secs_f64(),
            foto_importer::config::BACKUP_COMPRESSION_LEVEL,
            phases.join(", ")
        );
        std::fs::remove_file(&created.path)?;
    }
    // Reines Packen der Datenbankdatei mit verschiedenen Stufen (ohne Online-Kopie).
    for level in [1, 6, 9] {
        let started = Instant::now();
        let out = tempfile::tempfile_in(&root)?;
        let mut zip = zip::ZipWriter::new(std::io::BufWriter::new(out));
        zip.start_file(
            "db",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .compression_level(Some(level)),
        )?;
        let mut input = std::fs::File::open(&database)?;
        let mut buffer = vec![0u8; 1 << 20];
        loop {
            let read = input.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            zip.write_all(&buffer[..read])?;
        }
        let bytes = zip.finish()?.into_inner()?.metadata()?.len();
        println!(
            "nur packen, Stufe {level}: {} ({:.0} %) in {:.1} s",
            backup::megabytes(bytes),
            bytes as f64 * 100.0 / source as f64,
            started.elapsed().as_secs_f64()
        );
    }
    Ok(())
}

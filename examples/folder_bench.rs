//! Messreihe für das Laden des kompletten Ordnerbaums (ADR 0067) und die Kosten der
//! `folder_counts`-Trigger. Nur synthetische Daten.
//!
//! ```text
//! cargo run --release --example folder_bench -- tmp/bench-folders [fotos] [ordner]
//! ```
use foto_importer::core::{collection::Collection, folders, migrations};
use rusqlite::{Connection, params};
use std::{
    path::Path,
    time::{Duration, Instant},
};

const CHANGES: i64 = 10_000;

type Work = dyn Fn(&Connection) -> anyhow::Result<()>;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let root = Path::new(
        args.get(1)
            .map(String::as_str)
            .unwrap_or("tmp/bench-folders"),
    );
    let photos: i64 = args.get(2).and_then(|v| v.parse().ok()).unwrap_or(100_000);
    let folders_total: i64 = args.get(3).and_then(|v| v.parse().ok()).unwrap_or(20_000);
    std::fs::create_dir_all(root)?;
    let mut c = Collection::open(root, true)?;
    let have: i64 =
        c.db.query_row("SELECT count(*) FROM files", [], |r| r.get(0))?;
    if have == 0 {
        let started = Instant::now();
        let tx = c.db.transaction()?;
        {
            let mut photo = tx.prepare("INSERT INTO photos(id,status) VALUES(?,'present')")?;
            let mut file = tx.prepare("INSERT INTO files(id,relpath,relpath_fold,dir,name,stem,ext,size,mtime_ns,photo_id,status) VALUES(?,?,?,?,?,?,'jpg',12,1,?,'present')")?;
            for id in 1..=photos {
                let folder = id % folders_total;
                let dir = format!("{}/Ordner {folder:05}/Unter {}", 2000 + folder % 25, id % 3);
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
        eprintln!("Aufbau {photos} Fotos: {:?}", started.elapsed());
    }
    let time = |db: &Connection, label: &str, sql: &str| -> anyhow::Result<()> {
        let started = Instant::now();
        let mut st = db.prepare(sql)?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        let n = rows.count();
        eprintln!("{label}: {n} Zeilen in {:?}", started.elapsed());
        Ok(())
    };
    let join = "SELECT f.dir,count(*) FROM files f INDEXED BY ix_files_directory_name JOIN metadata_photos p ON p.photo_id=f.photo_id AND p.file_id=f.id GROUP BY f.dir";
    time(&c.db, "A Join ueber ix_files_directory_name", join)?;
    time(&c.db, "A (warm)", join)?;
    let cover = "SELECT f.dir,count(*) FROM files f INDEXED BY ix_files_dir_cover JOIN metadata_photos p ON p.photo_id=f.photo_id AND p.file_id=f.id GROUP BY f.dir";
    time(&c.db, "B Join ueber Abdeckungsindex", cover)?;
    time(&c.db, "B (warm)", cover)?;
    let only = "SELECT rtrim(rtrim(relpath,replace(relpath,'/','')),'/'),count(*) FROM metadata_photos GROUP BY 1";
    time(&c.db, "C nur metadata_photos", only)?;
    time(&c.db, "C (warm)", only)?;
    let table = "SELECT dir,photos FROM folder_counts";
    time(&c.db, "D folder_counts", table)?;
    time(&c.db, "D (warm)", table)?;
    {
        // Wie der Worker: frische, lesende Verbindung mit leerem Seitencache.
        let read = Collection::open(root, false)?;
        let started = Instant::now();
        let n = folders::directories(&read)?.len();
        eprintln!(
            "D directories() auf neuer Leseverbindung: {n} Ordner in {:?}",
            started.elapsed()
        );
    }
    let pages: i64 =
        c.db.query_row(
            "SELECT count(*) FROM dbstat WHERE name='folder_counts'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(-1);
    eprintln!("folder_counts: {pages} Seiten (-1: dbstat nicht verfügbar)");

    // Erstbefüllung wie beim ersten schreibenden Öffnen einer älteren Sammlung.
    c.db.execute_batch("BEGIN")?;
    for (name, _) in &migrations::FOLDER_COUNTS[1..] {
        c.db.execute_batch(&format!("DROP TRIGGER {name}"))?;
    }
    c.db.execute_batch("DROP TABLE folder_counts; COMMIT")?;
    let started = Instant::now();
    migrations::ensure_folder_counts(&c.db)?;
    eprintln!(
        "Erstbefüllung (Tabelle, Trigger, GROUP BY): {:?}",
        started.elapsed()
    );

    // Trigger-Kosten: dieselbe Arbeit mit und ohne folder_counts-Trigger, jeweils in einer
    // Transaktion, die danach zurückgerollt wird. Bester von drei Läufen.
    let quick_scan = move |db: &Connection| -> anyhow::Result<()> {
        let mut photo = db.prepare_cached("INSERT INTO photos(id,status) VALUES(?,'present')")?;
        let mut file = db.prepare_cached("INSERT INTO files(id,relpath,relpath_fold,dir,name,stem,ext,size,mtime_ns,photo_id,status) VALUES(?,?,?,?,?,?,'jpg',12,1,?,'present')")?;
        for offset in 1..=CHANGES {
            let id = photos + offset;
            // Teils neue, teils vorhandene Ordner.
            let dir = format!("2026/Import {:03}", offset % 200);
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
        Ok(())
    };
    let meta_updates = move |db: &Connection| -> anyhow::Result<()> {
        let mut meta = db.prepare_cached(
            "INSERT INTO meta(file_id,model,dt_original) VALUES(?,'Kamera','2024:01:01 10:00:00')",
        )?;
        let mut done = db.prepare_cached("UPDATE files SET meta_state='done' WHERE id=?")?;
        for id in 1..=CHANGES.min(photos) {
            meta.execute([id])?;
            done.execute([id])?;
        }
        Ok(())
    };
    let rebuild = |db: &Connection| -> anyhow::Result<()> {
        migrations::rebuild_metadata_photos(db)?;
        Ok(())
    };
    let work: [(&str, &Work); 3] = [
        ("Schnellscan 10 000 neue Dateien", &quick_scan),
        ("10 000 Metadaten-Updates", &meta_updates),
        ("Neuaufbau metadata_photos (Migration)", &rebuild),
    ];
    for (label, run) in work {
        let mut best = [Duration::MAX; 2];
        for _ in 0..3 {
            for (slot, with) in [(0, true), (1, false)] {
                c.db.execute_batch("BEGIN")?;
                if !with {
                    for (name, _) in &migrations::FOLDER_COUNTS[1..] {
                        c.db.execute_batch(&format!("DROP TRIGGER {name}"))?;
                    }
                }
                let started = Instant::now();
                let result = run(&c.db);
                let elapsed = started.elapsed();
                c.db.execute_batch("ROLLBACK")?;
                result?;
                best[slot] = best[slot].min(elapsed);
            }
        }
        eprintln!(
            "{label}: mit Triggern {:?}, ohne {:?} (+{:.1} %)",
            best[0],
            best[1],
            (best[0].as_secs_f64() / best[1].as_secs_f64() - 1.0) * 100.0
        );
    }
    Ok(())
}

//! Messreihe für das Laden des kompletten Ordnerbaums (ADR 0067). Nur synthetische Daten.
//!
//! ```text
//! cargo run --release --example folder_bench -- tmp/bench-folders [fotos] [ordner]
//! ```
use foto_importer::core::collection::Collection;
use rusqlite::params;
use std::{path::Path, time::Instant};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let root = Path::new(
        args.get(1)
            .map(String::as_str)
            .unwrap_or("tmp/bench-folders"),
    );
    let photos: i64 = args.get(2).and_then(|v| v.parse().ok()).unwrap_or(100_000);
    let folders: i64 = args.get(3).and_then(|v| v.parse().ok()).unwrap_or(20_000);
    std::fs::create_dir_all(root)?;
    let mut c = Collection::open(root, true)?;
    let have: i64 =
        c.db.query_row("SELECT count(*) FROM files", [], |r| r.get(0))?;
    if have == 0 {
        let tx = c.db.transaction()?;
        {
            let mut photo = tx.prepare("INSERT INTO photos(id,status) VALUES(?,'present')")?;
            let mut file = tx.prepare("INSERT INTO files(id,relpath,relpath_fold,dir,name,stem,ext,size,mtime_ns,photo_id,status) VALUES(?,?,?,?,?,?,'jpg',12,1,?,'present')")?;
            for id in 1..=photos {
                let folder = id % folders;
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
    }
    let time = |label: &str, sql: &str| -> anyhow::Result<()> {
        let started = Instant::now();
        let mut st = c.db.prepare(sql)?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        let n = rows.count();
        eprintln!("{label}: {n} Zeilen in {:?}", started.elapsed());
        Ok(())
    };
    let join = "SELECT f.dir,count(*) FROM files f INDEXED BY ix_files_directory_name JOIN metadata_photos p ON p.photo_id=f.photo_id AND p.file_id=f.id GROUP BY f.dir";
    time("A Join ueber ix_files_directory_name", join)?;
    time("A (warm)", join)?;
    c.db.execute_batch("CREATE INDEX IF NOT EXISTS ix_files_dir_cover ON files(dir,photo_id,id)")?;
    let cover = "SELECT f.dir,count(*) FROM files f INDEXED BY ix_files_dir_cover JOIN metadata_photos p ON p.photo_id=f.photo_id AND p.file_id=f.id GROUP BY f.dir";
    time("B Join ueber Abdeckungsindex", cover)?;
    time("B (warm)", cover)?;
    let only =
        "SELECT rtrim(relpath,replace(relpath,'/','')),count(*) FROM metadata_photos GROUP BY 1";
    time("C nur metadata_photos", only)?;
    time("C (warm)", only)?;
    Ok(())
}

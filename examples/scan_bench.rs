//! Reproducible index benchmark on a synthetic tree (never use private collections).
//!
//! `cargo run --release --example scan_bench -- [files] [dirs]` (defaults 30000 / 300)
//! Environment: `SCAN_BENCH_DIR` keeps the tree in that folder instead of a temp dir.
use foto_importer::core::{collection::Collection, files, jobs::Control, progress::Progress};
use std::{path::Path, time::Instant};

fn build(root: &Path, files: usize, dirs: usize) -> anyhow::Result<()> {
    for index in 0..files {
        let folder = root
            .join(format!("Jahr{}", index % dirs / 50))
            .join(format!("Ordner{:04}", index % dirs));
        std::fs::create_dir_all(&folder)?;
        // Every tenth photo has a RAW sibling of the same stem (variant pairing).
        let stem = format!("IMG_{index:06}");
        std::fs::write(folder.join(format!("{stem}.jpg")), format!("jpeg {index}"))?;
        if index % 10 == 0 {
            std::fs::write(folder.join(format!("{stem}.nef")), format!("raw {index}"))?;
        }
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    if std::env::args().nth(1).as_deref() == Some("dump") {
        // `scan_bench dump <collection>`: print files and photos for before/after comparisons.
        let collection = Collection::open(
            Path::new(&std::env::args().nth(2).unwrap_or_default()),
            false,
        )?;
        let mut statement = collection.db.prepare(
            "SELECT id,relpath,photo_id,status,size,ext,meta_state FROM files ORDER BY id",
        )?;
        for row in statement.query_map([], |r| {
            Ok(format!(
                "file {} {} photo={} {} {} {} {}",
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?
            ))
        })? {
            println!("{}", row?);
        }
        let mut statement = collection
            .db
            .prepare("SELECT id,status FROM photos ORDER BY id")?;
        for row in statement.query_map([], |r| {
            Ok(format!(
                "photo {} {}",
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?
            ))
        })? {
            println!("{}", row?);
        }
        return Ok(());
    }
    let count: usize = args.next().and_then(|v| v.parse().ok()).unwrap_or(30_000);
    let dirs: usize = args.next().and_then(|v| v.parse().ok()).unwrap_or(300);
    let temp;
    let root = match std::env::var("SCAN_BENCH_DIR") {
        Ok(dir) => Path::new(&dir).to_path_buf(),
        Err(_) => {
            temp = tempfile::tempdir()?;
            temp.path().to_path_buf()
        }
    };
    let started = Instant::now();
    build(&root, count, dirs)?;
    println!(
        "tree: {count} photos in {dirs} folders, built in {:.1} s",
        started.elapsed().as_secs_f64()
    );

    // First touch of freshly written files: stat only versus opening for read (different files).
    {
        let walk = files::scan_filtered(
            &root,
            false,
            &["jpg".to_string()],
            &[],
            true,
            &Control::default(),
        )?;
        let half = walk.files.len() / 2;
        let started = Instant::now();
        for file in &walk.files[..half] {
            file.path.metadata()?;
        }
        println!(
            "first touch stat only:  {:8.1} us/file",
            started.elapsed().as_secs_f64() * 1e6 / half as f64
        );
        let started = Instant::now();
        for file in &walk.files[half..] {
            std::fs::File::open(&file.path)?;
        }
        println!(
            "first touch open (read): {:8.1} us/file",
            started.elapsed().as_secs_f64() * 1e6 / (walk.files.len() - half) as f64
        );
    }
    let extensions: Vec<String> = ["jpg", "nef", "xmp"].map(String::from).to_vec();
    let started = Instant::now();
    let walk = files::scan_filtered(&root, false, &extensions, &[], true, &Control::default())?;
    println!(
        "walk + stat (no database): {} files, {:.2} s",
        walk.files.len(),
        started.elapsed().as_secs_f64()
    );

    let mut collection = Collection::open(&root, true)?;
    let control = Control::default();
    let started = Instant::now();
    let message = collection.quick_scan(false, &control)?;
    println!(
        "first index: {:.2} s · {message}",
        started.elapsed().as_secs_f64()
    );
    for (name, seconds) in control.timings() {
        println!("  phase {name}: {seconds:.2} s");
    }
    let progress: Progress = control.snapshot();
    println!("  final progress: {}", progress.describe());

    let control = Control::default();
    let started = Instant::now();
    collection.quick_scan(false, &control)?;
    println!(
        "second index (unchanged): {:.2} s",
        started.elapsed().as_secs_f64()
    );
    for (name, seconds) in control.timings() {
        println!("  phase {name}: {seconds:.2} s");
    }

    if let Ok(sizes) = std::env::var("SCAN_BENCH_BATCHES") {
        // Batch size sweep: SCAN_BENCH_BATCHES=1,50,500,5000 re-indexes the tree from scratch.
        drop(collection);
        for batch in sizes
            .split(',')
            .filter_map(|v| v.trim().parse::<usize>().ok())
        {
            for name in [
                ".myfotomanager",
                ".myfotomanager-wal",
                ".myfotomanager-shm",
                ".myfotomanager.backup",
            ] {
                let _ = std::fs::remove_file(root.join(name));
            }
            let mut fresh = Collection::open(&root, true)?;
            let started = Instant::now();
            fresh.quick_scan_batched(false, &Control::default(), batch)?;
            println!(
                "batch {batch:>6}: first index {:.2} s",
                started.elapsed().as_secs_f64()
            );
        }
        return Ok(());
    }
    micro_costs(&root, &collection, count.min(2000))?;

    // Trigger cost in isolation: the same rows inserted into a copy with and without triggers.
    let triggers: Vec<String> = {
        let mut stmt = collection.db.prepare(
            "SELECT name FROM sqlite_master WHERE type='trigger' AND name LIKE 'browse_files_%'",
        )?;
        stmt.query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    for with_triggers in [true, false] {
        let db = rusqlite::Connection::open_in_memory()?;
        db.execute_batch(include_str!("../src/core/schema.sql"))?;
        db.execute("INSERT INTO projection_revision VALUES(0)", [])?;
        if !with_triggers {
            for name in &triggers {
                db.execute_batch(&format!("DROP TRIGGER {name}"))?;
            }
        }
        let started = Instant::now();
        db.execute_batch("BEGIN")?;
        for index in 0..count {
            db.execute("INSERT INTO photos(status) VALUES('present')", [])?;
            let photo = db.last_insert_rowid();
            let name = format!("IMG_{index:06}.jpg");
            let relpath = format!("Jahr0/Ordner{:04}/{name}", index % dirs);
            db.execute(
                "INSERT INTO files(relpath,relpath_fold,dir,name,stem,ext,size,mtime_ns,photo_id,status) VALUES(?,?,?,?,?,'jpg',10,1,?,'present')",
                rusqlite::params![relpath, relpath, "d", name, format!("IMG_{index:06}"), photo],
            )?;
        }
        db.execute_batch("COMMIT")?;
        println!(
            "{count} inserts in memory {}: {:.2} s",
            if with_triggers {
                "with triggers"
            } else {
                "without triggers"
            },
            started.elapsed().as_secs_f64()
        );
    }
    Ok(())
}

/// Per-operation costs on a copy of the populated inventory, in microseconds per call.
fn micro_costs(root: &Path, collection: &Collection, samples: usize) -> anyhow::Result<()> {
    let copy = tempfile::NamedTempFile::new()?;
    collection.db.backup("main", copy.path(), None)?;
    let db = rusqlite::Connection::open(copy.path())?;
    db.create_scalar_function(
        "casefold",
        1,
        rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            Ok(files::fold(
                &ctx.get::<Option<String>>(0)?.unwrap_or_default(),
            ))
        },
    )?;
    let rows: Vec<(String, String, i64)> = db
        .prepare("SELECT relpath,stem,size FROM files LIMIT ?")?
        .query_map([samples as i64], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let per = |started: Instant| started.elapsed().as_secs_f64() * 1e6 / rows.len().max(1) as f64;
    let started = Instant::now();
    for row in &rows {
        db.query_row("SELECT id FROM files WHERE relpath=?", [&row.0], |r| {
            r.get::<_, i64>(0)
        })?;
    }
    println!(
        "micro (n={}): lookup by relpath (indexed)      {:8.1} us",
        rows.len(),
        per(started)
    );
    let started = Instant::now();
    for row in &rows {
        db.prepare("SELECT id FROM files WHERE inode=? AND size=?")?
            .query_map(rusqlite::params!["1", row.2], |r| r.get::<_, i64>(0))?
            .count();
    }
    println!(
        "micro: candidates by inode+size (full scan)    {:8.1} us",
        per(started)
    );
    let started = Instant::now();
    for row in &rows {
        db.prepare("SELECT id FROM files WHERE size=? AND full_hash IS NOT NULL")?
            .query_map([row.2], |r| r.get::<_, i64>(0))?
            .count();
    }
    println!(
        "micro: candidates by size+hash (full scan)     {:8.1} us",
        per(started)
    );
    let started = Instant::now();
    for row in &rows {
        db.prepare("SELECT photo_id,dir FROM files WHERE casefold(stem)=? AND photo_id IS NOT NULL ORDER BY id")?
            .query_map([files::fold(&row.1)], |r| r.get::<_, i64>(0))?
            .count();
    }
    println!(
        "micro: family partner by casefold(stem) (scan) {:8.1} us",
        per(started)
    );
    let started = Instant::now();
    for row in &rows {
        std::fs::File::open(root.join(&row.0))?;
    }
    println!(
        "micro: open file (file id)                     {:8.1} us",
        per(started)
    );
    let started = Instant::now();
    for row in &rows {
        root.join(&row.0).metadata()?;
    }
    println!(
        "micro: stat file                               {:8.1} us",
        per(started)
    );
    // Insert one photo and file per sample, statements prepared on every call as before.
    let started = Instant::now();
    db.execute_batch("BEGIN")?;
    for (index, row) in rows.iter().enumerate() {
        db.execute(
            "INSERT INTO photos(status,created_scan) VALUES('present',1)",
            [],
        )?;
        let photo = db.last_insert_rowid();
        let relpath = format!("x/{index}-{}", row.0);
        db.execute(
            "INSERT INTO files(relpath,relpath_fold,dir,name,stem,ext,size,mtime_ns,inode,photo_id,status,first_seen_scan,last_seen_scan) VALUES(?,?,?,?,?,?,?,?,?,?,'present',1,1)",
            rusqlite::params![relpath, relpath, "x", "n", "s", "jpg", 1, 1, "9", photo],
        )?;
    }
    println!(
        "micro: insert photo+file incl. triggers        {:8.1} us",
        per(started)
    );
    db.execute_batch("ROLLBACK")?;
    let started = Instant::now();
    db.execute_batch("BEGIN")?;
    for row in &rows {
        db.execute(
            "UPDATE files SET relpath=?,relpath_fold=?,dir=dir,name=name,stem=stem,ext=ext,size=size,mtime_ns=mtime_ns,inode=inode,last_seen_scan=2,status='present' WHERE relpath=?",
            rusqlite::params![row.0, row.0, row.0],
        )?;
    }
    println!(
        "micro: full UPDATE of unchanged row (trigger)  {:8.1} us",
        per(started)
    );
    db.execute_batch("ROLLBACK")?;
    let started = Instant::now();
    db.execute_batch("BEGIN")?;
    for row in &rows {
        db.execute(
            "UPDATE files SET last_seen_scan=2 WHERE relpath=?",
            [&row.0],
        )?;
    }
    println!(
        "micro: UPDATE last_seen_scan only (no trigger) {:8.1} us",
        per(started)
    );
    db.execute_batch("ROLLBACK")?;
    let started = Instant::now();
    collection.backup()?;
    println!(
        "backup of the whole database: {:.2} s",
        started.elapsed().as_secs_f64()
    );
    Ok(())
}

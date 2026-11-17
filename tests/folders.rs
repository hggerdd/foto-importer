use foto_importer::core::{
    collection::Collection,
    folders::{self, FolderTree, Row},
    metadata::{self, Search},
    migrations,
};
use rusqlite::params;
use std::time::Instant;

fn add(c: &Collection, id: i64, relpath: &str, ext: &str, photo: i64) -> anyhow::Result<()> {
    let dir = relpath.rsplit_once('/').map(|v| v.0).unwrap_or("");
    let name = relpath.rsplit('/').next().unwrap_or(relpath);
    c.db.execute(
        "INSERT OR IGNORE INTO photos(id,status) VALUES(?,'present')",
        [photo],
    )?;
    c.db.execute("INSERT INTO files(id,relpath,relpath_fold,dir,name,stem,ext,size,mtime_ns,photo_id,status) VALUES(?,?,?,?,?,?,?,12,1,?,'present')",params![id,relpath,relpath.to_lowercase(),dir,name,name,ext,photo])?;
    Ok(())
}

fn names(tree: &FolderTree) -> Vec<(String, usize)> {
    tree.rows()
        .into_iter()
        .filter_map(|row| match row {
            Row::Node { path, photos, .. } => Some((path, photos)),
            _ => None,
        })
        .collect()
}

fn load(c: &Collection) -> anyhow::Result<FolderTree> {
    let mut tree = FolderTree::default();
    tree.begin();
    tree.apply(&folders::directories(c)?);
    Ok(tree)
}

#[test]
fn the_tree_counts_photos_below_each_folder_and_sorts_like_the_explorer() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let c = Collection::open(temp.path(), true)?;
    add(&c, 1, "root.jpg", "jpg", 1)?;
    add(&c, 2, "2024/a.jpg", "jpg", 2)?;
    add(&c, 3, "2024/05/b.jpg", "jpg", 3)?;
    add(&c, 4, "2024/05/c.jpg", "jpg", 4)?;
    add(&c, 5, "2024-x/d.jpg", "jpg", 5)?;
    add(&c, 6, "2024 und mehr/e.jpg", "jpg", 6)?;
    add(&c, 7, "Zebra/f.jpg", "jpg", 7)?;
    add(&c, 11, "_Sortiert/h.jpg", "jpg", 9)?;
    add(&c, 12, "10 Urlaub/i.jpg", "jpg", 10)?;
    add(&c, 13, "2 Urlaub/j.jpg", "jpg", 11)?;
    add(&c, 14, "apfel/k.jpg", "jpg", 12)?;
    // A RAW+JPG pair counts once, a sidecar not at all.
    add(&c, 8, "2024/05/g.nef", "nef", 8)?;
    add(&c, 9, "2024/05/g.jpg", "jpg", 8)?;
    add(&c, 10, "2024/05/g.xmp", "xmp", 8)?;
    let mut tree = load(&c)?;
    tree.expand("2024");
    let rows = names(&tree);
    assert_eq!(
        rows,
        [
            ("_Sortiert".to_string(), 1),
            ("2 Urlaub".to_string(), 1),
            ("10 Urlaub".to_string(), 1),
            ("2024".to_string(), 4),
            ("2024/05".to_string(), 3),
            ("2024 und mehr".to_string(), 1),
            ("2024-x".to_string(), 1),
            ("apfel".to_string(), 1),
            ("Zebra".to_string(), 1),
        ]
    );
    // Counts equal the gallery total after choosing the folder.
    for (folder, expected) in [("2024", 4), ("2024/05", 3), ("2024-x", 1)] {
        let filter = Search {
            folder: Some(folder.into()),
            ..Default::default()
        };
        assert_eq!(
            metadata::search(&c, &filter, "")?.total,
            expected,
            "{folder}"
        );
    }
    // The summary table, the join on the covering index and the scan without the additive
    // index (read-only collection of an older version) give the same result.
    let summarized = sorted(folders::directories(&c)?);
    assert_eq!(summarized, sorted(folders::scan_directories(&c)?));
    c.db.execute_batch("DROP INDEX ix_files_dir_cover")?;
    assert_eq!(summarized, sorted(folders::scan_directories(&c)?));
    Ok(())
}

fn sorted(mut list: Vec<(String, usize)>) -> Vec<(String, usize)> {
    list.sort();
    list
}

/// Photos per folder computed from the relative paths in `metadata_photos` alone.
fn by_relpath(c: &Collection) -> anyhow::Result<Vec<(String, usize)>> {
    let mut st = c.db.prepare(
        "SELECT rtrim(rtrim(relpath,replace(relpath,'/','')),'/'),count(*) FROM metadata_photos GROUP BY 1",
    )?;
    let rows = st.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize))
    })?;
    Ok(sorted(rows.collect::<rusqlite::Result<_>>()?))
}

/// `folder_counts` equals the join over `files.dir` that the tree used before, and the folder
/// formula on `relpath` equals `files.dir` of every preferred file.
fn assert_consistent(c: &Collection, step: &str) -> anyhow::Result<()> {
    let table = sorted(folders::directories(c)?);
    assert_eq!(table, sorted(folders::scan_directories(c)?), "{step}: join");
    assert_eq!(table, by_relpath(c)?, "{step}: relpath");
    let mismatched: i64 = c.db.query_row(
        "SELECT count(*) FROM metadata_photos p JOIN files f ON f.id=p.file_id WHERE f.dir IS NOT rtrim(rtrim(p.relpath,replace(p.relpath,'/','')),'/')",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(mismatched, 0, "{step}: files.dir");
    let empty: i64 = c.db.query_row(
        "SELECT count(*) FROM folder_counts WHERE photos<=0",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(empty, 0, "{step}: empty folders are removed");
    Ok(())
}

fn exists(root: &std::path::Path, name: &str) -> anyhow::Result<bool> {
    let db = rusqlite::Connection::open(root.join(".myfotomanager"))?;
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name=?)",
        [name],
        |r| r.get(0),
    )?)
}

#[test]
fn folder_counts_follow_every_change_of_the_projection() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let c = Collection::open(temp.path(), true)?;
    assert_consistent(&c, "empty")?;
    add(&c, 1, "root.jpg", "jpg", 1)?;
    add(&c, 2, "Urlaub 2024/IMG 1.final.jpg", "jpg", 2)?;
    add(&c, 3, "a.b/c d.e/x.y.z.jpg", "jpg", 3)?;
    add(&c, 4, "a.b/c d.e/.versteckt", "jpg", 4)?;
    add(&c, 5, "2024/05/g.nef", "nef", 5)?;
    add(&c, 6, "2024/05/g.jpg", "jpg", 5)?;
    add(&c, 7, "2024/05/g.xmp", "xmp", 5)?;
    // RAW and JPG of one photo in different folders: the NEF is preferred first.
    add(&c, 8, "raw/p.nef", "nef", 8)?;
    add(&c, 9, "jpg/p.jpg", "jpg", 8)?;
    assert_consistent(&c, "inserts")?;
    assert!(folders::directories(&c)?.contains(&("raw".into(), 1)));
    // Metadata make the JPG the preferred variant: the photo moves to the other folder.
    c.db.execute_batch(
        "INSERT INTO meta(file_id,dt_original) VALUES(9,'2024:01:01 10:00:00');
         UPDATE files SET meta_state='done' WHERE id=9;",
    )?;
    assert_consistent(&c, "meta insert")?;
    assert!(folders::directories(&c)?.contains(&("jpg".into(), 1)));
    c.db.execute_batch(
        "UPDATE meta SET dt_original=NULL WHERE file_id=9;
         UPDATE files SET meta_state='pending' WHERE id=9;",
    )?;
    assert_consistent(&c, "meta update")?;
    c.db.execute("DELETE FROM meta WHERE file_id=9", [])?;
    assert_consistent(&c, "meta delete")?;
    // Moves (a renamed folder), missing and deleted files, merged photos.
    c.db.execute(
        "UPDATE files SET relpath='neu/x.y.z.jpg',relpath_fold='neu/x.y.z.jpg',dir='neu' WHERE id=3",
        [],
    )?;
    assert_consistent(&c, "move")?;
    c.db.execute("UPDATE files SET status='missing' WHERE id=1", [])?;
    assert_consistent(&c, "missing")?;
    assert!(
        !folders::directories(&c)?
            .iter()
            .any(|(dir, _)| dir.is_empty())
    );
    c.db.execute("UPDATE files SET status='present' WHERE id=1", [])?;
    assert_consistent(&c, "back again")?;
    c.db.execute("DELETE FROM files WHERE id IN (5,6,7)", [])?;
    assert_consistent(&c, "delete")?;
    c.db.execute("UPDATE files SET photo_id=2 WHERE id=4", [])?;
    assert_consistent(&c, "merge")?;
    // In-place update of the projection (not done by the app today, kept right anyway).
    c.db.execute(
        "UPDATE metadata_photos SET relpath='anders/q.jpg' WHERE photo_id=8",
        [],
    )?;
    assert_eq!(sorted(folders::directories(&c)?), by_relpath(&c)?, "update");
    // The bulk rebuild of the migration repairs even a damaged summary.
    c.db.execute("UPDATE folder_counts SET photos=99", [])?;
    c.db.execute_batch("BEGIN")?;
    migrations::rebuild_metadata_photos(&c.db)?;
    c.db.execute_batch("COMMIT")?;
    assert_consistent(&c, "rebuild")?;
    add(&c, 20, "nach dem Neuaufbau/n.jpg", "jpg", 20)?;
    assert_consistent(&c, "triggers active again")?;
    assert!(folders::directories(&c)?.contains(&("nach dem Neuaufbau".into(), 1)));
    Ok(())
}

#[test]
fn folder_counts_are_added_on_write_open_and_read_only_open_falls_back() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let c = Collection::open(temp.path(), true)?;
    add(&c, 1, "root.jpg", "jpg", 1)?;
    add(&c, 2, "2024/a.jpg", "jpg", 2)?;
    add(&c, 3, "2024/05/b.jpg", "jpg", 3)?;
    let expected = sorted(folders::directories(&c)?);
    assert_eq!(expected.len(), 3);
    // schema.sql contains the same table and triggers (new collections).
    let schema = include_str!("../src/core/schema.sql");
    for (name, ddl) in migrations::FOLDER_COUNTS {
        assert!(schema.contains(ddl), "{name} fehlt in schema.sql");
    }
    // A collection of an older version: neither table nor triggers.
    c.db.execute_batch(
        "DROP TRIGGER folder_counts_insert; DROP TRIGGER folder_counts_delete;
         DROP TRIGGER folder_counts_update; DROP TABLE folder_counts;",
    )?;
    drop(c);
    let read = Collection::open(temp.path(), false)?;
    assert_eq!(sorted(folders::directories(&read)?), expected, "fallback");
    drop(read);
    for (name, _) in migrations::FOLDER_COUNTS {
        assert!(!exists(temp.path(), name)?, "lesend: {name} entsteht nicht");
    }
    let write = Collection::open(temp.path(), true)?;
    for (name, _) in migrations::FOLDER_COUNTS {
        assert!(
            exists(temp.path(), name)?,
            "{name} fehlt nach Öffnen mit Schreibrecht"
        );
    }
    assert_consistent(&write, "initial fill")?;
    assert!(!migrations::ensure_folder_counts(&write.db)?, "idempotent");
    // A missing trigger makes the counts unreliable: everything is rebuilt.
    write
        .db
        .execute_batch("DROP TRIGGER folder_counts_insert")?;
    add(&write, 4, "2025/c.jpg", "jpg", 4)?;
    assert!(migrations::ensure_folder_counts(&write.db)?);
    assert_consistent(&write, "repaired")?;
    Ok(())
}

#[test]
fn large_synthetic_tree_loads_in_one_pass_on_the_covering_index() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let mut c = Collection::open(temp.path(), true)?;
    let folders_total = 20_000i64;
    let tx = c.db.transaction()?;
    {
        let mut photo = tx.prepare("INSERT INTO photos(id,status) VALUES(?,'present')")?;
        let mut file = tx.prepare("INSERT INTO files(id,relpath,relpath_fold,dir,name,stem,ext,size,mtime_ns,photo_id,status) VALUES(?,?,?,?,?,?,'jpg',12,1,?,'present')")?;
        for id in 1..=100_000i64 {
            let folder = id % folders_total;
            let dir = format!("Ordner {folder:05}/Unter {}", id % 3);
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
    let plan = folders::plan(&c)?.join(" | ");
    assert!(
        plan.contains("COVERING INDEX ix_files_dir_cover") || plan.contains("ix_files_dir_cover"),
        "{plan}"
    );
    let started = Instant::now();
    let tree = load(&c)?;
    let elapsed = started.elapsed();
    assert_eq!(tree.folder_count(), 20_000 + 60_000);
    let rows = names(&tree);
    assert_eq!(rows.len(), 20_000, "nothing is expanded");
    assert_eq!(rows[0], ("Ordner 00000".to_string(), 5));
    eprintln!("whole tree of 100000 photos in 60000 folders: {elapsed:?}");
    Ok(())
}

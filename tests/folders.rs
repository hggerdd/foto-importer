use foto_importer::core::{
    collection::Collection,
    folders::{self, FolderTree, Row},
    metadata::{self, Search},
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
    // Without the additive index (read-only collection of an older version) the result is equal.
    let covered = folders::directories(&c)?;
    c.db.execute_batch("DROP INDEX ix_files_dir_cover")?;
    let mut fallback = folders::directories(&c)?;
    let mut covered = covered;
    covered.sort();
    fallback.sort();
    assert_eq!(covered, fallback);
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

use foto_importer::core::{
    collection::Collection,
    folders::{self, FOLDER_PAGE},
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

#[test]
fn children_count_photos_below_each_folder() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let c = Collection::open(temp.path(), true)?;
    add(&c, 1, "root.jpg", "jpg", 1)?;
    add(&c, 2, "2024/a.jpg", "jpg", 2)?;
    add(&c, 3, "2024/05/b.jpg", "jpg", 3)?;
    add(&c, 4, "2024/05/c.jpg", "jpg", 4)?;
    add(&c, 5, "2024-x/d.jpg", "jpg", 5)?;
    add(&c, 6, "2024 und mehr/e.jpg", "jpg", 6)?;
    add(&c, 7, "Zebra/f.jpg", "jpg", 7)?;
    // A RAW+JPG pair counts once, a sidecar not at all.
    add(&c, 8, "2024/05/g.nef", "nef", 8)?;
    add(&c, 9, "2024/05/g.jpg", "jpg", 8)?;
    add(&c, 10, "2024/05/g.xmp", "xmp", 8)?;
    let root = folders::children(&c, "", 0)?;
    let names: Vec<_> = root
        .folders
        .iter()
        .map(|f| (f.name.as_str(), f.photos, f.has_children))
        .collect();
    assert_eq!(
        names,
        [
            ("2024", 4, true),
            ("2024 und mehr", 1, false),
            ("2024-x", 1, false),
            ("Zebra", 1, false)
        ]
    );
    assert!(!root.more);
    let year = folders::children(&c, "2024", 0)?;
    assert_eq!(
        year.folders,
        [folders::FolderNode {
            name: "05".into(),
            photos: 3,
            has_children: false
        }]
    );
    assert!(folders::children(&c, "2024/05", 0)?.folders.is_empty());
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
    Ok(())
}

#[test]
fn large_synthetic_tree_is_paged_and_uses_the_directory_index() -> anyhow::Result<()> {
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
    let plan = folders::plan(&c, "")?.join(" | ");
    assert!(plan.contains("ix_files_directory_name"), "{plan}");
    let plan = folders::plan(&c, "Ordner 00007")?.join(" | ");
    assert!(
        plan.contains("USING INDEX ix_files_directory_name (dir>? AND dir<?)"),
        "{plan}"
    );
    let started = Instant::now();
    let first = folders::children(&c, "", 0)?;
    let root_time = started.elapsed();
    assert_eq!(first.folders.len(), FOLDER_PAGE);
    assert!(first.more && first.folders.iter().all(|f| f.has_children));
    assert_eq!(first.folders[0].name, "Ordner 00000");
    assert_eq!(first.folders[0].photos, 5);
    let last = folders::children(&c, "", 99)?;
    assert_eq!(last.folders.len(), FOLDER_PAGE);
    assert!(!last.more);
    assert!(folders::children(&c, "", 100)?.folders.is_empty());
    let started = Instant::now();
    let below = folders::children(&c, "Ordner 00007", 0)?;
    let below_time = started.elapsed();
    assert_eq!(below.folders.len(), 3);
    eprintln!("root page: {root_time:?}, subfolder level: {below_time:?}");
    Ok(())
}

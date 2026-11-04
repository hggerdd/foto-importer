use foto_importer::core::{collection::Collection, maintenance};
use rusqlite::Connection;
fn historical(version: i64, partial: bool) -> anyhow::Result<tempfile::TempDir> {
    let folder = tempfile::tempdir()?;
    std::fs::write(folder.path().join("legacy.jpg"), b"original")?;
    let c = Collection::open(folder.path(), true)?;
    c.db.execute_batch("DELETE FROM collection;INSERT INTO collection VALUES('legacy-id','2020','python');INSERT INTO photos(id,identity_key,status) VALUES(42,'cam:7','present');INSERT INTO files(id,relpath,relpath_fold,dir,name,stem,ext,size,mtime_ns,photo_id,status) VALUES(91,'legacy.jpg','legacy.jpg','','legacy.jpg','legacy','jpg',8,1,42,'present');INSERT INTO meta(file_id,model,dt_original,json) VALUES(91,'Camera','2020:01:01 12:00:00','{}');")?;
    if version >= 2 {
        c.db.execute_batch("INSERT INTO persons(id) VALUES(77);INSERT INTO person_attrs VALUES(77,'name','Legacy name');INSERT INTO faces(id,photo_id,source_file_id,source_relpath,detector_id,person_id,assignment,quality) VALUES(99,42,91,'legacy.jpg','legacy',77,'user',1);INSERT INTO training_set VALUES(77,99,'legacy-model','ref','2020','pin',1);")?;
        c.db.execute(
            "INSERT INTO face_embeddings VALUES(99,'legacy-model',2,?)",
            [vec![0u8, 0, 128, 63, 0, 0, 0, 0]],
        )?;
    }
    drop(c);
    let db = Connection::open(folder.path().join(".myfotomanager"))?;
    let triggers = db
        .prepare("SELECT name FROM sqlite_master WHERE type='trigger'")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for name in triggers {
        db.execute_batch(&format!("DROP TRIGGER {name}"))?;
    }
    let inventory = [
        "collection",
        "settings",
        "photos",
        "files",
        "meta",
        "scans",
        "changes",
    ];
    let faces = [
        "face_settings",
        "persons",
        "person_attrs",
        "faces",
        "face_embeddings",
        "face_rejections",
        "training_set",
        "face_review",
        "relation_types",
        "relations",
        "face_jobs",
    ];
    let tables = db
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for table in tables {
        let keep = inventory.contains(&table.as_str())
            || version >= 2 && faces.contains(&table.as_str())
            || version >= 3 && table == "training_exclusions"
            || version >= 4;
        if !keep {
            db.execute_batch(&format!("DROP TABLE {table}"))?;
        }
    }
    if version >= 4 {
        db.execute_batch("DROP INDEX ix_group_page; CREATE INDEX ix_group_page ON face_group_catalog(embedder_id,group_id) WHERE members>0;")?;
    }
    if partial {
        db.execute_batch("DROP TABLE face_group_dirty;DROP TABLE face_sources;")?;
    }
    db.execute_batch(&format!("PRAGMA user_version={version}"))?;
    drop(db);
    Ok(folder)
}
#[test]
fn upgrade_versions_one_through_five_preserves_source_ids_and_labels() -> anyhow::Result<()> {
    for version in 1..=5 {
        let folder = historical(version, false)?;
        let c = Collection::open(folder.path(), true)?;
        assert_eq!(
            c.db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?,
            6
        );
        assert_eq!(
            c.db.query_row("SELECT photo_id FROM files WHERE id=91", [], |r| r
                .get::<_, i64>(0))?,
            42
        );
        assert_eq!(c.status()?.photos, 1);
        if version >= 2 {
            assert_eq!(
                c.db.query_row(
                    "SELECT value FROM person_attrs WHERE person_id=77 AND key='name'",
                    [],
                    |r| r.get::<_, String>(0)
                )?,
                "Legacy name"
            );
            assert_eq!(
                c.db.query_row("SELECT person_id FROM faces WHERE id=99", [], |r| r
                    .get::<_, i64>(0))?,
                77
            );
            assert_eq!(
                c.db.query_row(
                    "SELECT pinned FROM training_set WHERE face_id=99",
                    [],
                    |r| r.get::<_, i64>(0)
                )?,
                1
            );
            assert_eq!(
                c.db.query_row(
                    "SELECT vec FROM face_embeddings WHERE face_id=99",
                    [],
                    |r| r.get::<_, Vec<u8>>(0)
                )?,
                vec![0u8, 0, 128, 63, 0, 0, 0, 0]
            );
        }
        assert!(
            std::fs::read_dir(folder.path())?
                .filter_map(Result::ok)
                .any(|e| e
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".myfotomanager.before-rust-"))
        );
        assert_eq!(
            std::fs::read(folder.path().join("legacy.jpg"))?,
            b"original"
        );
    }
    Ok(())
}
#[test]
fn repair_partial_five_and_scrub_its_migration_backup() -> anyhow::Result<()> {
    let folder = historical(5, true)?;
    let c = Collection::open(folder.path(), true)?;
    assert_eq!(
        c.db.query_row(
            "SELECT file_id FROM face_sources WHERE photo_id=42",
            [],
            |r| r.get::<_, i64>(0)
        )?,
        91
    );
    maintenance::delete_biometrics(&c)?;
    for entry in std::fs::read_dir(folder.path())? {
        let path = entry?.path();
        if path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .starts_with(".myfotomanager.before-rust-")
        {
            let backup = Connection::open(path)?;
            assert_eq!(
                backup.query_row("SELECT count(*) FROM faces", [], |r| r.get::<_, i64>(0))?,
                0
            );
        }
    }
    Ok(())
}


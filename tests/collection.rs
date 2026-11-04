use foto_importer::core::{
    collection::Collection,
    jobs::Control,
    metadata::{self, Rule, Search},
};
use rusqlite::params;
use std::{collections::BTreeSet, fs};
fn seed(c: &Collection) -> anyhow::Result<()> {
    for id in 1..=3 {
        c.db.execute("INSERT INTO photos(id,status) VALUES(?,'present')", [id])?;
        c.db.execute("INSERT INTO files(id,relpath,relpath_fold,dir,name,stem,ext,size,mtime_ns,photo_id,status) VALUES(?,?,?,'album',?,?,'jpg',12,1,?,'present')",params![id,format!("album/{id}.jpg"),format!("album/{id}.jpg"),format!("{id}.jpg"),id.to_string(),id])?;
    }
    Ok(())
}
#[test]
fn writer_lock_protects_other_instances_and_allows_readers() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let writer = Collection::open(temp.path(), true)?;
    assert!(Collection::open(temp.path(), true).is_err());
    let reader = Collection::open(temp.path(), false)?;
    assert_eq!(reader.status()?.files, 0);
    drop(reader);
    drop(writer);
    assert!(!temp.path().join(".myfotomanager.lock").exists());
    assert!(Collection::open(temp.path(), true).is_ok());
    Ok(())
}
#[test]
fn old_schema_is_rejected_without_modification_or_stranded_lock() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let db = rusqlite::Connection::open(temp.path().join(".myfotomanager"))?;
    db.execute_batch("PRAGMA user_version=5;")?;
    drop(db);
    assert!(Collection::open(temp.path(), true).is_err());
    assert!(!temp.path().join(".myfotomanager.lock").exists());
    let db = rusqlite::Connection::open(temp.path().join(".myfotomanager"))?;
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?,
        5
    );
    Ok(())
}
#[test]
fn repeated_scan_preserves_photo_and_file_ids() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    fs::write(temp.path().join("a.jpg"), b"jpeg")?;
    let mut c = Collection::open(temp.path(), true)?;
    c.quick_scan(false, &Control::default())?;
    let first: (i64, i64) = c.db.query_row("SELECT id,photo_id FROM files", [], |r| {
        Ok((r.get(0)?, r.get(1)?))
    })?;
    c.quick_scan(false, &Control::default())?;
    let second = c.db.query_row("SELECT id,photo_id FROM files", [], |r| {
        Ok((r.get(0)?, r.get(1)?))
    })?;
    assert_eq!(first, second);
    assert_eq!(c.status()?.photos, 1);
    Ok(())
}
#[test]
fn empty_and_cancelled_scans_never_remove_existing_inventory() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("a.jpg");
    fs::write(&path, b"jpeg")?;
    let mut c = Collection::open(temp.path(), true)?;
    c.quick_scan(false, &Control::default())?;
    fs::remove_file(path)?;
    assert!(c.quick_scan(false, &Control::default()).is_err());
    let cancel = Control::default();
    cancel.stop();
    assert!(c.quick_scan(false, &cancel).is_err());
    assert_eq!(c.status()?.files, 1);
    Ok(())
}
#[test]
fn integrity_mismatch_preserves_original_baseline() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("a.jpg");
    fs::write(&path, b"jpeg")?;
    let mut c = Collection::open(temp.path(), true)?;
    c.quick_scan(false, &Control::default())?;
    c.verify(&Control::default())?;
    let before: String =
        c.db.query_row("SELECT full_hash FROM files", [], |r| r.get(0))?;
    fs::write(path, b"changed")?;
    assert!(c.verify(&Control::default()).is_err());
    assert_eq!(
        c.db.query_row("SELECT full_hash FROM files", [], |r| r.get::<_, String>(0))?,
        before
    );
    Ok(())
}
#[test]
fn known_hash_preserves_identity_when_file_id_changes() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let old = temp.path().join("old.jpg");
    let new = temp.path().join("new.jpg");
    fs::write(&old, b"unique camera bytes")?;
    let mut c = Collection::open(temp.path(), true)?;
    c.quick_scan(false, &Control::default())?;
    c.verify(&Control::default())?;
    let id: i64 =
        c.db.query_row("SELECT photo_id FROM files", [], |r| r.get(0))?;
    fs::copy(&old, &new)?;
    fs::remove_file(old)?;
    c.quick_scan(false, &Control::default())?;
    assert_eq!(
        c.db.query_row(
            "SELECT photo_id FROM files WHERE relpath='new.jpg'",
            [],
            |r| r.get::<_, i64>(0)
        )?,
        id
    );
    assert_eq!(c.status()?.files, 1);
    Ok(())
}
#[test]
fn ignored_faces_count_for_strict_search_and_unanalysed_is_not_empty() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let c = Collection::open(temp.path(), true)?;
    seed(&c)?;
    c.db.execute_batch("INSERT INTO persons(id) VALUES(1); INSERT INTO faces(photo_id,source_relpath,detector_id,bbox_x1,bbox_y1,bbox_x2,bbox_y2,person_id,assignment) VALUES(1,'album/1.jpg','det',0,0,1,1,1,'user'); INSERT INTO faces(photo_id,source_relpath,detector_id,bbox_x1,bbox_y1,bbox_x2,bbox_y2,ignored) VALUES(1,'album/1.jpg','det',0,0,1,1,1); INSERT INTO face_jobs(photo_id,stage,model_id,status) VALUES(1,'detect','det','done'),(2,'detect','det','done');")?;
    let search = Search {
        people_all: vec![1],
        only_people: true,
        ..Default::default()
    };
    assert_eq!(metadata::search(&c, &search, "det")?.total, 0);
    let search = Search {
        face_state: "none".into(),
        ..Default::default()
    };
    let page = metadata::search(&c, &search, "det")?;
    assert_eq!(
        page.photos.iter().map(|p| p.id).collect::<BTreeSet<_>>(),
        BTreeSet::from([2])
    );
    Ok(())
}
#[test]
fn bound_values_cannot_change_query_structure() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let c = Collection::open(temp.path(), true)?;
    seed(&c)?;
    let search = Search {
        text: "' OR 1=1 --".into(),
        ..Default::default()
    };
    assert_eq!(metadata::search(&c, &search, "")?.total, 0);
    assert!(
        metadata::compile_rule(&Rule {
            field: "drop table".into(),
            operator: "is".into(),
            value: "1".into()
        })
        .is_err()
    );
    assert!(
        metadata::compile_rule(&Rule {
            field: "iso".into(),
            operator: "min".into(),
            value: "NaN".into()
        })
        .is_err()
    );
    Ok(())
}

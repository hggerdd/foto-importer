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
fn explicit_purge_preserves_photo_anchors_and_labels() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    for id in 0..5 {
        fs::write(temp.path().join(format!("{id}.jpg")), b"photo")?;
    }
    let mut c = Collection::open(temp.path(), true)?;
    c.quick_scan(false, &Control::default())?;
    let photo: i64 = c.db.query_row(
        "SELECT photo_id FROM files WHERE relpath='0.jpg'",
        [],
        |r| r.get(0),
    )?;
    c.db.execute("INSERT INTO persons(id) VALUES(77)", [])?;
    c.db.execute("INSERT INTO faces(photo_id,source_relpath,detector_id,person_id,assignment) VALUES(?,'0.jpg','legacy',77,'user')",[photo])?;
    fs::remove_file(temp.path().join("0.jpg"))?;
    c.quick_scan(false, &Control::default())?;
    assert_eq!(c.status()?.missing, 1);
    c.purge_missing(0)?;
    assert_eq!(
        c.db.query_row("SELECT count(*) FROM photos WHERE id=?", [photo], |r| r
            .get::<_, i64>(0))?,
        1
    );
    assert_eq!(
        c.db.query_row(
            "SELECT person_id FROM faces WHERE photo_id=?",
            [photo],
            |r| r.get::<_, i64>(0)
        )?,
        77
    );
    Ok(())
}
#[test]
fn explicit_ambiguous_move_retains_the_reviewed_anchor() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    for name in ["a.jpg", "b.jpg"] {
        fs::write(temp.path().join(name), b"samebits")?;
    }
    let mut c = Collection::open(temp.path(), true)?;
    c.quick_scan(false, &Control::default())?;
    c.verify(&Control::default())?;
    let anchor: i64 = c.db.query_row(
        "SELECT photo_id FROM files WHERE relpath='a.jpg'",
        [],
        |r| r.get(0),
    )?;
    fs::write(temp.path().join("new.jpg"), b"samebits")?;
    fs::remove_file(temp.path().join("a.jpg"))?;
    fs::remove_file(temp.path().join("b.jpg"))?;
    c.quick_scan(false, &Control::default())?;
    c.resolve_move("a.jpg", "new.jpg", &Control::default())?;
    assert_eq!(
        c.db.query_row(
            "SELECT photo_id FROM files WHERE relpath='new.jpg'",
            [],
            |r| r.get::<_, i64>(0)
        )?,
        anchor
    );
    assert_eq!(fs::read(temp.path().join("new.jpg"))?, b"samebits");
    Ok(())
}
#[test]
fn cancelled_sql_query_is_interrupted_on_the_worker() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let c = Collection::open(root.path(), true)?;
    let control = Control::default();
    c.cancel_with(&control);
    control.stop();
    let result=c.db.query_row("WITH RECURSIVE numbers(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM numbers WHERE i<1000000000) SELECT sum(i) FROM numbers",[],|r|r.get::<_,i64>(0));
    assert!(result.is_err());
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
fn face_names_and_counts_are_searchable_photo_metadata() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let c = Collection::open(temp.path(), true)?;
    seed(&c)?;
    c.db.execute_batch(
        "INSERT INTO persons(id) VALUES(1);
         INSERT INTO person_attrs(person_id,key,value) VALUES(1,'name','Jörg Weiß');
         INSERT INTO faces(photo_id,source_relpath,detector_id,bbox_x1,bbox_y1,bbox_x2,bbox_y2,
                           person_id,assignment,retained,ignored) VALUES
             (1,'album/1.jpg','det',0,0,1,1,1,'user',0,0),
             (1,'album/1.jpg','det',0,0,1,1,NULL,NULL,0,1),
             (2,'album/2.jpg','old',0,0,1,1,1,'user',1,0),
             (3,'album/3.jpg','old',0,0,1,1,1,'suggested',0,0);
         INSERT INTO face_jobs(photo_id,stage,model_id,status) VALUES
             (1,'detect','det','done'),(2,'detect','det','done'),(3,'detect','old','done');",
    )?;
    let ids = |search: &Search| -> anyhow::Result<BTreeSet<i64>> {
        Ok(metadata::search(&c, search, "det")?
            .photos
            .into_iter()
            .map(|p| p.id)
            .collect())
    };
    let mut search = Search {
        text: "WEISS".into(),
        ..Default::default()
    };
    assert_eq!(ids(&search)?, BTreeSet::from([1]));
    search.suggestions = true;
    assert_eq!(ids(&search)?, BTreeSet::from([1, 3]));
    search.suggestions = false;
    search.rules.push(Rule {
        field: "face_count".into(),
        operator: "is".into(),
        value: "2".into(),
    });
    assert_eq!(ids(&search)?, BTreeSet::from([1]));
    search.text.clear();
    search.rules[0].value = "0".into();
    assert_eq!(ids(&search)?, BTreeSet::from([2]));
    search.rules[0].operator = "empty".into();
    assert_eq!(ids(&search)?, BTreeSet::from([3]));
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
